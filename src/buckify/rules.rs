use std::{collections::BTreeSet as Set, vec};

use cargo_metadata::{Node, Package, camino::Utf8PathBuf};
use itertools::Itertools;

use crate::{
    buck::{Load, Rule, RustRule},
    context::BuckalContext,
    utils::{UnwrapOrExit, get_vendor_dir},
};

use super::emit::{
    emit_buildscript_build, emit_buildscript_run, emit_cargo_manifest, emit_filegroup,
    emit_http_archive, emit_rust_binary, emit_rust_library, emit_rust_test, patch_with_buildscript,
};

pub fn buckify_dep_node(node: &Node, ctx: &BuckalContext) -> Vec<Rule> {
    let package = ctx.packages_map.get(&node.id).unwrap().to_owned();

    // emit buck rules for lib target
    let mut buck_rules: Vec<Rule> = Vec::new();

    let manifest_dir = package.manifest_path.parent().unwrap().to_owned();
    let lib_target = package
        .targets
        .iter()
        .find(|t| {
            t.kind.contains(&cargo_metadata::TargetKind::Lib)
                || t.kind.contains(&cargo_metadata::TargetKind::CDyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::DyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::RLib)
                || t.kind.contains(&cargo_metadata::TargetKind::StaticLib)
                || t.kind.contains(&cargo_metadata::TargetKind::ProcMacro)
        })
        .expect("No library target found");

    let http_archive = emit_http_archive(&package, ctx);
    buck_rules.push(Rule::HttpArchive(http_archive));

    let cargo_manifest = emit_cargo_manifest(&package);
    buck_rules.push(Rule::CargoManifest(cargo_manifest));

    let rust_library = emit_rust_library(
        &package,
        node,
        &ctx.packages_map,
        lib_target,
        &manifest_dir,
        &package.name,
        ctx,
    );

    buck_rules.push(Rule::RustLibrary(rust_library));

    // Check if the package has a build script
    let custom_build_target = package
        .targets
        .iter()
        .find(|t| t.kind.contains(&cargo_metadata::TargetKind::CustomBuild));

    if let Some(build_target) = custom_build_target {
        // Patch the rust_library rule to support build scripts
        for rule in &mut buck_rules {
            if let Some(rust_rule) = rule.as_rust_rule_mut() {
                patch_with_buildscript(rust_rule, build_target, &package);
            }
        }

        // create the build script rule
        let buildscript_build = emit_buildscript_build(
            build_target,
            &package,
            node,
            &ctx.packages_map,
            &manifest_dir,
            ctx,
        );
        buck_rules.push(Rule::RustBinary(buildscript_build));

        // create the build script run rule
        let buildscript_run = emit_buildscript_run(&package, node, &ctx.packages_map, build_target);
        buck_rules.push(Rule::BuildscriptRun(buildscript_run));
    }

    buck_rules
}

pub fn buckify_root_node(node: &Node, ctx: &BuckalContext) -> Vec<Rule> {
    let package = ctx.packages_map.get(&node.id).unwrap().to_owned();

    let bin_targets = package
        .targets
        .iter()
        .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Bin))
        .collect::<Vec<_>>();

    let lib_targets = package
        .targets
        .iter()
        .filter(|t| {
            t.kind.contains(&cargo_metadata::TargetKind::Lib)
                || t.kind.contains(&cargo_metadata::TargetKind::CDyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::DyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::RLib)
                || t.kind.contains(&cargo_metadata::TargetKind::StaticLib)
                || t.kind.contains(&cargo_metadata::TargetKind::ProcMacro)
        })
        .collect::<Vec<_>>();

    let test_targets = package
        .targets
        .iter()
        .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Test))
        .collect::<Vec<_>>();

    let mut buck_rules: Vec<Rule> = Vec::new();

    let manifest_dir = package.manifest_path.parent().unwrap().to_owned();

    // emit filegroup rule for vendor
    let filegroup = emit_filegroup(&package);
    buck_rules.push(Rule::FileGroup(filegroup));

    let cargo_manifest = emit_cargo_manifest(&package);
    buck_rules.push(Rule::CargoManifest(cargo_manifest));

    // emit buck rules for bin targets
    for bin_target in &bin_targets {
        let buckal_name = bin_target.name.to_owned();

        let mut rust_binary = emit_rust_binary(
            &package,
            node,
            &ctx.packages_map,
            bin_target,
            &manifest_dir,
            &buckal_name,
            ctx,
        );

        if lib_targets.iter().any(|l| l.name == bin_target.name) {
            // Cargo allows `main.rs` to use items from `lib.rs` via the crate's own name by default.
            rust_binary
                .deps_mut()
                .insert(format!(":{}-lib", bin_target.name));
        }

        buck_rules.push(Rule::RustBinary(rust_binary));
    }

    // emit buck rules for lib targets
    for lib_target in &lib_targets {
        let buckal_name = if bin_targets.iter().any(|b| b.name == lib_target.name) {
            format!("{}-lib", lib_target.name)
        } else {
            lib_target.name.to_owned()
        };

        let rust_library = emit_rust_library(
            &package,
            node,
            &ctx.packages_map,
            lib_target,
            &manifest_dir,
            &buckal_name,
            ctx,
        );

        buck_rules.push(Rule::RustLibrary(rust_library));

        if !ctx.repo_config.ignore_tests && lib_target.test {
            // If the library target has inline tests, emit a rust_test rule for it
            let buckal_name = format!("{}-unittest", lib_target.name);

            let rust_test = emit_rust_test(
                &package,
                node,
                &ctx.packages_map,
                lib_target,
                &manifest_dir,
                &buckal_name,
                ctx,
            );

            buck_rules.push(Rule::RustTest(rust_test));
        }
    }

    // emit buck rules for integration test
    if !ctx.repo_config.ignore_tests {
        for test_target in &test_targets {
            let buckal_name = test_target.name.to_owned();

            let mut rust_test = emit_rust_test(
                &package,
                node,
                &ctx.packages_map,
                test_target,
                &manifest_dir,
                &buckal_name,
                ctx,
            );

            let package_name = package.name.replace("-", "_");
            let mut lib_alias = false;
            if bin_targets.iter().any(|b| b.name == package_name) {
                lib_alias = true;
                rust_test.env_mut().insert(
                    format!("CARGO_BIN_EXE_{}", package_name),
                    format!("$(location :{})", package_name),
                );
            }
            if lib_targets.iter().any(|l| l.name == package_name) {
                if lib_alias {
                    rust_test
                        .deps_mut()
                        .insert(format!(":{}-lib", package_name));
                } else {
                    rust_test.deps_mut().insert(format!(":{}", package_name));
                }
            }

            buck_rules.push(Rule::RustTest(rust_test));
        }
    }

    // Check if the package has a build script
    let custom_build_target = package
        .targets
        .iter()
        .find(|t| t.kind.contains(&cargo_metadata::TargetKind::CustomBuild));

    if let Some(build_target) = custom_build_target {
        // Patch the rust_library and rust_binary rules to support build scripts
        for rule in &mut buck_rules {
            if let Some(rust_rule) = rule.as_rust_rule_mut() {
                patch_with_buildscript(rust_rule, build_target, &package);
            }
        }

        // create the build script rule
        let buildscript_build = emit_buildscript_build(
            build_target,
            &package,
            node,
            &ctx.packages_map,
            &manifest_dir,
            ctx,
        );
        buck_rules.push(Rule::RustBinary(buildscript_build));

        // create the build script run rule
        let buildscript_run = emit_buildscript_run(&package, node, &ctx.packages_map, build_target);
        buck_rules.push(Rule::BuildscriptRun(buildscript_run));
    }

    buck_rules
}

pub fn vendor_package(package: &Package) -> Utf8PathBuf {
    // Vendor the package sources to `third-party/rust/crates/<package_name>/<version>`
    let vendor_dir = get_vendor_dir(&package.name, &package.version.to_string())
        .unwrap_or_exit_ctx("failed to get vendor directory");
    if !vendor_dir.exists() {
        std::fs::create_dir_all(&vendor_dir).expect("Failed to create target directory");
    }

    vendor_dir
}

pub fn gen_buck_content(rules: &[Rule]) -> String {
    // Analyze which rule types are present to build conditional load statements
    let mut has_cargo_manifest = false;
    let mut has_rust_library = false;
    let mut has_rust_binary = false;
    let mut has_rust_test = false;
    let mut has_buildscript_run = false;

    for rule in rules {
        match rule {
            Rule::CargoManifest(_) => has_cargo_manifest = true,
            Rule::RustLibrary(_) => has_rust_library = true,
            Rule::RustBinary(_) => has_rust_binary = true,
            Rule::RustTest(_) => has_rust_test = true,
            Rule::BuildscriptRun(_) => has_buildscript_run = true,
            _ => {}
        }
    }

    // Build load statements based on which rule types are present
    let mut loads: Vec<Rule> = vec![];

    if has_cargo_manifest {
        loads.push(Rule::Load(Load {
            bzl: "@buckal//:cargo_manifest.bzl".to_owned(),
            items: Set::from(["cargo_manifest".to_owned()]),
        }));
    }

    // Build wrapper.bzl load items based on which rust rules are present
    let mut wrapper_items: Set<String> = Set::new();

    if has_rust_library {
        wrapper_items.insert("rust_library".to_owned());
    }
    if has_rust_binary {
        wrapper_items.insert("rust_binary".to_owned());
    }
    if has_rust_test {
        wrapper_items.insert("rust_test".to_owned());
    }
    if has_buildscript_run {
        wrapper_items.insert("buildscript_run".to_owned());
    }

    if !wrapper_items.is_empty() {
        loads.push(Rule::Load(Load {
            bzl: "@buckal//:wrapper.bzl".to_owned(),
            items: wrapper_items,
        }));
    }

    let loads_string = loads
        .iter()
        .map(serde_starlark::to_string)
        .map(|r| r.unwrap())
        .join("");

    let mut content = rules
        .iter()
        .map(serde_starlark::to_string)
        .map(|r| r.unwrap())
        .join("\n");

    content.insert(0, '\n');
    content.insert_str(0, &loads_string);
    content.insert_str(0, "# @generated by `cargo buckal`\n\n");
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RepoConfig;
    use cargo_metadata::{TargetKind, camino::Utf8PathBuf};
    use std::collections::HashMap;

    fn mock_target(name: &str, kind: TargetKind) -> cargo_metadata::Target {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "kind": [kind],
            "crate_types": [],
            "required_features": [],
            "src_path": "/tmp/dummy.rs",
            "edition": "2021",
            "doctest": true,
            "test": true
        }))
        .unwrap()
    }

    fn mock_package(name: &str, targets: Vec<cargo_metadata::Target>) -> Package {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "id": format!("{} 0.1.0 (registry+...)", name),
            "license": null,
            "license_file": null,
            "description": null,
            "source": null,
            "dependencies": [],
            "targets": targets,
            "features": {},
            "manifest_path": "/tmp/Cargo.toml",
            "metadata": null,
            "publish": null,
            "authors": [],
            "categories": [],
            "keywords": [],
            "readme": null,
            "repository": null,
            "homepage": null,
            "documentation": null,
            "edition": "2021",
            "links": null,
            "default_run": null,
            "rust_version": null
        }))
        .unwrap()
    }

    #[test]
    fn test_buckify_root_node_name_collision() {
        let lib = mock_target("foo", TargetKind::Lib);
        let bin = mock_target("foo", TargetKind::Bin);
        let pkg = mock_package("foo", vec![lib, bin]);

        let mut packages_map = HashMap::new();
        packages_map.insert(pkg.id.clone(), pkg.clone());

        let node: Node = serde_json::from_value(serde_json::json!({
            "id": pkg.id.clone(),
            "deps": [],
            "dependencies": [],
            "features": []
        }))
        .unwrap();

        let ctx = BuckalContext {
            packages_map,
            nodes_map: HashMap::new(),
            root: pkg.clone(),
            repo_config: RepoConfig {
                ignore_tests: false,
                ..RepoConfig::default()
            },
            checksums_map: HashMap::new(),
            workspace_root: Utf8PathBuf::from("/tmp"),
            no_merge: false,
            separate: false,
        };

        let rules = buckify_root_node(&node, &ctx);

        let lib_rule = rules.iter().find_map(|r| {
            if let Rule::RustLibrary(l) = r {
                Some(l)
            } else {
                None
            }
        });

        assert!(lib_rule.is_some());
        assert_eq!(lib_rule.unwrap().name, "foo-lib");
    }

    #[test]
    fn test_buckify_root_node_test_deps_lib_alias() {
        let lib = mock_target("foo", TargetKind::Lib);
        let bin = mock_target("foo", TargetKind::Bin);
        let test = mock_target("integration_test", TargetKind::Test);

        let pkg = mock_package("foo", vec![lib, bin, test]);

        let mut packages_map = HashMap::new();
        packages_map.insert(pkg.id.clone(), pkg.clone());

        let node: Node = serde_json::from_value(serde_json::json!({
            "id": pkg.id.clone(),
            "deps": [],
            "dependencies": [],
            "features": []
        }))
        .unwrap();

        let ctx = BuckalContext {
            packages_map,
            nodes_map: HashMap::new(),
            root: pkg.clone(),
            repo_config: RepoConfig {
                ignore_tests: false,
                ..RepoConfig::default()
            },
            checksums_map: HashMap::new(),
            workspace_root: Utf8PathBuf::from("/tmp"),
            no_merge: false,
            separate: false,
        };

        let rules = buckify_root_node(&node, &ctx);

        let test_rule = rules.iter().find_map(|r| {
            if let Rule::RustTest(t) = r {
                if t.name == "integration_test" {
                    Some(t)
                } else {
                    None
                }
            } else {
                None
            }
        });

        assert!(test_rule.is_some());
        let test_rule = test_rule.unwrap();
        assert!(test_rule.deps.contains(":foo-lib"));
    }
}
