use crate::{
    buck2::Buck2Command,
    utils::{
        UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_buck2_root, get_target,
        platform_exists,
    },
};
use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use std::collections::HashSet;
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

    /// Test all targets that have `test = true` set
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

    /// Build for the target platform (passed to buck2 --target-platforms)
    #[arg(long, value_name = "PLATFORM")]
    pub target_platforms: Option<String>,

    /// Build artifacts in release mode, with optimizations
    #[arg(short, long)]
    pub release: bool,

    /// If specified, only run tests containing this string in their names
    #[arg(value_name = "TESTNAME")]
    pub test_name: Option<String>,

    /// Arguments for the test executor
    #[arg(last = true)]
    pub args: Vec<String>,
}

pub fn execute(args: &TestArgs) {
    ensure_prerequisites().unwrap_or_exit();
    check_buck2_package().unwrap_or_exit();

    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")
        .unwrap_or_exit();

    let buck2_root = get_buck2_root().unwrap_or_exit();

    let (targets, _is_specific_target) = resolve_targets(args, &metadata, &buck2_root)
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

    cmd = cmd.arg("--exclude").arg("//third-party/...");
    cmd = cmd.arg("--exclude").arg("root//third-party/...");

    if let Some(jobs) = args.jobs {
        cmd = cmd.arg("-j").arg(jobs.to_string());
    }

    let target_platforms = if let Some(platform) = &args.target_platforms {
        Some(platform.clone())
    } else {
        let platform = format!("//platforms:{}", get_target());
        if platform_exists(&platform) {
            Some(platform)
        } else {
            None
        }
    };
    if let Some(platform) = &target_platforms {
        cmd = cmd.arg("--target-platforms").arg(platform);
    }

    if args.release {
        cmd = cmd.arg("-m").arg("release");
    }

    if args.no_fail_fast {
        cmd = cmd.arg("--keep-going");
    }

    if !args.no_run {
        let mut raw_args = Vec::new();

        if let Some(name) = &args.test_name {
            raw_args.push(name.clone());
        }

        raw_args.extend_from_slice(&args.args);

        if !raw_args.is_empty() {
            cmd = cmd.arg("--");
            for arg in raw_args {
                cmd = cmd.arg("--test-arg").arg(arg);
            }
        }
    }

    let status = cmd.status().unwrap_or_exit_ctx("failed to execute buck2");

    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn resolve_targets(
    args: &TestArgs,
    metadata: &cargo_metadata::Metadata,
    buck2_root: &cargo_metadata::camino::Utf8Path,
) -> Result<(Vec<String>, bool)> {
    let mut patterns = Vec::new();
    let mut specific_found = false;

    // Build a set of workspace members to filter out third-party dependencies efficiently
    let workspace_members: HashSet<_> = metadata.workspace_members.iter().collect();

    if let Some(name) = &args.test_name {
        if is_glob_pattern(name) {
            for pkg in &metadata.packages {
                // Critical Filter: Only look at workspace members
                if !workspace_members.contains(&pkg.id) {
                    continue;
                }

                for target in &pkg.targets {
                    if target.kind.iter().any(|k| k.to_string() == "test")
                        && glob_match(name, &target.name)
                        && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                    {
                        patterns.push(owner);
                        specific_found = true;
                    }
                }
            }
            if !specific_found {
                return Err(anyhow!("error: no test target matched glob `{}`", name));
            }
        } else {
            let name_norm = name.replace('-', "_");
            let mut found_in_metadata = false;

            for pkg in &metadata.packages {
                // Critical Filter: Only look at workspace members
                if !workspace_members.contains(&pkg.id) {
                    continue;
                }

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

            if patterns.is_empty() {
                return Err(anyhow!(
                    "error: no test target or source file found matching `{}`",
                    name
                ));
            }
        }
    }

    if !args.test.is_empty() {
        for t_name in &args.test {
            let mut found_local = false;
            let is_glob = is_glob_pattern(t_name);
            let t_name_norm = if !is_glob {
                t_name.replace('-', "_")
            } else {
                t_name.clone()
            };

            for pkg in &metadata.packages {
                // Critical Filter: Only look at workspace members
                if !workspace_members.contains(&pkg.id) {
                    continue;
                }

                for target in &pkg.targets {
                    let file_stem = target.src_path.file_stem().unwrap_or("");
                    let file_stem_norm = file_stem.replace('-', "_");
                    let target_name_norm = target.name.replace('-', "_");

                    let is_match = if is_glob {
                        glob_match(t_name, &target.name)
                    } else {
                        target_name_norm == t_name_norm || file_stem_norm == t_name_norm
                    };

                    if is_match
                        && target.kind.iter().any(|k| k.to_string() == "test")
                        && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                    {
                        patterns.push(owner);
                        found_local = true;
                        specific_found = true;
                    }
                }
            }

            if !found_local && !is_glob {
                let root_path = buck2_root.as_std_path();
                if let Some(file_path) = find_file_recursive(root_path, t_name)
                    && let Ok(owner) = query_buck2_test_owner_std(&file_path, buck2_root)
                {
                    patterns.push(owner);
                    specific_found = true;
                }
            }

            if !found_local && !specific_found {
                if is_glob {
                    return Err(anyhow!("error: no test target matched glob `{}`", t_name));
                } else {
                    return Err(anyhow!("error: no test target named `{}`", t_name));
                }
            }
        }
    }

    let has_kind_selection =
        args.lib || args.bins || !args.bin.is_empty() || args.examples || !args.example.is_empty();

    if has_kind_selection {
        for pkg in &metadata.packages {
            // Critical Filter: Either explicit package match OR workspace member
            let is_workspace_member = workspace_members.contains(&pkg.id);

            if !args.package.is_empty() {
                if !args.package.contains(&pkg.name) {
                    continue;
                }
            } else if !is_workspace_member {
                // If no package arg is specified, skip all non-workspace members (e.g. dependencies)
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

                if target.kind.iter().any(|k| k.to_string() == "bin") {
                    if args.bins {
                        matches_kind = true;
                    } else if !args.bin.is_empty() {
                        for bin_arg in &args.bin {
                            if is_glob_pattern(bin_arg) {
                                if glob_match(bin_arg, &target.name) {
                                    matches_kind = true;
                                    specific_found = true;
                                }
                            } else if *bin_arg == target.name {
                                matches_kind = true;
                                specific_found = true;
                            }
                        }
                    }
                }

                if target.kind.iter().any(|k| k.to_string() == "example") {
                    if args.examples {
                        matches_kind = true;
                    } else if !args.example.is_empty() {
                        for ex_arg in &args.example {
                            if is_glob_pattern(ex_arg) {
                                if glob_match(ex_arg, &target.name) {
                                    matches_kind = true;
                                    specific_found = true;
                                }
                            } else if *ex_arg == target.name {
                                matches_kind = true;
                                specific_found = true;
                            }
                        }
                    }
                }

                if matches_kind
                    && let Ok(owner) = query_buck2_test_owner(&target.src_path, buck2_root)
                {
                    patterns.push(owner);
                    specific_found = true;
                }
            }
        }

        if !args.bin.is_empty() && !specific_found && !args.bins {
            return Err(anyhow!("error: no bin target matched"));
        }
        if !args.example.is_empty() && !specific_found && !args.examples {
            return Err(anyhow!("error: no example target matched"));
        }
    }

    if specific_found && !patterns.is_empty() {
        return Ok((patterns, true));
    }

    if patterns.is_empty() {
        let mut search_roots = Vec::new();

        if args.workspace {
            search_roots.push("//...".to_string());
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
                    search_roots.push(format_buck2_pattern(relative.as_str()));
                }
            }
        } else {
            let current_dir = std::env::current_dir()?;
            let relative = current_dir
                .strip_prefix(buck2_root.as_std_path())
                .map_err(|_| anyhow!("Current directory is outside project root"))?;
            search_roots.push(format_buck2_pattern(relative.to_str().unwrap()));
        }

        if !search_roots.is_empty() {
            let root_expr = search_roots.join(" + ");
            let query_expr = format!("kind(test, {})", root_expr);
            let output = Buck2Command::new()
                .arg("uquery")
                .arg(&query_expr)
                .output()
                .context("Failed to run buck2 uquery for wildcard resolution")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!(
                    "buck2 uquery failed to resolve wildcard targets: {}",
                    stderr
                ));
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    patterns.push(t.to_string());
                }
            }
        }
    }

    patterns.retain(|p| !p.contains("third-party"));

    Ok((patterns, false))
}

fn is_glob_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut last_star = None;
    let mut last_t = 0;

    while t_idx < t_chars.len() {
        if p_idx < p_chars.len() && (p_chars[p_idx] == '?' || p_chars[p_idx] == t_chars[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_chars.len() && p_chars[p_idx] == '*' {
            last_star = Some(p_idx);
            p_idx += 1;
            last_t = t_idx;
        } else if let Some(star_idx) = last_star {
            p_idx = star_idx + 1;
            last_t += 1;
            t_idx = last_t;
        } else {
            return false;
        }
    }

    while p_idx < p_chars.len() && p_chars[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_chars.len()
}

fn format_buck2_pattern(rel_path: &str) -> String {
    let trimmed = rel_path.trim_start_matches('/');
    if trimmed.is_empty() {
        "//...".to_string()
    } else {
        format!("//{}/...", trimmed)
    }
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "buck2 uquery failed for query `{}`: {}",
            query_expr,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;

    let path_str = rel_str.to_string();

    let best_owner = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .max_by_key(|target| {
            let target_parts: Vec<&str> = target.split(&['/', ':', '_'][..]).collect();
            let file_parts: Vec<&str> = path_str.split(&['/', ':', '_', '.'][..]).collect();

            let mut score = 0;
            for tp in &target_parts {
                if !tp.is_empty() && file_parts.contains(tp) {
                    score += 1;
                }
            }
            (score, target.len())
        })
        .ok_or_else(|| anyhow!("No Buck2 test rule found that owns file '{}'", rel_str))?;

    Ok(best_owner.trim().to_string())
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
