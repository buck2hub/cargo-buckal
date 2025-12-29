use crate::{
    buck2::Buck2Command,
    utils::{UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_buck2_root},
};
use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use std::process::exit;

#[derive(Parser, Debug)]
pub struct TestArgs {
    /// Package to run tests for
    #[arg(short, long, value_name = "SPEC")]
    pub package: Vec<String>,

    /// Test all packages in the workspace
    #[arg(long)]
    pub workspace: bool,

    /// Exclude packages from the test
    #[arg(long, value_name = "SPEC")]
    pub exclude: Vec<String>,

    /// Test all targets
    #[arg(long)]
    pub all_targets: bool,

    /// Test only this package's library
    #[arg(long)]
    pub lib: bool,

    /// Test only the specified binary
    #[arg(long, value_name = "NAME")]
    pub bin: Vec<String>,

    /// Test all binaries
    #[arg(long)]
    pub bins: bool,

    /// Test only the specified example
    #[arg(long, value_name = "NAME")]
    pub example: Vec<String>,

    /// Test all examples
    #[arg(long)]
    pub examples: bool,

    /// Test only the specified test target
    #[arg(long, value_name = "NAME")]
    pub test: Vec<String>,

    /// Test all tests
    #[arg(long)]
    pub tests: bool,

    /// Compile, but don't run tests
    #[arg(long)]
    pub no_run: bool,

    /// Run all tests regardless of failure
    #[arg(long)]
    pub no_fail_fast: bool,

    /// Number of parallel jobs, defaults to # of CPUs
    #[arg(short, long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Build for the target triple
    #[arg(long, value_name = "TRIPLE")]
    pub target: Option<String>,

    /// Build artifacts in release mode, with optimizations
    #[arg(short, long)]
    pub release: bool,

    /// Build artifacts with the specified profile
    #[arg(long, value_name = "PROFILE-NAME")]
    pub profile: Option<String>,

    /// The name of the test to run (positional argument)
    #[arg(value_name = "TESTNAME")]
    pub test_name: Option<String>,

    /// Arguments for the test binary
    #[arg(last = true)]
    pub args: Vec<String>,
}

pub fn execute(args: &TestArgs) {
    // Perform essential environment and package checks
    ensure_prerequisites().unwrap_or_exit();
    check_buck2_package().unwrap_or_exit();

    // Fetch cargo metadata to analyze the project structure
    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")
        .unwrap_or_exit();

    let buck2_root = get_buck2_root().unwrap_or_exit();

    // Core logic: resolve the requested cargo targets to Buck2 target labels
    let (targets, is_specific_target) = resolve_targets(args, &metadata, &buck2_root)
        .unwrap_or_exit_ctx("failed to resolve targets");

    if targets.is_empty() {
        eprintln!("No targets found to test.");
        return;
    }

    // Initialize the command as 'build' for --no-run, otherwise 'test'
    let mut cmd = if args.no_run {
        Buck2Command::new().arg("build")
    } else {
        Buck2Command::new().arg("test")
    };

    // Append resolved Buck2 targets to the command arguments
    for target in &targets {
        cmd = cmd.arg(target);
    }

    // Apply package exclusions using the Buck2 --exclude flag
    for excluded_pkg in &args.exclude {
        if let Some(pkg) = metadata
            .packages
            .iter()
            .find(|p| p.name.as_str() == excluded_pkg)
        {
            let pkg_path = pkg
                .manifest_path
                .parent()
                .ok_or_else(|| anyhow!("Package {} manifest has no parent directory", excluded_pkg))
                .unwrap_or_exit();

            let relative = pkg_path.strip_prefix(&buck2_root).unwrap_or_exit();
            let pattern = format_buck2_pattern(relative.as_str());
            cmd = cmd.arg("--exclude").arg(pattern);
        }
    }

    // Map common cargo CLI flags to their Buck2 equivalents
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

    // Handle pass-through arguments directed at the underlying test runner
    if !args.no_run {
        let mut passthrough_args = Vec::new();
        // If a specific target wasn't already resolved, pass the name as a filter
        if !is_specific_target && let Some(name) = &args.test_name {
            passthrough_args.push(name.clone());
        }
        passthrough_args.extend_from_slice(&args.args);

        if !passthrough_args.is_empty() {
            cmd = cmd.arg("--");
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

/// Resolves various cargo test flags into a list of Buck2 target labels.
fn resolve_targets(
    args: &TestArgs,
    metadata: &cargo_metadata::Metadata,
    buck2_root: &cargo_metadata::camino::Utf8Path,
) -> Result<(Vec<String>, bool)> {
    let mut patterns = Vec::new();
    let mut specific_found = false;

    // Strategy 1: Positional argument matching (e.g., `cargo buckal test name`)
    if let Some(name) = &args.test_name {
        let name_norm = name.replace('-', "_");
        let mut found_in_metadata = false;

        for pkg in &metadata.packages {
            for target in &pkg.targets {
                let file_stem = target.src_path.file_stem().unwrap_or("");
                let target_name_norm = target.name.replace('-', "_");
                let file_stem_norm = file_stem.replace('-', "_");

                if (target_name_norm == name_norm || file_stem_norm == name_norm)
                    && target.kind.iter().any(|k| k.to_string() == "test")
                    && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                {
                    patterns.push(owner);
                    found_in_metadata = true;
                    specific_found = true;
                }
            }
        }

        if !found_in_metadata {
            let root_path = buck2_root.as_std_path();
            if let Some(file_path) = find_file_recursive(root_path, name)
                && let Ok(owner) = query_buck2_test_owner_std(&file_path, buck2_root)
            {
                patterns.push(owner);
                return Ok((patterns, true));
            }
        } else {
            return Ok((patterns, true));
        }
    }

    // Strategy 2: Explicit --test <NAME> arguments
    if !args.test.is_empty() {
        for t_name in &args.test {
            let mut found_local = false;
            for pkg in &metadata.packages {
                for target in &pkg.targets {
                    let file_stem = target.src_path.file_stem().unwrap_or("");
                    if (target.name == *t_name || file_stem == *t_name)
                        && target.kind.iter().any(|k| k.to_string() == "test")
                        && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                    {
                        patterns.push(owner);
                        found_local = true;
                        specific_found = true;
                    }
                }
            }
            if !found_local {
                let root_path = buck2_root.as_std_path();
                if let Some(file_path) = find_file_recursive(root_path, t_name)
                    && let Ok(owner) = query_buck2_test_owner_std(&file_path, buck2_root)
                {
                    patterns.push(owner);
                    specific_found = true;
                }
            }
        }
    }

    // Strategy 3: Selection by target kind (--lib, --bin, --example)
    let has_kind_selection =
        args.lib || args.bins || !args.bin.is_empty() || args.examples || !args.example.is_empty();

    if has_kind_selection {
        for pkg in &metadata.packages {
            if !args.package.is_empty() && !args.package.contains(&pkg.name) {
                continue;
            }

            for target in &pkg.targets {
                let mut matches_kind = false;

                if args.lib
                    && target.kind.iter().any(|k| {
                        let s = k.to_string();
                        s == "lib" || s == "rlib" || s == "proc-macro"
                    })
                {
                    matches_kind = true;
                }

                if target.kind.iter().any(|k| k.to_string() == "bin")
                    && (args.bins || args.bin.contains(&target.name))
                {
                    matches_kind = true;
                }

                if target.kind.iter().any(|k| k.to_string() == "example")
                    && (args.examples || args.example.contains(&target.name))
                {
                    matches_kind = true;
                }

                if matches_kind
                    && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                {
                    patterns.push(owner);
                    specific_found = true;
                }
            }
        }
    }

    if specific_found && !patterns.is_empty() {
        return Ok((patterns, true));
    }

    // Strategy 4: Fallback to general patterns (workspace, package, or directory)
    if args.workspace {
        patterns.push("//...".to_string());
    } else if !args.package.is_empty() {
        for pkg_name in &args.package {
            if let Some(pkg) = metadata
                .packages
                .iter()
                .find(|p| p.name.as_str() == pkg_name)
            {
                let pkg_path = pkg.manifest_path.parent().ok_or_else(|| {
                    anyhow!("Package {} manifest has no parent directory", pkg_name)
                })?;

                let relative = pkg_path
                    .strip_prefix(buck2_root)
                    .map_err(|_| anyhow!("Package {} outside root", pkg_name))?;

                patterns.push(format_buck2_pattern(relative.as_str()));
            }
        }
    } else {
        let current_dir = std::env::current_dir()?;
        let relative = current_dir
            .strip_prefix(buck2_root.as_std_path())
            .map_err(|_| anyhow!("Current directory is outside project root"))?;

        patterns.push(format_buck2_pattern(relative.to_str().unwrap()));
    }

    Ok((patterns, false))
}

/// Helper to convert a relative path into a recursive Buck2 target pattern.
fn format_buck2_pattern(rel_path: &str) -> String {
    let trimmed = rel_path.trim_start_matches('/');
    if trimmed.is_empty() {
        "//...".to_string()
    } else {
        format!("//{}/...", trimmed)
    }
}

/// Helper function to resolve the Buck2 owner using camino paths.
fn query_buck2_test_owner(
    path: &cargo_metadata::camino::Utf8Path,
    root: &cargo_metadata::camino::Utf8Path,
) -> Result<String> {
    query_buck2_test_owner_std(path.as_std_path(), root)
}

/// Executes 'buck2 uquery' to find the test rule owning a specific file.
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "buck2 uquery failed for query `{}`: {}",
            query_expr,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let owner = stdout
        .lines()
        .next()
        .ok_or_else(|| anyhow!("No Buck2 test rule found that owns file '{}'", rel_str))?;
    Ok(owner.trim().to_string())
}

/// Recursively searches for a .rs file matching the specified name.
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
