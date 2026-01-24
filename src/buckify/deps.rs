use std::{
    collections::{BTreeSet as Set, HashMap},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Node, NodeDep, Package, PackageId, Target};

use crate::{
    RUST_CRATES_ROOT,
    buck::{CargoTargetKind, RustRule},
    buckal_note, buckal_warn,
    context::BuckalContext,
    platform::{Os, oses_from_platform, platform_is_target_only},
    utils::get_buck2_root,
};

pub(super) fn dep_kind_matches(target_kind: CargoTargetKind, dep_kind: DependencyKind) -> bool {
    match target_kind {
        CargoTargetKind::CustomBuild => dep_kind == DependencyKind::Build,
        // Cargo test targets can depend on both dev-deps and regular deps.
        CargoTargetKind::Test => {
            dep_kind == DependencyKind::Development || dep_kind == DependencyKind::Normal
        }
        _ => dep_kind == DependencyKind::Normal,
    }
}

fn get_lib_targets(package: &Package) -> Vec<&Target> {
    package
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
        .collect()
}

fn resolve_first_party_label(dep_package: &Package) -> Result<String> {
    let buck2_root = get_buck2_root().context("failed to get buck2 root")?;
    let manifest_path = PathBuf::from(&dep_package.manifest_path);
    let manifest_dir = manifest_path
        .parent()
        .context("manifest_path should always have a parent directory")?;
    let relative_path = manifest_dir
        .strip_prefix(&buck2_root)
        .with_context(|| {
            format!(
                "dependency manifest dir `{}` is not under Buck2 root `{}`",
                manifest_dir.display(),
                buck2_root
            )
        })?
        .to_string_lossy();

    let dep_bin_targets: Vec<_> = dep_package
        .targets
        .iter()
        .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Bin))
        .collect();

    let dep_lib_targets = get_lib_targets(dep_package);

    if dep_lib_targets.len() != 1 {
        bail!(
            "Expected exactly one library target for dependency {}, but found {}",
            dep_package.name,
            dep_lib_targets.len()
        );
    }

    let buckal_name = if dep_bin_targets
        .iter()
        .any(|b| b.name == dep_lib_targets[0].name)
    {
        format!("lib{}", dep_lib_targets[0].name)
    } else {
        dep_lib_targets[0].name.to_owned()
    };

    Ok(format!("//{relative_path}:{buckal_name}"))
}

fn resolve_dep_label(
    dep: &NodeDep,
    dep_package: &Package,
    use_workspace_alias: bool,
) -> Result<(String, Option<String>)> {
    let dep_package_name = dep_package.name.to_string();
    let is_renamed = dep.name != dep_package_name.replace("-", "_");
    let alias = if is_renamed {
        Some(dep.name.clone())
    } else {
        None
    };

    if dep_package.source.is_none() {
        let label = resolve_first_party_label(dep_package).with_context(|| {
            format!(
                "failed to resolve first-party label for `{}`",
                dep_package.name
            )
        })?;
        Ok((label, alias))
    } else {
        // third-party dependency
        Ok((
            if use_workspace_alias {
                format!("//third-party/rust:{}", dep_package.name)
            } else {
                format!(
                    "//{RUST_CRATES_ROOT}/{}/{}:{}",
                    dep_package.name, dep_package.version, dep_package.name
                )
            },
            alias,
        ))
    }
}

/// Insert a dependency label into `rust_rule` in the appropriate attribute.
///
/// `target` is the Buck label we want the rule to depend on. If `alias` is `Some`, the
/// dependency is recorded as a *named* dependency (used for renamed crates); otherwise it is
/// recorded as an unnamed dependency.
///
/// # Platforms
///
/// `platforms` controls whether the dependency is unconditional or platform-specific:
/// - `None` means the dependency applies on all platforms and is inserted into `deps` or
///   `named_deps`.
/// - `Some(set)` means the dependency is conditional and is inserted into `os_deps` or
///   `os_named_deps` for each OS in `set`.
///
/// # Conflict handling
///
/// - For unconditional named dependencies (`named_deps`), if an alias is encountered more than
///   once with different targets, we emit a warning and keep the first value.
/// - For platform-specific named dependencies (`os_named_deps`), an alias may map to only one
///   target per OS. Conflicting targets for the same `(alias, os)` are treated as an error.
fn insert_dep(
    rust_rule: &mut dyn RustRule,
    target: &str,
    alias: Option<&str>,
    platforms: Option<&Set<Os>>,
) -> Result<()> {
    if let Some(platforms) = platforms {
        for os in platforms {
            let os_key = os.key().to_owned();
            if let Some(alias) = alias {
                let entries = rust_rule
                    .os_named_deps_mut()
                    .entry(alias.to_owned())
                    .or_default();

                if let Some(existing) = entries.get(&os_key) {
                    if existing != target {
                        bail!(
                            "os_named_deps alias '{}' had conflicting targets for platform '{}': '{}' vs '{}'",
                            alias,
                            os_key,
                            existing,
                            target
                        );
                    }
                } else {
                    entries.insert(os_key.clone(), target.to_owned());
                }
            } else {
                rust_rule
                    .os_deps_mut()
                    .entry(os_key)
                    .or_default()
                    .insert(target.to_owned());
            }
        }
    } else if let Some(alias) = alias {
        let entry = rust_rule.named_deps_mut().entry(alias.to_owned());
        match entry {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(target.to_owned());
            }
            std::collections::btree_map::Entry::Occupied(o) => {
                if o.get() != target {
                    buckal_warn!(
                        "named_deps alias '{}' had conflicting targets: '{}' vs '{}'",
                        alias,
                        o.get(),
                        target
                    );
                }
            }
        }
    } else {
        rust_rule.deps_mut().insert(target.to_owned());
    }
    Ok(())
}

pub(super) fn set_deps(
    rust_rule: &mut dyn RustRule,
    node: &Node,
    packages_map: &HashMap<PackageId, Package>,
    kind: CargoTargetKind,
    ctx: &BuckalContext,
) -> Result<()> {
    let use_workspace_alias = ctx.repo_config.inherit_workspace_deps && node.id == ctx.root.id;

    for dep in &node.deps {
        let Some(dep_package) = packages_map.get(&dep.pkg) else {
            continue;
        };

        let mut unconditional = false;
        let mut platforms = Set::<Os>::new();
        let mut has_unsupported_platform = false;

        for dk in dep
            .dep_kinds
            .iter()
            .filter(|dk| dep_kind_matches(kind, dk.kind))
        {
            match &dk.target {
                None => unconditional = true,
                Some(platform) => {
                    let oses = oses_from_platform(platform);
                    if oses.is_empty() {
                        if platform_is_target_only(platform) {
                            has_unsupported_platform = true;
                            continue;
                        }
                        unconditional = true;
                        continue;
                    }
                    platforms.extend(oses);
                }
            }
        }

        if !unconditional && platforms.is_empty() {
            if has_unsupported_platform {
                buckal_note!(
                    "Dependency '{}' (package '{}') targets only unsupported platforms and will be omitted.",
                    dep.name,
                    dep_package.name
                );
            }
            continue;
        }

        let (target_label, alias) = resolve_dep_label(dep, dep_package, use_workspace_alias)
            .with_context(|| {
                format!(
                    "failed to resolve dependency label for '{}' (package '{}')",
                    dep.name, dep_package.name
                )
            })?;

        if unconditional {
            insert_dep(rust_rule, &target_label, alias.as_deref(), None)?;
        } else {
            insert_dep(rust_rule, &target_label, alias.as_deref(), Some(&platforms))?;
        }
    }
    Ok(())
}
