use std::collections::HashMap;
use std::path::PathBuf;

use cargo_metadata::{DependencyKind, Edition, PackageId, TargetKind, camino::Utf8PathBuf};
use daggy::{Dag, NodeIndex, Walker};
use serde::{Deserialize, Serialize};

use crate::cache::{BuckalHash, Fingerprint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    FirstParty { relative_path: String },
    ThirdParty,
}

/// A single dependency edge with platform/kind metadata.
///
/// This mirrors the relevant parts of `cargo_metadata::NodeDep` but uses
/// plain serializable types so it can be included in the cache fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckalDep {
    /// The PackageId of the dependency.
    pub pkg: PackageId,
    /// The name of the dependency (may differ from package name if renamed).
    pub name: String,
    /// Dependency kind + optional platform constraint for each edge.
    pub dep_kinds: Vec<BuckalDepKind>,
}

/// Serializable representation of a dependency kind with an optional platform target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckalDepKind {
    /// The dependency kind (normal, dev/build).
    pub kind: DependencyKind,
    /// Platform constraint (e.g. `cfg(target_os = "linux")`), if any.
    pub target: Option<cargo_platform::Platform>,
}

/// Serializable representation of a Cargo target (lib/bin/test/build-script).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckalTarget {
    pub name: String,
    /// e.g. `[TargetKind::Lib]`, `[TargetKind::Bin]`, `[TargetKind::ProcMacro]`, `[TargetKind::CustomBuild]`
    pub kind: Vec<TargetKind>,
    pub src_path: Utf8PathBuf,
    /// Whether doc-tests are enabled (used by lib targets).
    pub doctest: bool,
    /// Whether tests are enabled for this target.
    pub test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckalNode {
    pub package_id: PackageId,
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
    pub kind: NodeKind,
    pub edition: Edition,
    /// Full dependency edges with kind/platform info (replaces the old `dep_ids`).
    pub deps: Vec<BuckalDep>,
    // -- Fields from Package --
    pub manifest_path: Utf8PathBuf,
    pub targets: Vec<BuckalTarget>,
    /// `None` for local (first-party) packages; `Some(repr)` for registry/git sources.
    pub source: Option<String>,
    /// The `links` manifest key, if any.
    pub links: Option<String>,
    /// Cargo.lock checksum for this package, if available.
    pub checksum: Option<String>,
}

impl BuckalHash for BuckalNode {
    fn fingerprint(&self) -> Fingerprint {
        let encoded = bincode::serde::encode_to_vec(self, bincode::config::standard())
            .expect("Serialization failed");
        Fingerprint::new(blake3::hash(&encoded).into())
    }
}

/// Returns `true` if `kind` is a library-like target kind
/// (lib, cdylib, dylib, rlib, staticlib, or proc-macro).
pub fn is_lib_like(kind: &TargetKind) -> bool {
    matches!(
        kind,
        TargetKind::Lib
            | TargetKind::CDyLib
            | TargetKind::DyLib
            | TargetKind::RLib
            | TargetKind::StaticLib
            | TargetKind::ProcMacro
    )
}

pub struct BuckalResolve {
    pub dag: Dag<BuckalNode, (), u32>,
    pub index_map: HashMap<PackageId, NodeIndex<u32>>,
}

impl BuckalResolve {
    /// O(1) lookup of a node by its `PackageId`.
    pub fn get(&self, pkg_id: &PackageId) -> Option<&BuckalNode> {
        self.index_map.get(pkg_id).map(|&idx| &self.dag[idx])
    }

    /// Build a DAG from raw cargo metadata maps. `root_path` is used to compute
    /// relative paths for first-party packages (typically the buck2 root or workspace root).
    pub fn from_metadata(
        nodes_map: &HashMap<PackageId, cargo_metadata::Node>,
        packages_map: &HashMap<PackageId, cargo_metadata::Package>,
        checksums_map: &HashMap<String, String>,
        root_path: &std::path::Path,
    ) -> Self {
        let mut dag = Dag::<BuckalNode, (), u32>::new();
        let mut index_map = HashMap::new();

        // Create nodes
        for (pkg_id, node) in nodes_map {
            let package = packages_map.get(pkg_id).expect("package not found");

            let kind = if package.source.is_none() {
                // Local path dep — only first-party if under root_path
                let manifest_path = PathBuf::from(package.manifest_path.as_str());
                let manifest_dir = manifest_path
                    .parent()
                    .expect("manifest_path should have a parent");
                if let Ok(relative) = manifest_dir.strip_prefix(root_path) {
                    let relative_path = relative.to_string_lossy().replace('\\', "/");
                    NodeKind::FirstParty { relative_path }
                } else {
                    // Path dep outside workspace root — treat as third-party
                    NodeKind::ThirdParty
                }
            } else {
                NodeKind::ThirdParty
            };

            let deps: Vec<BuckalDep> = node
                .deps
                .iter()
                .map(|d| BuckalDep {
                    pkg: d.pkg.clone(),
                    name: d.name.clone(),
                    dep_kinds: d
                        .dep_kinds
                        .iter()
                        .map(|dk| BuckalDepKind {
                            kind: dk.kind,
                            target: dk.target.clone(),
                        })
                        .collect(),
                })
                .collect();

            let targets: Vec<BuckalTarget> = package
                .targets
                .iter()
                .map(|t| BuckalTarget {
                    name: t.name.clone(),
                    kind: t.kind.clone(),
                    src_path: t.src_path.clone(),
                    doctest: t.doctest,
                    test: t.test,
                })
                .collect();

            let checksum_key = format!("{}-{}", package.name, package.version);

            let buckal_node = BuckalNode {
                package_id: pkg_id.clone(),
                name: package.name.to_string(),
                version: package.version.to_string(),
                features: node.features.iter().map(|f| f.to_string()).collect(),
                kind,
                edition: package.edition,
                deps,
                manifest_path: package.manifest_path.clone(),
                targets,
                source: package.source.as_ref().map(|s| s.repr.clone()),
                links: package.links.clone(),
                checksum: checksums_map.get(&checksum_key).cloned(),
            };

            let idx = dag.add_node(buckal_node);
            index_map.insert(pkg_id.clone(), idx);
        }

        // Create edges
        for (pkg_id, node) in nodes_map {
            if let Some(&parent_idx) = index_map.get(pkg_id) {
                for dep in &node.deps {
                    if let Some(&child_idx) = index_map.get(&dep.pkg)
                        && dag.add_edge(parent_idx, child_idx, ()).is_err()
                    {
                        log::warn!(
                            "Detected cycle when adding edge from {} to {:?} — skipping",
                            pkg_id.repr,
                            dep.pkg.repr
                        );
                    }
                }
            }
        }

        Self { dag, index_map }
    }

    pub fn dependents(&self, pkg_id: &PackageId) -> Vec<&BuckalNode> {
        let Some(&idx) = self.index_map.get(pkg_id) else {
            return Vec::new();
        };
        self.dag
            .parents(idx)
            .iter(&self.dag)
            .map(|(_edge, node_idx)| &self.dag[node_idx])
            .collect()
    }

    pub fn dependencies(&self, pkg_id: &PackageId) -> Vec<&BuckalNode> {
        let Some(&idx) = self.index_map.get(pkg_id) else {
            return Vec::new();
        };
        self.dag
            .children(idx)
            .iter(&self.dag)
            .map(|(_edge, node_idx)| &self.dag[node_idx])
            .collect()
    }

    pub fn find_by_name(&self, name: &str, version: Option<&str>) -> Option<&BuckalNode> {
        self.dag
            .raw_nodes()
            .iter()
            .map(|n| &n.weight)
            .find(|node| node.name == name && version.is_none_or(|v| node.version == v))
    }

    pub fn nodes(&self) -> impl Iterator<Item = &BuckalNode> {
        self.dag.raw_nodes().iter().map(|n| &n.weight)
    }
}
