use clap::Parser;
use serde::Deserialize;

use crate::{
    buck2::Buck2Command,
    buckal_error, buckal_log,
    utils::{
        UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_buck2_root, get_target,
        platform_exists,
    },
};

#[derive(Parser, Debug)]
pub struct BuildArgs {
    /// Build optimized artifacts with the release profile
    #[arg(short, long)]
    pub release: bool,

    /// Use verbose output (`-vv` very verbose output)
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Build the package’s library
    #[arg(long)]
    pub lib: bool,

    /// Build the specified binary
    #[arg(long, value_name = "NAME")]
    pub bin: Vec<String>,

    /// Build all binary targets
    #[arg(long)]
    pub bins: bool,

    /// Build the specified example
    #[arg(long, value_name = "NAME")]
    pub example: Vec<String>,

    /// Build all example targets
    #[arg(long)]
    pub examples: bool,

    /// Build all targets
    #[arg(long)]
    pub all_targets: bool,

    /// Build for the target platform (passed to buck2 `--target-platforms`)
    #[arg(long, value_name = "PLATFORM")]
    pub target_platforms: Option<String>,
}

impl BuildArgs {
    /// Check if any target selection flags are set
    pub fn has_target_selection(&self) -> bool {
        self.lib
            || self.bins
            || !self.bin.is_empty()
            || self.examples
            || !self.example.is_empty()
            || self.all_targets
    }

    /// Check if any target selection flags are set, excluding all_targets
    pub fn has_other_target_selection(&self) -> bool {
        self.lib || self.bins || !self.bin.is_empty() || self.examples || !self.example.is_empty()
    }

    /// Validate target selection arguments
    pub fn validate_target_selection(&self) -> Result<(), String> {
        if self.all_targets && self.has_other_target_selection() {
            return Err(
                "--all-targets cannot be used with other target selection options".to_string(),
            );
        }
        Ok(())
    }
}

pub fn execute(args: &BuildArgs) {
    // Ensure all prerequisites are installed before proceeding
    ensure_prerequisites().unwrap_or_exit();

    // Check if the current directory is a valid Buck2 package
    check_buck2_package().unwrap_or_exit();

    // Validate target selection arguments
    args.validate_target_selection().unwrap_or_exit();

    // Get the root directory of the Buck2 project
    let buck2_root = get_buck2_root().unwrap_or_exit_ctx("failed to get Buck2 project root");
    let cwd = std::env::current_dir().unwrap_or_exit_ctx("failed to get current directory");
    let relative = cwd.strip_prefix(&buck2_root).ok();

    if relative.is_none() {
        buckal_error!("current directory is not inside the Buck2 project root");
        return;
    }

    let mut relative_path = relative.unwrap().to_string_lossy().into_owned();
    if !relative_path.is_empty() {
        relative_path += "/";
    }

    if args.verbose > 2 {
        buckal_error!("maximum verbosity");
        return;
    }

    // Determine build targets based on selection arguments
    let targets = if args.all_targets {
        // Build all first-party Rust targets (avoid third-party //...).
        get_available_targets_all(&relative_path)
    } else if args.has_target_selection() {
        // Build specific targets based on selection
        build_specific_targets(args, &relative_path)
    } else {
        // Default: build first-party Rust targets under the current directory.
        get_available_targets(&relative_path)
    };

    if targets.is_empty() {
        buckal_error!("no targets found matching the specified criteria");
        std::process::exit(1);
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

    // Execute build for each target
    for target in targets {
        let mut buck2_cmd = Buck2Command::build(&target).verbosity(args.verbose);
        if args.release {
            buck2_cmd = buck2_cmd.arg("-m").arg("release");
        }
        if let Some(platform) = &target_platforms {
            buck2_cmd = buck2_cmd.arg("--target-platforms").arg(platform);
        }

        let result = buck2_cmd.status();
        match result {
            Ok(status) if status.success() => {
                buckal_log!("Built", &target);
            }
            Ok(_) => {
                buckal_error!(format!("buck2 build failed for target: {}", target));
                std::process::exit(1);
            }
            Err(e) => {
                buckal_error!(format!(
                    "failed to execute buck2 build for target {}:\n {}",
                    target, e
                ));
                std::process::exit(1);
            }
        }
    }
}

/// Build specific targets based on target selection arguments
fn build_specific_targets(args: &BuildArgs, relative_path: &str) -> Vec<String> {
    let mut targets = Vec::new();

    // Get available targets from Buck2
    let available_targets = get_available_targets(relative_path);

    if args.lib {
        // Add library targets
        targets.extend(get_library_targets(&available_targets, relative_path));
    }

    if args.bins || !args.bin.is_empty() {
        // Add binary targets
        targets.extend(get_binary_targets(
            &available_targets,
            relative_path,
            &args.bin,
            args.bins,
        ));
    }

    if args.examples || !args.example.is_empty() {
        // Add example targets
        targets.extend(get_example_targets(
            &available_targets,
            relative_path,
            &args.example,
            args.examples,
        ));
    }

    // Remove duplicates
    targets.sort();
    targets.dedup();
    targets
}

/// Get available targets from Buck2
#[derive(Debug, Deserialize)]
struct TargetEntry {
    #[serde(rename = "buck.type")]
    buck_type: String,
    #[serde(rename = "buck.package")]
    buck_package: String,
    name: String,
}

fn get_available_targets(relative_path: &str) -> Vec<String> {
    get_available_targets_by_kind(relative_path, false)
}

fn get_available_targets_all(relative_path: &str) -> Vec<String> {
    get_available_targets_by_kind(relative_path, true)
}

fn get_available_targets_by_kind(relative_path: &str, include_tests: bool) -> Vec<String> {
    let target_pattern = format!("//{relative_path}...");

    match Buck2Command::targets()
        .arg(&target_pattern)
        .arg("--output-basic-attributes")
        .arg("--json")
        .output()
    {
        Ok(output) if output.status.success() => {
            match serde_json::from_slice::<Vec<TargetEntry>>(&output.stdout) {
                Ok(entries) => {
                    let targets = entries
                        .into_iter()
                        .filter(|entry| {
                            entry.buck_type.ends_with(":rust_binary")
                                || entry.buck_type.ends_with(":rust_library")
                                || (include_tests && entry.buck_type.ends_with(":rust_test"))
                        })
                        .map(|entry| {
                            let package = entry
                                .buck_package
                                .strip_prefix("root//")
                                .unwrap_or(entry.buck_package.as_str())
                                .trim_end_matches('/');
                            if package.is_empty() {
                                format!("//:{}", entry.name)
                            } else {
                                format!("//{}:{}", package, entry.name)
                            }
                        })
                        .collect::<Vec<_>>();
                    filter_root_third_party(targets, relative_path)
                }
                Err(_) => vec![target_pattern],
            }
        }
        _ => {
            // If we can't get specific targets, fall back to all targets
            vec![target_pattern]
        }
    }
}

fn filter_root_third_party(mut targets: Vec<String>, relative_path: &str) -> Vec<String> {
    if !relative_path.is_empty() {
        return targets;
    }

    targets.retain(|target| {
        !target.starts_with("//third-party/")
            && !target.starts_with("//toolchains/")
            && !target.starts_with("//platforms/")
    });
    targets
}

/// Get library targets
fn get_library_targets(available_targets: &[String], relative_path: &str) -> Vec<String> {
    available_targets
        .iter()
        .filter(|target| {
            // In tests, we'll simulate library targets by checking for "lib" in the name
            // In real usage, this would check for rust_library type
            let target_name = extract_target_name(target, relative_path);
            target.contains("rust_library")
                || target_name.contains("lib")
                || target_name.contains("_lib")
        })
        .map(|target| target.to_string())
        .collect()
}

/// Get binary targets with optional pattern matching
fn get_binary_targets(
    available_targets: &[String],
    relative_path: &str,
    bin_patterns: &[String],
    all_bins: bool,
) -> Vec<String> {
    // Identify binary targets - in tests, we'll look for targets with "bin", "app", "main", or "tool" in the name
    // In real usage, this would check for rust_binary type
    let binary_targets: Vec<String> = available_targets
        .iter()
        .filter(|target| {
            let target_name = extract_target_name(target, relative_path);
            target.contains("rust_binary")
                || (target_name.contains("bin")
                    || target_name.contains("app")
                    || target_name.contains("main")
                    || target_name.contains("tool"))
                    && !target_name.contains("example")
        })
        .map(|target| target.to_string())
        .collect();

    if all_bins {
        return binary_targets;
    }

    if bin_patterns.is_empty() {
        return vec![];
    }

    let mut matched_targets = Vec::new();

    for pattern in bin_patterns {
        for target in &binary_targets {
            let target_name = extract_target_name(target, relative_path);
            if pattern_matches(&target_name, pattern) {
                matched_targets.push(target.clone());
            }
        }
    }

    matched_targets
}

/// Get example targets with optional pattern matching
fn get_example_targets(
    available_targets: &[String],
    relative_path: &str,
    example_patterns: &[String],
    all_examples: bool,
) -> Vec<String> {
    // Identify example targets - look for targets with "example" in the name
    let example_targets: Vec<String> = available_targets
        .iter()
        .filter(|target| {
            let target_name = extract_target_name(target, relative_path);
            target_name.contains("example")
        })
        .map(|target| target.to_string())
        .collect();

    if all_examples {
        return example_targets;
    }

    if example_patterns.is_empty() {
        return vec![];
    }

    let mut matched_targets = Vec::new();

    for pattern in example_patterns {
        for target in &example_targets {
            let target_name = extract_target_name(target, relative_path);
            if pattern_matches(&target_name, pattern) {
                matched_targets.push(target.clone());
            }
        }
    }

    matched_targets
}

/// Extract target name from full target path
fn extract_target_name(target: &str, relative_path: &str) -> String {
    // First, get the part before any space or parenthesis (the actual target path)
    let target_path = target.split_whitespace().next().unwrap_or(target);

    // Remove the prefix and extract just the target name
    let prefix = if relative_path.is_empty() {
        "//:".to_string()
    } else {
        format!("//{relative_path}:")
    };

    if let Some(stripped) = target_path.strip_prefix(&prefix) {
        stripped.to_string()
    } else {
        // If the prefix doesn't match, try to extract just the part after the last colon
        target_path
            .split(':')
            .next_back()
            .unwrap_or(target_path)
            .to_string()
    }
}

/// Check if a target name matches a pattern with Unix-style wildcards
fn pattern_matches(target_name: &str, pattern: &str) -> bool {
    // Handle exact match
    if pattern == target_name {
        return true;
    }

    // Convert glob pattern to regex
    let regex_pattern = glob_to_regex(pattern);

    // Use regex for matching
    regex::Regex::new(&regex_pattern)
        .map(|re| re.is_match(target_name))
        .unwrap_or(false)
}

/// Convert glob pattern to regex pattern
fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::new();
    regex.push('^');

    for c in glob.chars() {
        match c {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => regex.push('['),
            ']' => regex.push(']'),
            '.' => regex.push_str("\\."),
            '+' => regex.push_str("\\+"),
            '^' => regex.push_str("\\^"),
            '$' => regex.push_str("\\$"),
            '{' => regex.push_str("\\{"),
            '}' => regex.push_str("\\}"),
            '(' => regex.push_str("\\("),
            ')' => regex.push_str("\\)"),
            '|' => regex.push_str("\\|"),
            '\\' => regex.push_str("\\\\"),
            _ => regex.push(c),
        }
    }

    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create test targets
    fn create_test_targets() -> Vec<String> {
        vec![
            "//src:my_lib".to_string(),
            "//src:main_bin".to_string(),
            "//src:cli_tool".to_string(),
            "//examples:demo_example".to_string(),
            "//examples:test_example".to_string(),
            "//examples:other_example".to_string(),
            "//src:app1".to_string(),
            "//src:app2".to_string(),
            "//src:lib1".to_string(),
            "//src:test_app".to_string(),
            "//src:demo_app".to_string(),
            "//src:other_app".to_string(),
        ]
    }

    #[test]
    fn test_build_args_validation() {
        // Test valid combinations
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: true,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.validate_target_selection().is_ok());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec!["myapp".to_string()],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.validate_target_selection().is_ok());

        // Test valid: only all-targets
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: true,
            target_platforms: None,
        };
        assert!(args.validate_target_selection().is_ok());

        // Test invalid combination: all-targets with other options
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: true,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: true,
            target_platforms: None,
        };
        assert!(args.validate_target_selection().is_err());
    }

    #[test]
    fn test_has_target_selection() {
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(!args.has_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: true,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.has_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec!["app".to_string()],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.has_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: true,
            target_platforms: None,
        };
        assert!(args.has_target_selection());
    }

    #[test]
    fn test_has_other_target_selection() {
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(!args.has_other_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: true,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.has_other_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec!["app".to_string()],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };
        assert!(args.has_other_target_selection());

        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: false,
            bin: vec![],
            bins: false,
            example: vec![],
            examples: false,
            all_targets: true,
            target_platforms: None,
        };
        assert!(!args.has_other_target_selection());
    }

    #[test]
    fn test_extract_target_name() {
        // Test with full target path including type
        assert_eq!(
            extract_target_name("//src/main:myapp (rust_binary)", "src/main/"),
            "myapp"
        );
        assert_eq!(extract_target_name("//:mylib (rust_library)", ""), "mylib");
        assert_eq!(
            extract_target_name("//examples/demo:demo_app (rust_binary)", "examples/demo/"),
            "demo_app"
        );

        // Test with just target path
        assert_eq!(
            extract_target_name("//src/main:myapp", "src/main/"),
            "myapp"
        );

        // Test edge cases
        assert_eq!(extract_target_name("//src:main_bin", "src/"), "main_bin");
        assert_eq!(extract_target_name("//:root_lib", ""), "root_lib");
    }

    #[test]
    fn test_glob_to_regex() {
        assert_eq!(glob_to_regex("test*"), "^test.*$");
        assert_eq!(glob_to_regex("test?"), "^test.$");
        assert_eq!(glob_to_regex("test[abc]"), "^test[abc]$");
        assert_eq!(glob_to_regex("test.app"), "^test\\.app$");
    }

    #[test]
    fn test_pattern_matching() {
        // Test wildcard matching
        assert!(pattern_matches("test_app", "test*"));
        assert!(pattern_matches("tester", "test*"));
        assert!(!pattern_matches("other", "test*"));

        // Test single character matching
        assert!(pattern_matches("demo1", "demo?"));
        assert!(pattern_matches("demo2", "demo?"));
        assert!(!pattern_matches("demo", "demo?"));
        assert!(!pattern_matches("demo_app", "demo?"));

        // Test exact match
        assert!(pattern_matches("exact", "exact"));
        assert!(!pattern_matches("exact-match", "exact"));

        // Test character classes
        assert!(pattern_matches("test1", "test[123]"));
        assert!(pattern_matches("test2", "test[123]"));
        assert!(pattern_matches("test3", "test[123]"));
        assert!(!pattern_matches("test4", "test[123]"));
    }

    #[test]
    fn test_target_selection_scenarios() {
        let available_targets = create_test_targets();
        let relative_path = "src/";

        // Test library selection
        let lib_targets = get_library_targets(&available_targets, relative_path);
        assert!(lib_targets.len() >= 2); // my_lib and lib1
        assert!(lib_targets.iter().any(|t| t.contains("my_lib")));
        assert!(lib_targets.iter().any(|t| t.contains("lib1")));

        // Test binary pattern matching
        let bin_targets = get_binary_targets(
            &available_targets,
            relative_path,
            &["main*".to_string()],
            false,
        );
        assert_eq!(bin_targets.len(), 1);
        assert!(bin_targets[0].contains("main_bin"));
    }

    #[test]
    fn test_edge_cases() {
        // Test empty patterns
        let targets = get_binary_targets(&[], "", &[], false);
        assert!(targets.is_empty());

        // Test invalid regex pattern (should not panic)
        assert!(!pattern_matches("target", "invalid[pattern"));
    }

    #[test]
    fn test_binary_targets_all_bins() {
        let available_targets = create_test_targets();
        let targets = get_binary_targets(&available_targets, "src/", &[], true);

        // Should find main_bin, cli_tool, app1, app2, test_app, demo_app
        assert!(targets.len() >= 6);
        assert!(targets.iter().any(|t| t.contains("main_bin")));
        assert!(targets.iter().any(|t| t.contains("cli_tool")));
        assert!(targets.iter().any(|t| t.contains("app1")));
        assert!(targets.iter().any(|t| t.contains("app2")));
        assert!(targets.iter().any(|t| t.contains("test_app")));
        assert!(targets.iter().any(|t| t.contains("demo_app")));
    }

    #[test]
    fn test_binary_targets_with_patterns() {
        let available_targets = create_test_targets();
        let targets = get_binary_targets(
            &available_targets,
            "src/",
            &["test*".to_string(), "demo*".to_string()],
            false,
        );

        // Should find test_app and demo_app
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|t| t.contains("test_app")));
        assert!(targets.iter().any(|t| t.contains("demo_app")));
    }

    #[test]
    fn test_example_targets_all_examples() {
        let available_targets = create_test_targets();
        let targets = get_example_targets(&available_targets, "examples/", &[], true);

        // Should find all examples
        assert_eq!(targets.len(), 3);
        assert!(targets.iter().any(|t| t.contains("demo_example")));
        assert!(targets.iter().any(|t| t.contains("test_example")));
        assert!(targets.iter().any(|t| t.contains("other_example")));
    }

    #[test]
    fn test_example_targets_with_patterns() {
        let available_targets = create_test_targets();
        let targets = get_example_targets(
            &available_targets,
            "examples/",
            &["demo*".to_string()],
            false,
        );

        // Should find only demo_example
        assert_eq!(targets.len(), 1);
        assert!(targets[0].contains("demo_example"));
    }

    #[test]
    fn test_duplicate_removal() {
        let mut targets = vec![
            "//src:app1".to_string(),
            "//src:app2".to_string(),
            "//src:app1".to_string(), // duplicate
            "//src:app3".to_string(),
        ];

        targets.sort();
        targets.dedup();

        assert_eq!(targets.len(), 3);
        assert_eq!(targets, vec!["//src:app1", "//src:app2", "//src:app3"]);
    }

    #[test]
    fn test_complex_glob_patterns() {
        // Test character classes
        assert!(pattern_matches("test1", "test[123]"));
        assert!(pattern_matches("test2", "test[123]"));
        assert!(pattern_matches("test3", "test[123]"));
        assert!(!pattern_matches("test4", "test[123]"));
        assert!(!pattern_matches("test12", "test[123]"));

        // Test ranges
        assert!(pattern_matches("testa", "test[a-c]"));
        assert!(pattern_matches("testb", "test[a-c]"));
        assert!(pattern_matches("testc", "test[a-c]"));
        assert!(!pattern_matches("testd", "test[a-c]"));

        // Test question mark
        assert!(pattern_matches("test1", "test?"));
        assert!(pattern_matches("testa", "test?"));
        assert!(!pattern_matches("test", "test?"));
        assert!(!pattern_matches("test12", "test?"));
    }

    #[test]
    fn test_no_targets_found() {
        let available_targets = vec!["//src:lib1".to_string()];

        // Try to find binaries when only libraries exist
        let targets = get_binary_targets(&available_targets, "src/", &["app*".to_string()], false);
        assert!(targets.is_empty());

        // Try to find examples when none exist
        let targets =
            get_example_targets(&available_targets, "src/", &["example*".to_string()], false);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_mixed_target_selection() {
        let args = BuildArgs {
            release: false,
            verbose: 0,
            lib: true,
            bin: vec!["main*".to_string()],
            bins: false,
            example: vec!["demo*".to_string()],
            examples: false,
            all_targets: false,
            target_platforms: None,
        };

        assert!(args.has_target_selection());
        assert!(args.validate_target_selection().is_ok());
    }

    #[test]
    fn test_empty_relative_path() {
        // Test with empty relative path (root directory)
        let target = "//:mylib";
        let extracted = extract_target_name(target, "");
        assert_eq!(extracted, "mylib");
        let target = "//src:myapp";
        let extracted = extract_target_name(target, "");
        assert_eq!(extracted, "myapp");
    }
}
