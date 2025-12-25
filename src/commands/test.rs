use crate::{
    buck2::Buck2Command,
    utils::{UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_buck2_root},
};
use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use std::process::exit;

#[derive(Parser, Debug)]
#[command(disable_version_flag = true)]
pub struct TestArgs {
    #[arg(short, long, value_name = "SPEC")]
    pub package: Vec<String>,

    #[arg(long)]
    pub workspace: bool,

    #[arg(long, value_name = "SPEC")]
    pub exclude: Vec<String>,

    #[arg(long)]
    pub all_targets: bool,

    #[arg(long)]
    pub lib: bool,

    #[arg(long, value_name = "NAME")]
    pub bin: Vec<String>,

    #[arg(long)]
    pub bins: bool,

    #[arg(long, value_name = "NAME")]
    pub example: Vec<String>,

    #[arg(long)]
    pub examples: bool,

    #[arg(long, value_name = "NAME")]
    pub test: Vec<String>,

    #[arg(long)]
    pub tests: bool,

    #[arg(long, value_name = "NAME")]
    pub bench: Vec<String>,

    #[arg(long)]
    pub benches: bool,

    #[arg(long)]
    pub doc: bool,

    #[arg(long)]
    pub no_run: bool,

    #[arg(long)]
    pub no_fail_fast: bool,

    #[arg(short = 'F', long, value_name = "FEATURES")]
    pub features: Vec<String>,

    #[arg(long)]
    pub all_features: bool,

    #[arg(long)]
    pub no_default_features: bool,

    #[arg(short, long, value_name = "N")]
    pub jobs: Option<usize>,

    #[arg(long, value_name = "TRIPLE")]
    pub target: Option<String>,

    #[arg(short, long)]
    pub release: bool,

    #[arg(long, value_name = "PROFILE-NAME")]
    pub profile: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<String>,

    #[arg(long)]
    pub ignore_rust_version: bool,

    #[arg(long)]
    pub locked: bool,

    #[arg(long)]
    pub offline: bool,

    #[arg(long)]
    pub frozen: bool,

    #[arg(value_name = "TESTNAME")]
    pub test_name: Option<String>,

    #[arg(last = true)]
    pub args: Vec<String>,
}

#[allow(clippy::collapsible_if)]
pub fn execute(args: &TestArgs) {
    ensure_prerequisites().unwrap_or_exit();
    check_buck2_package().unwrap_or_exit();

    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")
        .unwrap_or_exit();

    let buck2_root = get_buck2_root().unwrap_or_exit();

    let (targets, is_specific_target) = resolve_targets(args, &metadata, &buck2_root)
        .unwrap_or_exit_ctx("failed to resolve targets");

    if targets.is_empty() {
        eprintln!("No targets found to test.");
        return;
    }

    let mut cmd = if args.no_run {
        Buck2Command::new().arg("build")
    } else {
        Buck2Command::new().arg("test")
    };

    for target in &targets {
        cmd = cmd.arg(target);
    }

    for excluded_pkg in &args.exclude {
        if let Some(pkg) = metadata
            .packages
            .iter()
            .find(|p| p.name.as_str() == excluded_pkg)
        {
            let pkg_path = pkg.manifest_path.parent().unwrap();
            let relative = pkg_path.strip_prefix(&buck2_root).unwrap_or_exit();
            let rel_str = relative.as_str().trim_start_matches('/');
            let pattern = if rel_str.is_empty() {
                "//...".to_string()
            } else {
                format!("//{}/...", rel_str)
            };
            cmd = cmd.arg("--exclude").arg(pattern);
        }
    }

    if let Some(jobs) = args.jobs {
        cmd = cmd.arg("-j").arg(jobs.to_string());
    }

    if let Some(target) = &args.target {
        cmd = cmd.arg("--target-platforms").arg(target);
    }

    if args.release {
        cmd = cmd.arg("-m").arg("release");
    } else if let Some(profile) = &args.profile {
        cmd = cmd.arg("-m").arg(profile);
    }

    if args.no_fail_fast {
        cmd = cmd.arg("--keep-going");
    }

    if !args.no_run {
        let mut passthrough_args = Vec::new();
        if !is_specific_target {
            if let Some(name) = &args.test_name {
                passthrough_args.push(name.clone());
            }
        }
        passthrough_args.extend_from_slice(&args.args);

        if !passthrough_args.is_empty() {
            cmd = cmd.arg("--").arg("--");
            for arg in passthrough_args {
                cmd = cmd.arg(arg);
            }
        }
    }

    let status = cmd.status().unwrap_or_exit_ctx("failed to execute buck2");

    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

#[allow(clippy::collapsible_if)]
fn resolve_targets(
    args: &TestArgs,
    metadata: &cargo_metadata::Metadata,
    buck2_root: &cargo_metadata::camino::Utf8Path,
) -> Result<(Vec<String>, bool)> {
    let mut patterns = Vec::new();

    if let Some(name) = &args.test_name {
        let name_norm = name.replace("-", "_");
        let mut found_in_metadata = false;

        for pkg in &metadata.packages {
            for target in &pkg.targets {
                let file_stem = target.src_path.file_stem().unwrap_or("");
                let target_name_norm = target.name.replace("-", "_");
                let file_stem_norm = file_stem.replace("-", "_");

                if (target_name_norm == name_norm || file_stem_norm == name_norm)
                    && target.kind.iter().any(|k| k.to_string() == "test")
                {
                    if let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root) {
                        patterns.push(owner);
                        found_in_metadata = true;
                    }
                }
            }
        }

        if !found_in_metadata {
            let root_path = buck2_root.as_std_path();
            if let Some(file_path) = find_file_recursive(root_path, name) {
                if let Ok(owner) = query_buck2_test_owner_std(&file_path, buck2_root) {
                    patterns.push(owner);
                    return Ok((patterns, true));
                }
            }
        } else {
            return Ok((patterns, true));
        }
    }

    if !args.test.is_empty() {
        for t_name in &args.test {
            let mut found_local = false;
            for pkg in &metadata.packages {
                for target in &pkg.targets {
                    let file_stem = target.src_path.file_stem().unwrap_or("");
                    if (target.name == *t_name || file_stem == *t_name)
                        && target.kind.iter().any(|k| k.to_string() == "test")
                    {
                        if let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root) {
                            patterns.push(owner);
                            found_local = true;
                        }
                    }
                }
            }
            if !found_local {
                let root_path = buck2_root.as_std_path();
                if let Some(file_path) = find_file_recursive(root_path, t_name) {
                    if let Ok(owner) = query_buck2_test_owner_std(&file_path, buck2_root) {
                        patterns.push(owner);
                    }
                }
            }
        }
        if !patterns.is_empty() {
            return Ok((patterns, true));
        }
    }

    if args.workspace {
        patterns.push("//...".to_string());
    } else if !args.package.is_empty() {
        for pkg_name in &args.package {
            if let Some(pkg) = metadata
                .packages
                .iter()
                .find(|p| p.name.as_str() == pkg_name)
            {
                let pkg_path = pkg.manifest_path.parent().unwrap();
                let relative = pkg_path
                    .strip_prefix(buck2_root)
                    .map_err(|_| anyhow!("Package {} outside root", pkg_name))?;
                let rel_str = relative.as_str().trim_start_matches('/');
                patterns.push(if rel_str.is_empty() {
                    "//...".to_string()
                } else {
                    format!("//{}/...", rel_str)
                });
            }
        }
    } else {
        let current_dir = std::env::current_dir()?;
        let relative = current_dir
            .strip_prefix(buck2_root.as_std_path())
            .map_err(|_| anyhow!("Outside project"))?;
        let rel_str = relative.to_str().unwrap().trim_start_matches('/');
        patterns.push(if rel_str.is_empty() {
            "//...".to_string()
        } else {
            format!("//{}/...", rel_str)
        });
    }

    Ok((patterns, false))
}

fn query_buck2_test_owner(
    path: &cargo_metadata::camino::Utf8Path,
    root: &cargo_metadata::camino::Utf8Path,
) -> Result<String> {
    query_buck2_test_owner_std(path.as_std_path(), root)
}

fn query_buck2_test_owner_std(
    path: &std::path::Path,
    root: &cargo_metadata::camino::Utf8Path,
) -> Result<String> {
    let relative = path.strip_prefix(root.as_std_path()).unwrap_or(path);
    let rel_str = relative.to_str().ok_or_else(|| anyhow!("Invalid path"))?;

    let query_expr = format!("kind(test, rdeps(//..., owner('{}'), 1))", rel_str);

    let output = Buck2Command::new()
        .arg("uquery")
        .arg(&query_expr)
        .output()
        .context("Failed to run buck2 uquery")?;

    if !output.status.success() {
        return Err(anyhow!("buck2 uquery failed"));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let owner = stdout
        .lines()
        .next()
        .ok_or_else(|| anyhow!("No test owner found"))?;
    Ok(owner.trim().to_string())
}

fn find_file_recursive(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current_dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let dirname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if dirname != "target" && dirname != ".git" && dirname != "buck-out" {
                        stack.push(path);
                    }
                } else if path.file_stem().is_some_and(|s| s == name)
                    && path.extension().is_some_and(|e| e == "rs")
                {
                    return Some(path);
                }
            }
        }
    }
    None
}
