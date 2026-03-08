use std::{borrow::Cow, collections::BTreeSet as Set};

use cargo_metadata::{Node, Package, Target, camino::Utf8PathBuf};
use cargo_util_schemas::lockfile::TomlLockfileSourceId;

use crate::{
    buck::{
        BuildscriptRun, CargoManifest, CargoTargetKind, FileGroup, GitFetch, Glob, HttpArchive,
        RustBinary, RustLibrary, RustRule, RustTest,
    },
    context::BuckalContext,
    platform::{buck_labels, lookup_platforms},
    utils::{UnwrapOrExit, get_cfgs, get_target, get_vendor_path_relative},
};

use super::deps::{dep_kind_matches, set_deps};

/// Emit `rust_library` rule for the given lib target
pub(super) fn emit_rust_library(
    package: &Package,
    node: &Node,
    lib_target: &Target,
    manifest_dir: &Utf8PathBuf,
    buckal_name: &str,
    ctx: &BuckalContext,
) -> RustLibrary {
    let mut rust_library = RustLibrary {
        name: buckal_name.to_owned(),
        srcs: Set::from([get_vendor_target()]),
        crate_name: lib_target.name.to_owned().replace("-", "_"),
        edition: package.edition.to_string(),
        features: Set::from_iter(node.features.iter().map(|f| f.to_string())),
        rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_owned()]),
        visibility: Set::from(["PUBLIC".to_owned()]),
        ..Default::default()
    };

    if lib_target
        .kind
        .contains(&cargo_metadata::TargetKind::ProcMacro)
    {
        rust_library.proc_macro = Some(true);
    }

    // Set the crate root path
    rust_library.crate_root = format!(
        "{}/{}",
        get_vendor_name(),
        normalize_path_for_buck(
            lib_target
                .src_path
                .to_owned()
                .strip_prefix(manifest_dir)
                .expect("Failed to get library source path")
                .as_str()
        )
    );

    // look up platform compatibility
    if let Some(platforms) = lookup_platforms(&package.name) {
        rust_library.compatible_with = buck_labels(&platforms);
    }

    // Set dependencies
    set_deps(&mut rust_library, node, CargoTargetKind::Lib, ctx)
        .unwrap_or_exit_ctx(format!("failed to set dependencies for '{}'", buckal_name));

    rust_library
}

/// Emit `rust_binary` rule for the given bin target
pub(super) fn emit_rust_binary(
    package: &Package,
    node: &Node,
    bin_target: &Target,
    manifest_dir: &Utf8PathBuf,
    buckal_name: &str,
    ctx: &BuckalContext,
) -> RustBinary {
    let mut rust_binary = RustBinary {
        name: buckal_name.to_owned(),
        srcs: Set::from([get_vendor_target()]),
        crate_name: bin_target.name.to_owned().replace("-", "_"),
        edition: package.edition.to_string(),
        features: Set::from_iter(node.features.iter().map(|f| f.to_string())),
        rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_owned()]),
        visibility: Set::from(["PUBLIC".to_owned()]),
        ..Default::default()
    };

    // Set the crate root path
    rust_binary.crate_root = format!(
        "{}/{}",
        get_vendor_name(),
        normalize_path_for_buck(
            bin_target
                .src_path
                .to_owned()
                .strip_prefix(manifest_dir)
                .expect("Failed to get binary source path")
                .as_str()
        )
    );

    // Set dependencies
    set_deps(&mut rust_binary, node, CargoTargetKind::Bin, ctx)
        .unwrap_or_exit_ctx(format!("failed to set dependencies for '{}'", buckal_name));

    if let Some(platforms) = lookup_platforms(&package.name) {
        rust_binary.compatible_with = buck_labels(&platforms);
    }

    rust_binary
}

/// Emit `rust_test` rule for the given bin target
pub(super) fn emit_rust_test(
    package: &Package,
    node: &Node,
    test_target: &Target,
    manifest_dir: &Utf8PathBuf,
    buckal_name: &str,
    ctx: &BuckalContext,
) -> RustTest {
    let mut rust_test = RustTest {
        name: buckal_name.to_owned(),
        srcs: Set::from([get_vendor_target()]),
        crate_name: test_target.name.to_owned().replace("-", "_"),
        edition: package.edition.to_string(),
        features: Set::from_iter(node.features.iter().map(|f| f.to_string())),
        rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_owned()]),
        visibility: Set::from(["PUBLIC".to_owned()]),
        ..Default::default()
    };

    // Set the crate root path
    rust_test.crate_root = format!(
        "{}/{}",
        get_vendor_name(),
        normalize_path_for_buck(
            test_target
                .src_path
                .to_owned()
                .strip_prefix(manifest_dir)
                .expect("Failed to get test source path")
                .as_str()
        )
    );

    // Set dependencies
    set_deps(&mut rust_test, node, CargoTargetKind::Test, ctx)
        .unwrap_or_exit_ctx(format!("failed to set dependencies for '{}'", buckal_name));

    if let Some(platforms) = lookup_platforms(&package.name) {
        rust_test.compatible_with = buck_labels(&platforms);
    }

    rust_test
}

/// Emit `buildscript_build` rule for the given build target
pub(super) fn emit_buildscript_build(
    build_target: &Target,
    package: &Package,
    node: &Node,
    manifest_dir: &Utf8PathBuf,
    ctx: &BuckalContext,
) -> RustBinary {
    // create the build script rule
    let mut buildscript_build = RustBinary {
        name: build_target.name.to_owned(),
        srcs: Set::from([get_vendor_target()]),
        crate_name: build_target.name.to_owned().replace("-", "_"),
        edition: package.edition.to_string(),
        features: Set::from_iter(node.features.iter().map(|f| f.to_string())),
        rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_owned()]),
        ..Default::default()
    };

    // Set the crate root path for the build script
    buildscript_build.crate_root = format!(
        "{}/{}",
        get_vendor_name(),
        normalize_path_for_buck(
            build_target
                .src_path
                .to_owned()
                .strip_prefix(manifest_dir)
                .expect("Failed to get build script source path")
                .as_str()
        )
    );

    // Set dependencies for the build script
    set_deps(
        &mut buildscript_build,
        node,
        CargoTargetKind::CustomBuild,
        ctx,
    )
    .unwrap_or_exit_ctx(format!(
        "failed to set dependencies for '{}'",
        &buildscript_build.name
    ));

    buildscript_build
}

/// Emit `buildscript_run` rule for the given build target
pub(super) fn emit_buildscript_run(
    package: &Package,
    node: &Node,
    build_target: &Target,
    ctx: &BuckalContext,
) -> BuildscriptRun {
    // create the build script run rule
    let build_name = get_build_name(&build_target.name);
    let mut buildscript_run = BuildscriptRun {
        name: format!("{}-run", build_name),
        package_name: package.name.to_string(),
        buildscript_rule: format!(":{}", build_target.name),
        env_srcs: Set::from([":manifest[env_dict]".to_owned()]),
        features: Set::from_iter(node.features.iter().map(|f| f.to_string())),
        version: package.version.to_string(),
        manifest_dir: get_vendor_target(),
        visibility: Set::from(["PUBLIC".to_owned()]),
        ..Default::default()
    };

    let host_target = get_target();
    let host_cfgs = get_cfgs();

    // Set environment variables from dependencies
    // See https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key
    for dep in &node.deps {
        if let Some(dep_package) = ctx.packages_map.get(&dep.pkg)
            && dep_package.links.is_some()
            && dep.dep_kinds.iter().any(|dk| {
                dep_kind_matches(CargoTargetKind::Lib, dk.kind)
                    && dk
                        .target
                        .as_ref()
                        .map(|platform| platform.matches(&host_target, &host_cfgs[..]))
                        .unwrap_or(true)
            })
        {
            // Only normal dependencies with The links Manifest Key for current arch are considered
            let custom_build_target_dep = dep_package
                .targets
                .iter()
                .find(|t| t.kind.contains(&cargo_metadata::TargetKind::CustomBuild));
            if let Some(build_target_dep) = custom_build_target_dep {
                let build_name_dep = get_build_name(&build_target_dep.name);
                buildscript_run.env_srcs.insert(format!(
                    "//{}:{build_name_dep}-run[metadata]",
                    get_vendor_path_relative(&dep_package.id).unwrap_or_exit()
                ));
            } else {
                panic!(
                    "Dependency {} has links key but no build script target",
                    dep_package.name
                );
            }
        }
    }

    buildscript_run
}

/// Patch the given `rust_library` or `rust_binary` rule to support build scripts
pub(super) fn patch_with_buildscript(rust_rule: &mut dyn RustRule, build_target: &Target) {
    let build_name = get_build_name(&build_target.name);
    rust_rule.env_mut().insert(
        "OUT_DIR".to_owned(),
        format!("$(location :{build_name}-run[out_dir])").to_owned(),
    );
    rust_rule
        .rustc_flags_mut()
        .insert(format!("@$(location :{build_name}-run[rustc_flags])",).to_owned());
}

/// Emit `http_archive` rule for the given package
pub(super) fn emit_http_archive(package: &Package, ctx: &BuckalContext) -> HttpArchive {
    let url = format!(
        "https://static.crates.io/crates/{}/{}-{}.crate",
        package.name, package.name, package.version
    );
    let buckal_name = format!("{}-{}", package.name, package.version);
    let checksum = ctx
        .checksums_map
        .get(&format!("{}-{}", package.name, package.version))
        .unwrap();

    HttpArchive {
        name: get_vendor_name().to_string(),
        urls: Set::from([url]),
        sha256: checksum.to_string(),
        _type: "tar.gz".to_owned(),
        strip_prefix: buckal_name,
        out: None,
    }
}

/// Emit `filegroup` rule for the given package
pub(super) fn emit_filegroup() -> FileGroup {
    FileGroup {
        name: get_vendor_name().to_string(),
        srcs: Glob {
            include: Set::from(["**/**".to_owned()]),
            ..Default::default()
        },
        out: None,
    }
}

/// Emit `git_fetch` rule for the given package
pub(super) fn emit_git_fetch(package: &Package) -> GitFetch {
    let source_id = TomlLockfileSourceId::new(
        package
            .source
            .as_ref()
            .expect("failed to get package source")
            .repr
            .to_owned(),
    )
    .unwrap_or_exit();

    let mut git_repo = source_id.url().to_owned();
    git_repo.set_fragment(None);
    git_repo.set_query(None);

    GitFetch {
        name: get_vendor_name().to_string(),
        repo: git_repo.to_string(),
        rev: source_id.url().fragment().unwrap().to_string(),
    }
}

/// Emit `cargo_manifest` rule for the given package
pub(super) fn emit_cargo_manifest() -> CargoManifest {
    CargoManifest {
        name: "manifest".to_owned(),
        vendor: get_vendor_target(),
    }
}

fn get_build_name(s: &str) -> Cow<'_, str> {
    if let Some(stripped) = s.strip_suffix("-build") {
        Cow::Owned(stripped.to_string())
    } else {
        Cow::Borrowed(s)
    }
}

/// Get the name of the vendor target
fn get_vendor_name() -> Cow<'static, str> {
    Cow::Borrowed("vendor")
}

/// Get the label of the vendor target
fn get_vendor_target() -> String {
    format!(":{}", get_vendor_name())
}

/// Normalize a path for Buck by converting backslashes to forward slashes.
/// This normalization is critical on Windows, where paths use backslashes,
/// as Buck2 requires forward slashes in all generated BUCK files regardless of the host platform.
fn normalize_path_for_buck(path: &str) -> String {
    path.replace('\\', "/")
}
