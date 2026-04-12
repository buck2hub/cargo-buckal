use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use semver::Version;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use crate::{
    buckal_log,
    cache::{BuckalCache, BuckalChange, ChangeType},
    config::{RepoConfig, VersionPatch},
    context::BuckalContext,
    resolve::{BuckalNode, NodeKind},
    utils::{UnwrapOrExit, ensure_prerequisites, section},
};

#[derive(Parser, Debug)]
pub struct PatchArgs {
    /// Dependency to patch, optionally pinned to the version to keep
    #[arg(value_name = "SPEC")]
    pub package: String,

    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedPatch {
    name: String,
    version_patch: VersionPatch,
}

pub fn execute(args: &PatchArgs) {
    ensure_prerequisites().unwrap_or_exit();

    let mut ctx = BuckalContext::new(args.manifest_path.clone());
    let selected = select_patch_for_spec(&ctx, &args.package)
        .unwrap_or_exit_ctx("failed to determine dependency patch");

    // Compute targeted BUCK changes before persisting to buckal.toml so that
    // a failure here does not leave a partially-written config file.
    let changes = targeted_patch_changes(&ctx, &selected)
        .unwrap_or_exit_ctx("failed to compute patched BUCK updates");

    upsert_version_patch(
        &RepoConfig::repo_config_path(),
        &selected.name,
        &selected.version_patch,
    )
    .unwrap_or_exit_ctx("failed to update buckal.toml");

    // Update the in-memory config so label emission and cache use the new patch.
    ctx.repo_config
        .patch
        .version
        .insert(selected.name.clone(), selected.version_patch.clone());

    section("Buckal Console");
    buckal_log!(
        "Patching",
        format!(
            "{} {} -> {}",
            selected.name, selected.version_patch.from, selected.version_patch.to
        )
    );

    changes.apply(&ctx);
    BuckalCache::from_resolve(&ctx.resolve, &ctx.workspace_root, &ctx.repo_config.patch).save();
}

fn select_patch_for_spec(ctx: &BuckalContext, spec: &str) -> Result<SelectedPatch> {
    let (name, requested_to) = parse_package_spec(spec);
    let versions = collect_third_party_versions(ctx, name);

    if versions.len() < 2 {
        bail!(
            "dependency '{}' only has one resolved third-party version ({})",
            name,
            versions.join(", ")
        );
    }

    let version_patch = if let Some(requested_to) = requested_to {
        if !versions.iter().any(|version| version == requested_to) {
            bail!(
                "dependency '{}' does not resolve version '{}'; available versions: {}",
                name,
                requested_to,
                versions.join(", ")
            );
        }

        let from_versions: Vec<_> = versions
            .iter()
            .filter(|version| version.as_str() != requested_to)
            .cloned()
            .collect();

        if from_versions.len() != 1 {
            bail!(
                "dependency '{}' resolves multiple source versions ({}); specify a unique target by first reducing the graph to exactly two versions",
                name,
                versions.join(", ")
            );
        }

        VersionPatch {
            from: from_versions[0].clone(),
            to: requested_to.to_string(),
        }
    } else {
        if versions.len() != 2 {
            bail!(
                "dependency '{}' resolves {} versions ({}); specify the version to keep as '{}@<VERSION>'",
                name,
                versions.len(),
                versions.join(", "),
                name
            );
        }

        VersionPatch {
            from: versions[0].clone(),
            to: versions[1].clone(),
        }
    };

    Ok(SelectedPatch {
        name: name.to_string(),
        version_patch,
    })
}

fn collect_third_party_versions(ctx: &BuckalContext, name: &str) -> Vec<String> {
    let mut versions: Vec<String> = ctx
        .resolve
        .nodes()
        .filter(|node| matches!(node.kind, NodeKind::ThirdParty) && node.name == name)
        .map(|node| node.version.clone())
        .collect();

    versions.sort_by(|a, b| version_cmp(a, b));
    versions.dedup();
    versions
}

fn targeted_patch_changes(ctx: &BuckalContext, selected: &SelectedPatch) -> Result<BuckalChange> {
    let from_node = find_unique_third_party_node(
        ctx.resolve.nodes(),
        &selected.name,
        &selected.version_patch.from,
    )?;
    let to_node = find_unique_third_party_node(
        ctx.resolve.nodes(),
        &selected.name,
        &selected.version_patch.to,
    )?;

    let mut changes = BTreeMap::new();
    changes.insert(to_node.package_id.clone(), ChangeType::Changed);

    for dependent in ctx.resolve.dependents(&from_node.package_id) {
        changes.insert(dependent.package_id.clone(), ChangeType::Changed);
    }

    Ok(BuckalChange { changes })
}

fn find_unique_third_party_node<'a>(
    nodes: impl Iterator<Item = &'a BuckalNode>,
    name: &str,
    version: &str,
) -> Result<&'a BuckalNode> {
    let matches: Vec<&BuckalNode> = nodes
        .filter(|node| {
            matches!(node.kind, NodeKind::ThirdParty)
                && node.name == name
                && node.version == version
        })
        .collect();

    match matches.as_slice() {
        [node] => Ok(*node),
        [] => Err(anyhow!(
            "dependency '{}' version '{}' was not found in the resolved graph",
            name,
            version
        )),
        _ => Err(anyhow!(
            "dependency '{}' version '{}' is ambiguous in the resolved graph",
            name,
            version
        )),
    }
}

fn upsert_version_patch(
    repo_config_path: &std::path::Path,
    dep_name: &str,
    patch: &VersionPatch,
) -> Result<()> {
    let mut doc = if repo_config_path.exists() {
        fs::read_to_string(repo_config_path)
            .with_context(|| format!("failed to read {}", repo_config_path.display()))?
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", repo_config_path.display()))?
    } else {
        DocumentMut::new()
    };

    insert_version_patch(&mut doc, dep_name, patch)?;

    fs::write(repo_config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", repo_config_path.display()))?;
    Ok(())
}

fn insert_version_patch(doc: &mut DocumentMut, dep_name: &str, patch: &VersionPatch) -> Result<()> {
    let patch_item = doc.entry("patch").or_insert(Item::Table(Table::new()));
    let Some(patch_table) = patch_item.as_table_mut() else {
        bail!("[patch] must be a table");
    };

    if !patch_table.contains_key("version") {
        patch_table.insert("version", Item::Table(Table::new()));
    }

    let Some(version_table) = patch_table["version"].as_table_mut() else {
        bail!("[patch.version] must be a table");
    };

    let mut inline = InlineTable::new();
    inline.insert("from", Value::from(patch.from.clone()));
    inline.insert("to", Value::from(patch.to.clone()));
    version_table.insert(dep_name, Item::Value(Value::InlineTable(inline)));
    Ok(())
}

fn parse_package_spec(spec: &str) -> (&str, Option<&str>) {
    if let Some((name, version)) = spec.split_once('@') {
        (name, Some(version))
    } else {
        (spec, None)
    }
}

fn version_cmp(left: &str, right: &str) -> Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_ctx(name: &str, versions: &[&str]) -> BuckalContext {
        use std::collections::HashMap;

        use cargo_metadata::{Edition, PackageId, TargetKind, camino::Utf8PathBuf};
        use daggy::Dag;

        use crate::{
            config::RepoPatchConfig,
            resolve::{BuckalResolve, BuckalTarget},
        };

        let mut dag = Dag::new();
        let mut index_map = HashMap::new();

        for version in versions {
            let node = BuckalNode {
                package_id: PackageId {
                    repr: format!(
                        "registry+https://github.com/rust-lang/crates.io-index#{name}@{version}"
                    ),
                },
                name: name.to_string(),
                version: (*version).to_string(),
                features: vec![],
                kind: NodeKind::ThirdParty,
                edition: Edition::E2021,
                manifest_path: Utf8PathBuf::from(format!("/tmp/{name}/{version}/Cargo.toml")),
                targets: vec![BuckalTarget {
                    name: name.to_string(),
                    kind: vec![TargetKind::Lib],
                    src_path: Utf8PathBuf::from(format!("/tmp/{name}/{version}/src/lib.rs")),
                    doctest: true,
                    test: true,
                }],
                source: Some("registry+https://github.com/rust-lang/crates.io-index".to_string()),
                links: None,
                checksum: None,
            };
            let pkg_id = node.package_id.clone();
            let idx = dag.add_node(node);
            index_map.insert(pkg_id, idx);
        }

        BuckalContext {
            root: None,
            resolve: BuckalResolve { dag, index_map },
            workspace_root: Utf8PathBuf::from("/tmp/workspace"),
            workspace_inherit: false,
            no_merge: false,
            repo_config: RepoConfig {
                patch: RepoPatchConfig::default(),
                ..RepoConfig::default()
            },
        }
    }

    #[test]
    fn test_select_patch_without_explicit_target_uses_higher_version() {
        let ctx = mock_ctx("pyo3", &["0.26.0", "0.27.2"]);

        let selected = select_patch_for_spec(&ctx, "pyo3").expect("failed to select patch");

        assert_eq!(
            selected.version_patch,
            VersionPatch {
                from: "0.26.0".to_string(),
                to: "0.27.2".to_string(),
            }
        );
    }

    #[test]
    fn test_select_patch_with_explicit_target_version() {
        let ctx = mock_ctx("pyo3", &["0.26.0", "0.27.2"]);

        let selected = select_patch_for_spec(&ctx, "pyo3@0.27.2").expect("failed to select patch");

        assert_eq!(selected.version_patch.from, "0.26.0");
        assert_eq!(selected.version_patch.to, "0.27.2");
    }

    #[test]
    fn test_select_patch_requires_unique_other_version() {
        let ctx = mock_ctx("pyo3", &["0.25.0", "0.26.0", "0.27.2"]);

        let err = select_patch_for_spec(&ctx, "pyo3@0.27.2").expect_err("expected ambiguity");

        assert!(
            err.to_string()
                .contains("resolves multiple source versions")
        );
    }

    #[test]
    fn test_insert_version_patch_writes_expected_shape() {
        let mut doc = DocumentMut::new();
        let patch = VersionPatch {
            from: "0.26.0".to_string(),
            to: "0.27.2".to_string(),
        };

        insert_version_patch(&mut doc, "pyo3", &patch).unwrap();

        let rendered = doc.to_string();
        assert!(rendered.contains("[patch.version]"));
        assert!(rendered.contains("pyo3 = { from = \"0.26.0\", to = \"0.27.2\" }"));
    }
}
