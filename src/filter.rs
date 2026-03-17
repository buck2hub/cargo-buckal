// NOTE: `dead_code` is allowed temporarily for stubs reserved for future development.
// This attribute should be removed once the implementation is complete.
#![allow(dead_code)]

use anyhow::bail;
use cargo_metadata::TargetKind;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::buck2::Buck2Command;

/// Indicates whether or not the library target gets included.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum LibRule {
    /// Include the library, fail if not present
    True,
    /// Include the library if present
    Default,
    /// Exclude the library
    False,
}

/// Indicates which targets will be selected to be built.
#[derive(Debug, Clone)]
pub enum FilterRule {
    /// All included.
    All,
    /// Just a subset of Cargo targets based on names given.
    Just(GlobSet),
}

/// Indicates which command is calling the filter,
/// which may be used to determine the default filtering behavior.
#[derive(Debug, Clone)]
pub enum FilterCaller {
    Build,
    Test,
}

/// Filter to apply to the root package to select which targets will be built.
/// (examples, bins, benches, tests, ...)
#[derive(Debug, Clone)]
pub enum TargetFilter {
    /// The default set of targets, determined by the caller.
    Default(FilterCaller),
    /// Only includes a subset of all targets.
    Only {
        /// Include all targets.
        all_targets: bool,
        lib: LibRule,
        bins: FilterRule,
        examples: FilterRule,
        tests: FilterRule,
        benches: FilterRule,
    },
}

impl FilterRule {
    pub fn new(targets: Vec<String>, all: bool) -> anyhow::Result<FilterRule> {
        if all {
            Ok(FilterRule::All)
        } else {
            let mut builder = GlobSetBuilder::new();
            for target in targets {
                builder.add(Glob::new(&target)?);
            }

            Ok(FilterRule::Just(builder.build()?))
        }
    }

    /// Creates a filter with no rule.
    ///
    /// In the current implementation, filter without a rule implies
    /// the default behaviour to filter targets.
    pub fn none() -> FilterRule {
        FilterRule::Just(GlobSetBuilder::new().build().unwrap())
    }

    /// Checks if a target definition matches this filter rule.
    fn matches(&self, target: &BuckTargetEntry) -> bool {
        match *self {
            FilterRule::All => true,
            FilterRule::Just(ref targets) => targets.is_match(target.name()),
        }
    }

    /// Check if a filter is specific.
    ///
    /// Only filters without rules are considered as not specific.
    fn is_specific(&self) -> bool {
        match *self {
            FilterRule::All => true,
            FilterRule::Just(ref targets) => !targets.is_empty(),
        }
    }
}

impl TargetFilter {
    /// Constructs a filter from raw command line arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn from_raw_arguments(
        lib_only: bool,
        bins: Vec<String>,
        all_bins: bool,
        tests: Vec<String>,
        all_tests: bool,
        examples: Vec<String>,
        all_examples: bool,
        benches: Vec<String>,
        all_benches: bool,
        all_targets: bool,
        caller: FilterCaller,
    ) -> anyhow::Result<TargetFilter> {
        if all_targets {
            return Ok(TargetFilter::new_all_targets());
        }
        let rule_lib = if lib_only {
            LibRule::True
        } else {
            LibRule::False
        };
        let rule_bins = FilterRule::new(bins, all_bins)?;
        let rule_tests = FilterRule::new(tests, all_tests)?;
        let rule_examples = FilterRule::new(examples, all_examples)?;
        let rule_benches = FilterRule::new(benches, all_benches)?;

        Ok(TargetFilter::new(
            rule_lib,
            rule_bins,
            rule_tests,
            rule_examples,
            rule_benches,
            caller,
        ))
    }

    /// Constructs a filter from underlying primitives.
    pub fn new(
        rule_lib: LibRule,
        rule_bins: FilterRule,
        rule_tests: FilterRule,
        rule_examples: FilterRule,
        rule_benches: FilterRule,
        caller: FilterCaller,
    ) -> TargetFilter {
        if rule_lib == LibRule::True
            || rule_bins.is_specific()
            || rule_tests.is_specific()
            || rule_examples.is_specific()
            || rule_benches.is_specific()
        {
            TargetFilter::Only {
                all_targets: false,
                lib: rule_lib,
                bins: rule_bins,
                examples: rule_examples,
                benches: rule_benches,
                tests: rule_tests,
            }
        } else {
            TargetFilter::Default(caller)
        }
    }

    /// Constructs a filter that includes all targets.
    pub fn new_all_targets() -> TargetFilter {
        TargetFilter::Only {
            all_targets: true,
            lib: LibRule::Default,
            bins: FilterRule::All,
            examples: FilterRule::All,
            benches: FilterRule::All,
            tests: FilterRule::All,
        }
    }

    /// Constructs a filter that includes all test targets.
    ///
    /// This is different from `TargetFilter::Default(TargetCaller::Test)` in that it doesn't include examples, which are not necessary for testing and may significantly increase the build time.
    pub fn all_test_targets() -> Self {
        Self::Only {
            all_targets: false,
            lib: LibRule::Default,
            bins: FilterRule::none(),
            examples: FilterRule::none(),
            tests: FilterRule::All,
            benches: FilterRule::none(),
        }
    }

    /// Constructs a filter that includes lib target only.
    pub fn lib_only() -> Self {
        Self::Only {
            all_targets: false,
            lib: LibRule::True,
            bins: FilterRule::none(),
            examples: FilterRule::none(),
            tests: FilterRule::none(),
            benches: FilterRule::none(),
        }
    }

    /// Constructs a filter that includes the given binary. No more. No less.
    pub fn single_bin(bin: String) -> anyhow::Result<Self> {
        Ok(Self::Only {
            all_targets: false,
            lib: LibRule::False,
            bins: FilterRule::new(vec![bin], false)?,
            examples: FilterRule::none(),
            tests: FilterRule::none(),
            benches: FilterRule::none(),
        })
    }

    /// Selects targets to run.
    pub fn target_run(&self, target: &BuckTargetEntry) -> bool {
        match *self {
            TargetFilter::Default(FilterCaller::Build) => {
                matches!(target.kind(), TargetKind::Bin | TargetKind::Lib)
            }
            TargetFilter::Default(FilterCaller::Test) => matches!(
                target.kind(),
                TargetKind::Lib | TargetKind::Bin | TargetKind::Example | TargetKind::Test
            ),
            TargetFilter::Only {
                ref lib,
                ref bins,
                ref examples,
                ref tests,
                ref benches,
                ..
            } => {
                let rule = match target.kind() {
                    TargetKind::Bin => bins,
                    TargetKind::Test => tests,
                    TargetKind::Bench => benches,
                    TargetKind::Example => examples,
                    TargetKind::Lib => {
                        return match *lib {
                            LibRule::True => true,
                            LibRule::Default => true,
                            LibRule::False => false,
                        };
                    }
                    TargetKind::CustomBuild => return false,
                    _ => return false,
                };
                rule.matches(target)
            }
        }
    }

    pub fn is_specific(&self) -> bool {
        match *self {
            TargetFilter::Default(_) => false,
            TargetFilter::Only { .. } => true,
        }
    }

    pub fn is_all_targets(&self) -> bool {
        matches!(
            *self,
            TargetFilter::Only {
                all_targets: true,
                ..
            }
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BuckTargetEntry {
    #[serde(rename = "buck.type")]
    buck_type: String,
    #[serde(rename = "buck.package")]
    buck_package: String,
    name: String,
}

impl BuckTargetEntry {
    /// Checks if this target is a Rust rule, excluding helper rules.
    pub fn is_rust_rule(&self) -> bool {
        self.buck_type == "prelude//rules.bzl:rust_library"
            || self.buck_type == "prelude//rules.bzl:rust_binary"
            || self.buck_type == "prelude//rules.bzl:rust_test"
    }

    /// Checks if this target is third-party.
    pub fn is_third_party(&self) -> bool {
        self.buck_package.starts_with("root//third-party/")
    }

    /// Returns the full target label in the format of `<cell>//<package>:<target>`.
    pub fn label(&self) -> String {
        format!("{}:{}", self.buck_package, self.name)
    }

    /// Returns the target name.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the target kind (e.g., bin, test, bench, example, lib).
    pub fn kind(&self) -> TargetKind {
        match self.buck_type.as_str() {
            "prelude//rules.bzl:rust_library" => TargetKind::Lib,
            "prelude//rules.bzl:rust_binary" => TargetKind::Bin,
            "prelude//rules.bzl:rust_test" => TargetKind::Test,
            _ => TargetKind::Unknown("Unsupported".into()),
        }
    }
}

/// Get available targets from Buck2 under the specified package, and filter out non-Rust rules and third-party rules.
pub fn get_available_targets(package: &str) -> anyhow::Result<Vec<BuckTargetEntry>> {
    let target_pattern = if package.is_empty() {
        "//...".into()
    } else {
        format!("//{package}/...")
    };

    match Buck2Command::targets()
        .arg(&target_pattern)
        .arg("--output-basic-attributes")
        .arg("--json")
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                let targets = serde_json::from_slice::<Vec<BuckTargetEntry>>(&output.stdout)?
                    .into_iter()
                    .filter(|entry| entry.is_rust_rule() && !entry.is_third_party())
                    .collect();
                Ok(targets)
            } else {
                bail!(
                    "failed to query Buck2 targets: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        Err(e) => {
            bail!("failed to execute Buck2 command: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_target(buck_type: &str, name: &str) -> BuckTargetEntry {
        BuckTargetEntry {
            buck_type: buck_type.to_string(),
            buck_package: "root//pkg".to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn new_returns_default_when_no_specific_rules() {
        let filter = TargetFilter::new(
            LibRule::False,
            FilterRule::none(),
            FilterRule::none(),
            FilterRule::none(),
            FilterRule::none(),
            FilterCaller::Build,
        );

        assert!(matches!(filter, TargetFilter::Default(_)));
        assert!(!filter.is_specific());
        assert!(!filter.is_all_targets());
    }

    #[test]
    fn new_all_targets_sets_expected_flags() {
        let filter = TargetFilter::new_all_targets();

        assert!(matches!(
            filter,
            TargetFilter::Only {
                all_targets: true,
                ..
            }
        ));
        assert!(filter.is_specific());
        assert!(filter.is_all_targets());
    }

    #[test]
    fn all_test_targets_only_runs_tests() {
        let filter = TargetFilter::all_test_targets();
        let test_target = mk_target("prelude//rules.bzl:rust_test", "unit_tests");
        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "app");
        let lib_target = mk_target("prelude//rules.bzl:rust_library", "pkg");

        assert!(filter.target_run(&test_target));
        assert!(!filter.target_run(&bin_target));
        assert!(filter.target_run(&lib_target));
    }

    #[test]
    fn lib_only_only_runs_library_target() {
        let filter = TargetFilter::lib_only();
        let lib_target = mk_target("prelude//rules.bzl:rust_library", "pkg");
        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "app");
        let test_target = mk_target("prelude//rules.bzl:rust_test", "tests");

        assert!(filter.target_run(&lib_target));
        assert!(!filter.target_run(&bin_target));
        assert!(!filter.target_run(&test_target));
    }

    #[test]
    fn single_bin_matches_only_given_bin_name() {
        let filter = TargetFilter::single_bin("my-bin".to_string())
            .expect("target filter creation should succeed");
        let wanted_bin = mk_target("prelude//rules.bzl:rust_binary", "my-bin");
        let other_bin = mk_target("prelude//rules.bzl:rust_binary", "other");
        let lib_target = mk_target("prelude//rules.bzl:rust_library", "pkg");

        assert!(filter.target_run(&wanted_bin));
        assert!(!filter.target_run(&other_bin));
        assert!(!filter.target_run(&lib_target));
    }

    #[test]
    fn from_raw_arguments_all_targets_takes_precedence() {
        let filter = TargetFilter::from_raw_arguments(
            false,
            vec!["app".to_string()],
            false,
            vec!["tests".to_string()],
            false,
            vec!["example".to_string()],
            false,
            vec!["bench".to_string()],
            false,
            true,
            FilterCaller::Build,
        )
        .expect("target filter creation should succeed");

        assert!(filter.is_all_targets());
        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "any-bin");
        let test_target = mk_target("prelude//rules.bzl:rust_test", "any-test");
        let lib_target = mk_target("prelude//rules.bzl:rust_library", "any-lib");
        assert!(filter.target_run(&bin_target));
        assert!(filter.target_run(&test_target));
        assert!(filter.target_run(&lib_target));
    }

    #[test]
    fn from_raw_arguments_default_when_all_empty() {
        let filter = TargetFilter::from_raw_arguments(
            false,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            false,
            FilterCaller::Build,
        )
        .expect("target filter creation should succeed");

        assert!(matches!(filter, TargetFilter::Default(_)));
        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "app");
        let test_target = mk_target("prelude//rules.bzl:rust_test", "tests");
        let lib_target = mk_target("prelude//rules.bzl:rust_library", "pkg");
        assert!(filter.target_run(&bin_target));
        assert!(!filter.target_run(&test_target));
        assert!(filter.target_run(&lib_target));
    }

    #[test]
    fn from_raw_arguments_lib_only_sets_only_mode() {
        let filter = TargetFilter::from_raw_arguments(
            true,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            false,
            FilterCaller::Build,
        )
        .expect("target filter creation should succeed");

        let lib_target = mk_target("prelude//rules.bzl:rust_library", "pkg");
        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "app");
        assert!(filter.target_run(&lib_target));
        assert!(!filter.target_run(&bin_target));
    }

    #[test]
    fn from_raw_arguments_all_bins_runs_any_bin() {
        let filter = TargetFilter::from_raw_arguments(
            false,
            vec![],
            true,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            false,
            FilterCaller::Build,
        )
        .expect("target filter creation should succeed");

        let bin_target = mk_target("prelude//rules.bzl:rust_binary", "bin-a");
        let test_target = mk_target("prelude//rules.bzl:rust_test", "tests");
        assert!(filter.target_run(&bin_target));
        assert!(!filter.target_run(&test_target));
    }

    #[test]
    fn from_raw_arguments_returns_error_for_invalid_glob() {
        let err = TargetFilter::from_raw_arguments(
            false,
            vec!["invalid[glob".to_string()],
            false,
            vec![],
            false,
            vec![],
            false,
            vec![],
            false,
            false,
            FilterCaller::Build,
        )
        .expect_err("target filter creation should fail");

        let msg = err.to_string();
        assert!(msg.contains("unclosed character class"));
    }

    #[test]
    fn target_run_returns_false_for_unsupported_kind_in_only_mode() {
        let filter = TargetFilter::single_bin("app".to_string())
            .expect("target filter creation should succeed");
        let unsupported = mk_target("prelude//rules.bzl:filegroup", "assets");

        assert!(!filter.target_run(&unsupported));
    }
}
