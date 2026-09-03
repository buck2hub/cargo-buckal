use std::{
    env, fs, io,
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
    header::{CONTENT_LENGTH, USER_AGENT},
};

use crate::{buckal_log, buckal_note, user_agent, utils::UnwrapOrExit};

pub const BUCK2_RELEASE_VERSION: &str = "2026-04-15";
const BUCK2_RELEASE_DOWNLOAD_BASE: &str = "https://github.com/facebook/buck2/releases/download";

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
        .build()
        .context("failed to create HTTP client")?;

    let mut failures = Vec::new();
    for asset in assets {
        let url = release_url(&asset.archive_name);
        buckal_log!("Fetching", &url);
        let _ = io::stdout().flush();
        match download_and_install(&client, &url, destination) {
            Ok(action) => {
                return Ok(InstallOutcome::Installed {
                    destination: destination.to_path_buf(),
                    action,
                });
            }
            Err(error) => failures.push(format!("{}: {:#}", url, error)),
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

fn download_and_install(client: &Client, url: &str, destination: &Path) -> Result<InstallAction> {
    let mut response = client
        .get(url)
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

    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<u64>().ok());

    let temp_path = temporary_install_path(destination);
    let decode_result = (|| -> Result<InstallAction> {
        let temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;

        let progress = download_progress_bar(content_length);
        let download_result = {
            let progress_reader = progress.wrap_read(&mut response);
            match ruzstd::decoding::StreamingDecoder::new(progress_reader) {
                Ok(mut decoder) => decode_to_file(&mut decoder, temp_file).map_err(Into::into),
                Err(error) => Err(anyhow!("failed to start zstd decoder: {}", error)),
            }
        };
        progress.finish_and_clear();
        download_result?;

        set_executable(&temp_path)?;
        let action = install_action_for(destination);
        replace_file(&temp_path, destination)?;
        Ok(action)
    })();

    if decode_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    decode_result
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
    let cargo_home = cargo_home()?;
    Ok(buck2_destination_in(&cargo_home, env::consts::OS))
}

fn cargo_home() -> Result<PathBuf> {
    if let Some(cargo_home) = non_empty_env("CARGO_HOME") {
        return Ok(PathBuf::from(cargo_home));
    }

    Ok(home_dir()?.join(".cargo"))
}

fn home_dir() -> Result<PathBuf> {
    if let Some(home) = non_empty_env("HOME") {
        return Ok(PathBuf::from(home));
    }

    #[cfg(windows)]
    {
        if let Some(profile) = non_empty_env("USERPROFILE") {
            return Ok(PathBuf::from(profile));
        }

        if let (Some(drive), Some(path)) = (non_empty_env("HOMEDRIVE"), non_empty_env("HOMEPATH")) {
            return Ok(PathBuf::from(format!("{drive}{path}")));
        }
    }

    Err(anyhow!(
        "could not determine home directory; set CARGO_HOME to install buck2"
    ))
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}

fn buck2_destination_in(cargo_home: &Path, os: &str) -> PathBuf {
    cargo_home.join("bin").join(executable_name(os))
}

fn executable_name(os: &str) -> &'static str {
    if os == "windows" {
        "buck2.exe"
    } else {
        "buck2"
    }
}

fn buck2_assets_for_current_target(
    linux_variant: Option<LinuxLibcVariant>,
) -> Result<Vec<Buck2Asset>> {
    buck2_assets_for(env::consts::OS, env::consts::ARCH, linux_variant).ok_or_else(|| {
        anyhow!(
            "unsupported platform or variant: {}-{}{}",
            env::consts::OS,
            env::consts::ARCH,
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

fn release_url(archive_name: &str) -> String {
    format!("{BUCK2_RELEASE_DOWNLOAD_BASE}/{BUCK2_RELEASE_VERSION}/{archive_name}")
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
    fn builds_release_url_from_version_constant() {
        assert_eq!(
            release_url("buck2-x86_64-unknown-linux-gnu.zst"),
            format!(
                "https://github.com/facebook/buck2/releases/download/{BUCK2_RELEASE_VERSION}/buck2-x86_64-unknown-linux-gnu.zst"
            )
        );
    }

    #[test]
    fn chooses_destination_binary_name_by_platform() {
        let cargo_home = PathBuf::from("/home/example/.cargo");
        assert_eq!(
            buck2_destination_in(&cargo_home, "linux"),
            PathBuf::from("/home/example/.cargo/bin/buck2")
        );
        assert_eq!(
            buck2_destination_in(&cargo_home, "windows"),
            PathBuf::from("/home/example/.cargo/bin/buck2.exe")
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
