use std::{
    env::consts,
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use inquire::Confirm;
use reqwest::{
    blocking::Client,
    header::{ACCEPT, CONTENT_LENGTH, USER_AGENT},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{buckal_log, buckal_note, user_agent, utils::UnwrapOrExit};

pub const BUCK2_RELEASE_VERSION: &str = "2026-04-15";
const BUCK2_RELEASE_API_BASE: &str = "https://api.github.com/repos/facebook/buck2/releases/tags";
const GITHUB_API_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_API_VERSION: &str = "2022-11-28";
const BUCK2_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const BUCK2_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Parser, Debug)]
pub struct SetupArgs {
    /// Overwrite an existing buck2 binary without prompting
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,

    /// Linux libc variant to install
    #[arg(long, value_enum)]
    pub variant: Option<LinuxLibcVariant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed {
        destination: PathBuf,
        action: InstallAction,
    },
    Skipped(PathBuf),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InstallAction {
    Installing,
    Replacing,
}

impl InstallAction {
    fn log_label(self) -> &'static str {
        match self {
            Self::Installing => "Installing",
            Self::Replacing => "Replacing",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum LinuxLibcVariant {
    Gnu,
    Musl,
}

impl LinuxLibcVariant {
    fn as_target_env(self) -> &'static str {
        match self {
            Self::Gnu => "gnu",
            Self::Musl => "musl",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Buck2Asset {
    archive_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Buck2Download {
    archive_name: String,
    url: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

pub fn execute(args: &SetupArgs) {
    match install_buck2_with_variant(args.yes, args.variant)
        .unwrap_or_exit_ctx("failed to install buck2")
    {
        InstallOutcome::Installed {
            destination,
            action,
        } => {
            let bin_dir = destination.parent().unwrap_or(destination.as_path());
            buckal_log!(action.log_label(), destination.display());
            buckal_note!("make sure {} is in your PATH", bin_dir.display());
            let _ = io::stdout().flush();
        }
        InstallOutcome::Skipped(destination) => {
            buckal_log!(
                "Skipped",
                format!("{} already exists", destination.display())
            );
        }
    }
}

pub fn install_buck2(force: bool) -> Result<InstallOutcome> {
    install_buck2_with_variant(force, None)
}

pub fn install_buck2_with_variant(
    force: bool,
    linux_variant: Option<LinuxLibcVariant>,
) -> Result<InstallOutcome> {
    let destination = default_buck2_destination()?;
    let assets = buck2_assets_for_current_target(linux_variant)?;
    install_buck2_from_assets(&assets, &destination, force)
}

fn install_buck2_from_assets(
    assets: &[Buck2Asset],
    destination: &Path,
    force: bool,
) -> Result<InstallOutcome> {
    if destination.exists() && !force && !confirm_overwrite(destination)? {
        return Ok(InstallOutcome::Skipped(destination.to_path_buf()));
    }

    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("invalid buck2 destination: {}", destination.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let client = Client::builder()
        .user_agent(user_agent())
        .connect_timeout(BUCK2_CONNECT_TIMEOUT)
        .timeout(BUCK2_DOWNLOAD_TIMEOUT)
        .build()
        .context("failed to create HTTP client")?;
    let release = fetch_buck2_release(&client)?;

    let mut failures = Vec::new();
    for asset in assets {
        let download = match release_download_for(&release, asset) {
            Ok(download) => download,
            Err(error) => {
                failures.push(format!("{}: {:#}", asset.archive_name, error));
                continue;
            }
        };
        buckal_log!("Fetching", &download.url);
        let _ = io::stdout().flush();
        match download_and_install(&client, &download, destination) {
            Ok(action) => {
                return Ok(InstallOutcome::Installed {
                    destination: destination.to_path_buf(),
                    action,
                });
            }
            Err(error) => failures.push(format!("{}: {:#}", download.url, error)),
        }
    }

    Err(anyhow!(
        "failed to download a buck2 binary for this platform:\n{}",
        failures.join("\n")
    ))
}

fn confirm_overwrite(destination: &Path) -> Result<bool> {
    Confirm::new(&format!(
        "{} already exists. Overwrite it?",
        destination.display()
    ))
    .with_default(false)
    .prompt()
    .map_err(|e| anyhow!("confirmation failed: {}", e))
}

fn fetch_buck2_release(client: &Client) -> Result<GithubRelease> {
    let url = release_api_url();
    let response = client
        .get(&url)
        .header(ACCEPT, GITHUB_API_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header(USER_AGENT, user_agent())
        .send()
        .map_err(|error| anyhow!("request to {} failed: {:?}", url, error))?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "{} returned HTTP {} from {}",
            url,
            status,
            response.url()
        ));
    }

    response
        .json()
        .with_context(|| format!("failed to parse GitHub release metadata from {url}"))
}

fn release_download_for(release: &GithubRelease, asset: &Buck2Asset) -> Result<Buck2Download> {
    let release_asset = release
        .assets
        .iter()
        .find(|release_asset| release_asset.name == asset.archive_name)
        .ok_or_else(|| anyhow!("release asset {} was not found", asset.archive_name))?;

    Ok(Buck2Download {
        archive_name: asset.archive_name.clone(),
        url: release_asset.browser_download_url.clone(),
        sha256: parse_sha256_digest(&asset.archive_name, release_asset.digest.as_deref())?,
    })
}

fn parse_sha256_digest(archive_name: &str, digest: Option<&str>) -> Result<String> {
    let digest =
        digest.ok_or_else(|| anyhow!("release asset {archive_name} is missing a digest"))?;
    let (algorithm, value) = digest
        .split_once(':')
        .ok_or_else(|| anyhow!("release asset {archive_name} has invalid digest {digest}"))?;

    if !algorithm.eq_ignore_ascii_case("sha256") {
        return Err(anyhow!(
            "release asset {archive_name} uses unsupported digest algorithm {algorithm}"
        ));
    }

    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "release asset {archive_name} has invalid SHA-256 digest {digest}"
        ));
    }

    Ok(value.to_ascii_lowercase())
}

fn download_and_install(
    client: &Client,
    download: &Buck2Download,
    destination: &Path,
) -> Result<InstallAction> {
    let temp_path = temporary_install_path(destination);
    let archive_path = temporary_archive_path(&temp_path);
    let install_result = (|| -> Result<InstallAction> {
        download_archive(client, download, &archive_path)?;

        let archive_file = fs::File::open(&archive_path)
            .with_context(|| format!("failed to open {}", archive_path.display()))?;
        let temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;

        match ruzstd::decoding::StreamingDecoder::new(archive_file) {
            Ok(mut decoder) => decode_to_file(&mut decoder, temp_file).map_err(Into::into),
            Err(error) => Err(anyhow!("failed to start zstd decoder: {}", error)),
        }?;

        set_executable(&temp_path)?;
        let action = install_action_for(destination);
        replace_file(&temp_path, destination)?;
        Ok(action)
    })();

    let _ = fs::remove_file(&archive_path);
    if install_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    install_result
}

fn download_archive(client: &Client, download: &Buck2Download, archive_path: &Path) -> Result<()> {
    let mut response = client
        .get(&download.url)
        .header(USER_AGENT, user_agent())
        .send()
        .map_err(|error| anyhow!("request to {} failed: {:?}", download.url, error))?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "{} returned HTTP {} from {}",
            download.url,
            status,
            response.url()
        ));
    }

    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<u64>().ok());

    let archive_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(archive_path)
        .with_context(|| format!("failed to create {}", archive_path.display()))?;

    let progress = download_progress_bar(content_length);
    let download_result = {
        let mut progress_reader = progress.wrap_read(&mut response);
        download_to_file_with_sha256(&mut progress_reader, archive_file)
    };
    progress.finish_and_clear();
    let actual_sha256 = download_result?;

    if actual_sha256 != download.sha256 {
        return Err(anyhow!(
            "checksum mismatch for {}: expected sha256:{}, got sha256:{}",
            download.archive_name,
            download.sha256,
            actual_sha256
        ));
    }

    Ok(())
}

fn download_to_file_with_sha256<R: Read>(reader: &mut R, mut file: fs::File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
        file.write_all(&buffer[..bytes_read])?;
    }

    file.sync_all()?;
    Ok(hex::encode(hasher.finalize()))
}

fn decode_to_file<R: Read>(reader: &mut R, mut file: fs::File) -> io::Result<()> {
    io::copy(reader, &mut file)?;
    file.sync_all()
}

fn install_action_for(destination: &Path) -> InstallAction {
    if destination.exists() {
        InstallAction::Replacing
    } else {
        InstallAction::Installing
    }
}

fn download_progress_bar(content_length: Option<u64>) -> ProgressBar {
    let progress = match content_length {
        Some(length) if length > 0 => {
            ProgressBar::with_draw_target(Some(length), ProgressDrawTarget::stdout())
        }
        _ => ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout()),
    };

    let style = match content_length {
        Some(length) if length > 0 => ProgressStyle::with_template(
            "{prefix:>12} [{wide_bar:.cyan/blue}] {percent:>3}% {bytes}/{total_bytes}",
        )
        .expect("valid progress bar template")
        .progress_chars("=> "),
        _ => ProgressStyle::with_template(
            "{prefix:>12} {spinner} {bytes} {bytes_per_sec} {wide_msg}",
        )
        .expect("valid progress spinner template")
        .tick_strings(&["-", "\\", "|", "/"]),
    };

    progress.set_style(style);
    progress.set_prefix("Downloading");
    progress.enable_steady_tick(Duration::from_millis(100));
    progress
}

fn default_buck2_destination() -> Result<PathBuf> {
    let cargo_home =
        home::cargo_home().context("could not determine Cargo home directory to install buck2")?;
    let dest = cargo_home.join("bin").join(if consts::OS == "windows" {
        "buck2.exe"
    } else {
        "buck2"
    });
    Ok(dest)
}

fn buck2_assets_for_current_target(
    linux_variant: Option<LinuxLibcVariant>,
) -> Result<Vec<Buck2Asset>> {
    buck2_assets_for(consts::OS, consts::ARCH, linux_variant).ok_or_else(|| {
        anyhow!(
            "unsupported platform or variant: {}-{}{}",
            consts::OS,
            consts::ARCH,
            linux_variant
                .map(|variant| format!(" ({})", variant.as_target_env()))
                .unwrap_or_default()
        )
    })
}

fn buck2_assets_for(
    os: &str,
    arch: &str,
    linux_variant: Option<LinuxLibcVariant>,
) -> Option<Vec<Buck2Asset>> {
    let triples: Vec<String> = match (os, arch) {
        ("linux", "x86_64") => vec![format!(
            "x86_64-unknown-linux-{}",
            selected_linux_variant(linux_variant).as_target_env()
        )],
        ("linux", "aarch64") => vec![format!(
            "aarch64-unknown-linux-{}",
            selected_linux_variant(linux_variant).as_target_env()
        )],
        ("linux", "riscv64") => match selected_linux_variant(linux_variant) {
            LinuxLibcVariant::Gnu => vec!["riscv64gc-unknown-linux-gnu".to_string()],
            LinuxLibcVariant::Musl => return None,
        },
        ("macos", "x86_64") if linux_variant.is_none() => vec!["x86_64-apple-darwin".to_string()],
        ("macos", "aarch64") if linux_variant.is_none() => vec!["aarch64-apple-darwin".to_string()],
        ("windows", "x86_64") if linux_variant.is_none() => {
            vec!["x86_64-pc-windows-msvc.exe".to_string()]
        }
        ("windows", "aarch64") if linux_variant.is_none() => {
            vec!["aarch64-pc-windows-msvc.exe".to_string()]
        }
        _ => return None,
    };

    Some(
        triples
            .into_iter()
            .map(|triple| Buck2Asset {
                archive_name: format!("buck2-{}.zst", triple),
            })
            .collect(),
    )
}

fn selected_linux_variant(variant: Option<LinuxLibcVariant>) -> LinuxLibcVariant {
    variant.unwrap_or_else(default_linux_variant)
}

fn default_linux_variant() -> LinuxLibcVariant {
    if cfg!(target_env = "musl") {
        LinuxLibcVariant::Musl
    } else {
        LinuxLibcVariant::Gnu
    }
}

fn release_api_url() -> String {
    format!("{BUCK2_RELEASE_API_BASE}/{BUCK2_RELEASE_VERSION}")
}

fn temporary_install_path(destination: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("buck2");
    destination.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        timestamp
    ))
}

fn temporary_archive_path(temp_path: &Path) -> PathBuf {
    let file_name = temp_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(".buck2.tmp");
    temp_path.with_file_name(format!("{file_name}.zst"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temp_path, destination)
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp_path, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_linux_x86_64_gnu_asset() {
        let assets = buck2_assets_for("linux", "x86_64", Some(LinuxLibcVariant::Gnu))
            .expect("linux x86_64 gnu should be supported");

        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.archive_name.as_str())
                .collect::<Vec<_>>(),
            vec!["buck2-x86_64-unknown-linux-gnu.zst"]
        );
    }

    #[test]
    fn maps_linux_x86_64_musl_asset() {
        let assets = buck2_assets_for("linux", "x86_64", Some(LinuxLibcVariant::Musl))
            .expect("linux x86_64 musl should be supported");

        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.archive_name.as_str())
                .collect::<Vec<_>>(),
            vec!["buck2-x86_64-unknown-linux-musl.zst"]
        );
    }

    #[test]
    fn maps_macos_aarch64_asset() {
        let assets =
            buck2_assets_for("macos", "aarch64", None).expect("macos aarch64 should be supported");

        assert_eq!(assets[0].archive_name, "buck2-aarch64-apple-darwin.zst");
    }

    #[test]
    fn maps_windows_x86_64_asset_with_exe_suffix() {
        let assets = buck2_assets_for("windows", "x86_64", None)
            .expect("windows x86_64 should be supported");

        assert_eq!(
            assets[0].archive_name,
            "buck2-x86_64-pc-windows-msvc.exe.zst"
        );
    }

    #[test]
    fn rejects_unsupported_platforms() {
        assert!(buck2_assets_for("freebsd", "x86_64", None).is_none());
        assert!(buck2_assets_for("linux", "powerpc64", None).is_none());
    }

    #[test]
    fn rejects_linux_variant_on_non_linux_platforms() {
        assert!(buck2_assets_for("macos", "aarch64", Some(LinuxLibcVariant::Gnu)).is_none());
        assert!(buck2_assets_for("windows", "x86_64", Some(LinuxLibcVariant::Musl)).is_none());
    }

    #[test]
    fn rejects_missing_linux_musl_asset() {
        assert!(buck2_assets_for("linux", "riscv64", Some(LinuxLibcVariant::Musl)).is_none());
    }

    #[test]
    fn builds_release_api_url_from_version_constant() {
        assert_eq!(
            release_api_url(),
            format!(
                "https://api.github.com/repos/facebook/buck2/releases/tags/{BUCK2_RELEASE_VERSION}"
            )
        );
    }

    #[test]
    fn builds_download_metadata_from_release_asset() {
        let expected_sha256 = "a".repeat(64);
        let release = GithubRelease {
            assets: vec![GithubReleaseAsset {
                name: "buck2-x86_64-unknown-linux-gnu.zst".to_string(),
                browser_download_url: "https://example.com/buck2.zst".to_string(),
                digest: Some(format!("sha256:{expected_sha256}")),
            }],
        };
        let asset = Buck2Asset {
            archive_name: "buck2-x86_64-unknown-linux-gnu.zst".to_string(),
        };

        let download = release_download_for(&release, &asset).expect("asset should resolve");

        assert_eq!(download.archive_name, asset.archive_name);
        assert_eq!(download.url, "https://example.com/buck2.zst");
        assert_eq!(download.sha256, expected_sha256);
    }

    #[test]
    fn rejects_release_asset_without_digest() {
        let release = GithubRelease {
            assets: vec![GithubReleaseAsset {
                name: "buck2-x86_64-unknown-linux-gnu.zst".to_string(),
                browser_download_url: "https://example.com/buck2.zst".to_string(),
                digest: None,
            }],
        };
        let asset = Buck2Asset {
            archive_name: "buck2-x86_64-unknown-linux-gnu.zst".to_string(),
        };

        let error = release_download_for(&release, &asset).expect_err("missing digest should fail");

        assert!(error.to_string().contains("missing a digest"));
    }

    #[test]
    fn rejects_non_sha256_digest() {
        let error = parse_sha256_digest(
            "buck2.zst",
            Some("sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        )
        .expect_err("non-sha256 digest should fail");

        assert!(error.to_string().contains("unsupported digest algorithm"));
    }

    #[test]
    fn rejects_invalid_sha256_digest() {
        let error = parse_sha256_digest("buck2.zst", Some("sha256:not-a-valid-hex-digest"))
            .expect_err("invalid sha256 digest should fail");

        assert!(error.to_string().contains("invalid SHA-256 digest"));
    }

    #[test]
    fn writes_download_and_returns_sha256() {
        let temp_dir = tempfile::TempDir::new().expect("failed to create temp dir");
        let archive_path = temp_dir.path().join("asset.zst");
        let archive_file = fs::File::create(&archive_path).expect("failed to create archive file");
        let mut reader = &b"abc"[..];

        let actual_sha256 =
            download_to_file_with_sha256(&mut reader, archive_file).expect("hash should compute");

        assert_eq!(
            actual_sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            fs::read(&archive_path).expect("archive file should exist"),
            b"abc"
        );
    }

    #[test]
    fn detects_install_action_from_destination() {
        let temp_dir = tempfile::TempDir::new().expect("failed to create temp dir");
        let destination = temp_dir.path().join("buck2");

        assert_eq!(install_action_for(&destination), InstallAction::Installing);

        fs::write(&destination, b"old buck2").expect("failed to write destination");
        assert_eq!(install_action_for(&destination), InstallAction::Replacing);
    }
}
