use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::bail;
use cargo_metadata::{DependencyKind, Edition, PackageId, TargetKind, camino::Utf8PathBuf};
use daggy::{Dag, NodeIndex, Walker};
use serde::{Deserialize, Serialize};

use crate::cache::{Fingerprint, PackageIdExt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    FirstParty { relative_path: String },
    ThirdParty,
}

/// A single dependency edge with platform/kind metadata.
///
/// Used as the graph edge weight. The target node identity comes from the
/// edge's target, so no `PackageId` is stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckalDep {
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
    pub dag: Dag<BuckalNode, BuckalDep, u32>,
    pub index_map: HashMap<PackageId, NodeIndex<u32>>,
}

impl BuckalResolve {
    /// O(1) lookup of a node by its `PackageId`.
    pub fn get(&self, pkg_id: &PackageId) -> Option<&BuckalNode> {
        self.index_map.get(pkg_id).map(|&idx| &self.dag[idx])
    }

    /// Build a dependency DAG from raw cargo metadata maps. `root_path` is used to compute
    /// relative paths for first-party packages (typically the buck2 root or workspace root).
    ///
    /// Uses `daggy::Dag` which enforces acyclicity. Edges are inserted in two passes:
    /// first normal/build deps, then dev deps. Returns an error if any edge would form
    /// a cycle — Buck2 requires a strictly acyclic dependency graph.
    ///
    /// When `lenient` is `true`, dev-dependency cycles are silently skipped instead of
    /// causing a hard error. This is used by the cache fallback path where having all
    /// nodes (for removal detection) matters more than strict edge accuracy.
    pub fn from_metadata(
        nodes_map: &HashMap<PackageId, cargo_metadata::Node>,
        packages_map: &HashMap<PackageId, cargo_metadata::Package>,
        checksums_map: &HashMap<String, String>,
        root_path: &std::path::Path,
        lenient: bool,
    ) -> anyhow::Result<Self> {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
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
                manifest_path: package.manifest_path.clone(),
                targets,
                source: package.source.as_ref().map(|s| s.repr.clone()),
                links: package.links.clone(),
                checksum: checksums_map.get(&checksum_key).cloned(),
            };

            let idx = dag.add_node(buckal_node);
            index_map.insert(pkg_id.clone(), idx);
        }

        // Pass 1: Normal and Build deps (non-dev)
        for (pkg_id, node) in nodes_map {
            if let Some(&parent_idx) = index_map.get(pkg_id) {
                for dep in &node.deps {
                    if let Some(&child_idx) = index_map.get(&dep.pkg) {
                        let non_dev_kinds: Vec<_> = dep
                            .dep_kinds
                            .iter()
                            .filter(|dk| dk.kind != DependencyKind::Development)
                            .map(|dk| BuckalDepKind {
                                kind: dk.kind,
                                target: dk.target.clone(),
                            })
                            .collect();
                        if non_dev_kinds.is_empty() {
                            continue;
                        }
                        let buckal_dep = BuckalDep {
                            name: dep.name.clone(),
                            dep_kinds: non_dev_kinds,
                        };
                        if dag.add_edge(parent_idx, child_idx, buckal_dep).is_err() {
                            let parent_name = &dag[parent_idx].name;
                            let child_name = &dag[child_idx].name;
                            bail!(
                                "dependency cycle detected: {parent_name} -> {child_name}\n\
                                 \n\
                                 cargo-buckal requires a strictly acyclic dependency graph to \
                                 generate BUCK files, but a cycle was found among normal/build \
                                 dependencies.\n\
                                 \n\
                                 This is unexpected because Cargo itself rejects cycles in normal \
                                 dependencies. Please check your Cargo.toml files for circular \
                                 [dependencies] or [build-dependencies] entries."
                            );
                        }
                    }
                }
            }
        }

        // Pass 2: Dev deps only
        for (pkg_id, node) in nodes_map {
            if let Some(&parent_idx) = index_map.get(pkg_id) {
                for dep in &node.deps {
                    if let Some(&child_idx) = index_map.get(&dep.pkg) {
                        let dev_kinds: Vec<_> = dep
                            .dep_kinds
                            .iter()
                            .filter(|dk| dk.kind == DependencyKind::Development)
                            .map(|dk| BuckalDepKind {
                                kind: dk.kind,
                                target: dk.target.clone(),
                            })
                            .collect();
                        if dev_kinds.is_empty() {
                            continue;
                        }
                        // If a non-dev edge already exists (from Pass 1) with the
                        // same name, merge dev dep_kinds into it to avoid duplicate
                        // edges. If the name differs (renamed dev-dep alias), fall
                        // through to add a separate edge so the alias is preserved.
                        if let Some(edge_idx) = dag.find_edge(parent_idx, child_idx)
                            && dag[edge_idx].name == dep.name
                        {
                            dag[edge_idx].dep_kinds.extend(dev_kinds);
                            continue;
                        }
                        let buckal_dep = BuckalDep {
                            name: dep.name.clone(),
                            dep_kinds: dev_kinds,
                        };
                        // NOTE: We are intentionally stricter than Cargo here. Cargo
                        // allows dev-dependency cycles, and Buck2 *could* represent
                        // them acyclically because dev deps only appear on rust_test
                        // targets (see dep_kind_matches in buckify/deps.rs and
                        // buckify_root_node in buckify/rules.rs). If we want to relax
                        // this in the future: (1) skip cycle detection for dev-only
                        // edges here, and (2) ensure downstream consumers correctly
                        // scope dev edges to test targets only.
                        // See: https://github.com/buck2hub/cargo-buckal/pull/75#discussion_r2975238783
                        if dag.add_edge(parent_idx, child_idx, buckal_dep).is_err() {
                            let parent_name = &dag[parent_idx].name;
                            let child_name = &dag[child_idx].name;
                            if lenient {
                                // In lenient mode (cache fallback), skip the cyclic
                                // dev edge so we still have all nodes for diffing.
                                continue;
                            }
                            bail!(
                                "dev-dependency cycle detected: {parent_name} -> {child_name}\n\
                                 \n\
                                 cargo-buckal requires a strictly acyclic dependency graph to \
                                 generate BUCK files. Cargo allows dev-dependency cycles \
                                 (https://doc.rust-lang.org/cargo/reference/resolver.html\
                                 #dev-dependency-cycles), but Buck2 does not.\n\
                                 \n\
                                 To fix this, restructure your workspace so that the \
                                 dev-dependency from `{parent_name}` to `{child_name}` does \
                                 not form a cycle. Common approaches:\n  \
                                 - Extract shared test utilities into a separate crate\n  \
                                 - Move integration tests into a dedicated test crate"
                            );
                        }
                    }
                }
            }
        }

        Ok(Self { dag, index_map })
    }

    pub fn dependents(&self, pkg_id: &PackageId) -> Vec<&BuckalNode> {
        let Some(&idx) = self.index_map.get(pkg_id) else {
            return Vec::new();
        };
        self.dag
            .parents(idx)
            .iter(&self.dag)
            .map(|(_, node_idx)| &self.dag[node_idx])
            .collect()
    }

    pub fn dependencies(&self, pkg_id: &PackageId) -> Vec<&BuckalNode> {
        let Some(&idx) = self.index_map.get(pkg_id) else {
            return Vec::new();
        };
        self.dag
            .children(idx)
            .iter(&self.dag)
            .map(|(_, node_idx)| &self.dag[node_idx])
            .collect()
    }

    /// Find a node by crate name and optional version. When `version` is `None`
    /// and multiple versions exist, returns the highest semver version for
    /// determinism (node insertion order depends on `HashMap` iteration).
    pub fn find_by_name(&self, name: &str, version: Option<&str>) -> Option<&BuckalNode> {
        let mut matches = self
            .dag
            .raw_nodes()
            .iter()
            .map(|n| &n.weight)
            .filter(|node| node.name == name && version.is_none_or(|v| node.version == v));

        if version.is_some() {
            return matches.next();
        }

        matches.max_by(|a, b| {
            let va = semver::Version::parse(&a.version).ok();
            let vb = semver::Version::parse(&b.version).ok();
            va.cmp(&vb)
        })
    }

    pub fn nodes(&self) -> impl Iterator<Item = &BuckalNode> {
        self.dag.raw_nodes().iter().map(|n| &n.weight)
    }

    /// Returns `(edge_weight, child_node)` pairs for all outgoing edges of the given node.
    /// This is the primary way to iterate over a node's dependency edges with metadata.
    pub fn deps_of(&self, pkg_id: &PackageId) -> Vec<(&BuckalDep, &BuckalNode)> {
        let Some(&idx) = self.index_map.get(pkg_id) else {
            return Vec::new();
        };
        self.dag
            .children(idx)
            .iter(&self.dag)
            .map(|(edge_idx, node_idx)| (&self.dag[edge_idx], &self.dag[node_idx]))
            .collect()
    }

    /// Compute a deterministic, location-independent fingerprint for the given package.
    ///
    /// The fingerprint covers the node's intrinsic properties (excluding absolute paths
    /// like `manifest_path` and `targets[*].src_path`) plus all outgoing dependency edges
    /// (sorted by canonicalized child `PackageId` for determinism).
    ///
    /// `workspace_root` is used to canonicalize `PackageId` values for path dependencies
    /// so that the same project checked out at different locations produces identical
    /// fingerprints.
    pub fn fingerprint_of(&self, pkg_id: &PackageId, workspace_root: &Utf8PathBuf) -> Fingerprint {
        let idx = self.index_map[pkg_id];
        let node = &self.dag[idx];
        let mut hasher = blake3::Hasher::new();

        // 1. Package identity — canonicalize to remove absolute workspace path
        let canonical_id = node.package_id.canonicalize(workspace_root);
        hasher.update(canonical_id.repr.as_bytes());

        // 2. Stable scalar fields
        hasher.update(node.name.as_bytes());
        hasher.update(node.version.as_bytes());
        hasher.update(node.edition.to_string().as_bytes());

        // 3. Features (sorted — cargo metadata doesn't guarantee order)
        let mut sorted_features = node.features.clone();
        sorted_features.sort();
        for f in &sorted_features {
            hasher.update(f.as_bytes());
        }

        // 4. NodeKind (FirstParty already stores relative_path)
        match &node.kind {
            NodeKind::FirstParty { relative_path } => {
                hasher.update(b"first_party");
                hasher.update(relative_path.as_bytes());
            }
            NodeKind::ThirdParty => {
                hasher.update(b"third_party");
            }
        }

        // 5. Targets — hash name, kind, relative src_path, doctest, test
        //    Skip absolute src_path; relativize against manifest_dir
        let manifest_dir = node.manifest_path.parent().unwrap_or(&node.manifest_path);
        for target in &node.targets {
            hasher.update(target.name.as_bytes());
            let kind_encoded =
                bincode::serde::encode_to_vec(&target.kind, bincode::config::standard())
                    .expect("Serialization failed");
            hasher.update(&kind_encoded);
            let rel_src = target
                .src_path
                .strip_prefix(manifest_dir)
                .unwrap_or(&target.src_path);
            hasher.update(rel_src.as_str().as_bytes());
            hasher.update(&[target.doctest as u8]);
            hasher.update(&[target.test as u8]);
        }

        // 6. Optional fields with discriminant bytes to prevent boundary collisions
        match &node.source {
            Some(src) => {
                hasher.update(&[1u8]);
                hasher.update(src.as_bytes());
            }
            None => {
                hasher.update(&[0u8]);
            }
        }
        match &node.links {
            Some(links) => {
                hasher.update(&[1u8]);
                hasher.update(links.as_bytes());
            }
            None => {
                hasher.update(&[0u8]);
            }
        }
        match &node.checksum {
            Some(checksum) => {
                hasher.update(&[1u8]);
                hasher.update(checksum.as_bytes());
            }
            None => {
                hasher.update(&[0u8]);
            }
        }

        // 7. manifest_path is intentionally skipped — identity is captured by
        //    kind.relative_path (first-party) or name+version+checksum (third-party)

        // 8. Outgoing edges — canonicalize child PackageId.repr
        let mut children: Vec<_> = self
            .dag
            .children(idx)
            .iter(&self.dag)
            .map(|(edge_idx, node_idx)| {
                let child_canonical = self.dag[node_idx].package_id.canonicalize(workspace_root);
                (child_canonical, edge_idx)
            })
            .collect();
        children.sort_by(|(a, _), (b, _)| a.repr.cmp(&b.repr));

        for (child_canonical_id, edge_idx) in &children {
            let dep_encoded =
                bincode::serde::encode_to_vec(&self.dag[*edge_idx], bincode::config::standard())
                    .expect("Serialization failed");
            hasher.update(&dep_encoded);
            hasher.update(child_canonical_id.repr.as_bytes());
        }

        Fingerprint::new(hasher.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace_root() -> Utf8PathBuf {
        Utf8PathBuf::from("/tmp")
    }

    fn make_pkg_id(name: &str) -> PackageId {
        PackageId {
            repr: format!(
                "registry+https://github.com/rust-lang/crates.io-index#{}@1.0.0",
                name
            ),
        }
    }

    fn make_pkg_id_versioned(name: &str, version: &str) -> PackageId {
        PackageId {
            repr: format!(
                "registry+https://github.com/rust-lang/crates.io-index#{}@{}",
                name, version
            ),
        }
    }

    fn make_node(name: &str, version: &str) -> BuckalNode {
        BuckalNode {
            package_id: make_pkg_id(name),
            name: name.to_string(),
            version: version.to_string(),
            features: vec![],
            kind: NodeKind::ThirdParty,
            edition: Edition::E2021,
            manifest_path: Utf8PathBuf::from(format!("/tmp/{}/Cargo.toml", name)),
            targets: vec![],
            source: Some("registry+https://github.com/rust-lang/crates.io-index".to_string()),
            links: None,
            checksum: None,
        }
    }

    fn make_dep(name: &str) -> BuckalDep {
        BuckalDep {
            name: name.to_string(),
            dep_kinds: vec![BuckalDepKind {
                kind: DependencyKind::Normal,
                target: None,
            }],
        }
    }

    #[test]
    fn test_three_node_chain() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("a", "1.0.0");
        let node_b = make_node("b", "1.0.0");
        let node_c = make_node("c", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_b = dag.add_node(node_b.clone());
        let idx_c = dag.add_node(node_c.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_b.package_id.clone(), idx_b);
        index_map.insert(node_c.package_id.clone(), idx_c);

        dag.add_edge(idx_a, idx_b, make_dep("b")).unwrap();
        dag.add_edge(idx_b, idx_c, make_dep("c")).unwrap();

        let resolve = BuckalResolve { dag, index_map };

        // B's dependents should be [A]
        let b_dependents = resolve.dependents(&make_pkg_id("b"));
        assert_eq!(b_dependents.len(), 1);
        assert_eq!(b_dependents[0].name, "a");

        // A's dependencies should be [B]
        let a_deps = resolve.dependencies(&make_pkg_id("a"));
        assert_eq!(a_deps.len(), 1);
        assert_eq!(a_deps[0].name, "b");

        // C has no dependents besides B
        let c_dependents = resolve.dependents(&make_pkg_id("c"));
        assert_eq!(c_dependents.len(), 1);
        assert_eq!(c_dependents[0].name, "b");
    }

    #[test]
    fn test_first_party_relative_path() {
        let mut node = make_node("my-crate", "0.1.0");
        node.package_id = make_pkg_id("my-crate");
        node.kind = NodeKind::FirstParty {
            relative_path: "crates/my-crate".to_string(),
        };
        node.source = None;

        match &node.kind {
            NodeKind::FirstParty { relative_path } => {
                assert_eq!(relative_path, "crates/my-crate");
            }
            _ => panic!("expected FirstParty"),
        }
    }

    #[test]
    fn test_find_by_name() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("serde", "1.0.0");
        let node_b = make_node("tokio", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_b = dag.add_node(node_b.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_b.package_id.clone(), idx_b);

        let resolve = BuckalResolve { dag, index_map };

        assert!(resolve.find_by_name("serde", None).is_some());
        assert!(resolve.find_by_name("serde", Some("1.0.0")).is_some());
        assert!(resolve.find_by_name("serde", Some("2.0.0")).is_none());
        assert!(resolve.find_by_name("nonexistent", None).is_none());
    }

    #[test]
    fn test_fingerprint_stability_and_sensitivity() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node1 = make_node("foo", "1.0.0");
        let node2 = make_node("foo", "1.0.0");
        let node3 = make_node("foo", "1.1.0");

        let idx1 = dag.add_node(node1.clone());
        index_map.insert(node1.package_id.clone(), idx1);
        let resolve1 = BuckalResolve {
            dag,
            index_map: index_map.clone(),
        };

        let mut dag2 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map2 = HashMap::new();
        let idx2 = dag2.add_node(node2.clone());
        index_map2.insert(node2.package_id.clone(), idx2);
        let resolve2 = BuckalResolve {
            dag: dag2,
            index_map: index_map2,
        };

        let mut dag3 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map3 = HashMap::new();
        let idx3 = dag3.add_node(node3.clone());
        index_map3.insert(node3.package_id.clone(), idx3);
        let resolve3 = BuckalResolve {
            dag: dag3,
            index_map: index_map3,
        };

        // Same data -> same fingerprint
        assert_eq!(
            resolve1.fingerprint_of(&node1.package_id, &test_workspace_root()),
            resolve2.fingerprint_of(&node2.package_id, &test_workspace_root())
        );

        // Different version -> different fingerprint
        assert_ne!(
            resolve1.fingerprint_of(&node1.package_id, &test_workspace_root()),
            resolve3.fingerprint_of(&node3.package_id, &test_workspace_root())
        );
    }

    /// Diamond dependency with version conflict:
    ///
    ///     root
    ///    /    \
    /// dep_a  dep_b
    ///    \    /
    ///   common  (v1.0.0 via dep_a, v2.0.0 via dep_b)
    ///
    /// Both versions of `common` must exist as separate nodes in the graph.
    #[test]
    fn test_diamond_dependency_version_conflict() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let common_v1_id = make_pkg_id_versioned("common", "1.0.0");
        let common_v2_id = make_pkg_id_versioned("common", "2.0.0");

        let root = {
            let mut n = make_node("root", "0.1.0");
            n.kind = NodeKind::FirstParty {
                relative_path: "".to_string(),
            };
            n.source = None;
            n
        };

        let dep_a = make_node("dep-a", "1.0.0");
        let dep_b = make_node("dep-b", "1.0.0");

        let common_v1 = {
            let mut n = make_node("common", "1.0.0");
            n.package_id = common_v1_id.clone();
            n
        };

        let common_v2 = {
            let mut n = make_node("common", "2.0.0");
            n.package_id = common_v2_id.clone();
            n
        };

        let idx_root = dag.add_node(root.clone());
        let idx_a = dag.add_node(dep_a.clone());
        let idx_b = dag.add_node(dep_b.clone());
        let idx_cv1 = dag.add_node(common_v1.clone());
        let idx_cv2 = dag.add_node(common_v2.clone());

        index_map.insert(root.package_id.clone(), idx_root);
        index_map.insert(dep_a.package_id.clone(), idx_a);
        index_map.insert(dep_b.package_id.clone(), idx_b);
        index_map.insert(common_v1_id.clone(), idx_cv1);
        index_map.insert(common_v2_id.clone(), idx_cv2);

        dag.add_edge(idx_root, idx_a, make_dep("dep-a")).unwrap();
        dag.add_edge(idx_root, idx_b, make_dep("dep-b")).unwrap();
        dag.add_edge(idx_a, idx_cv1, make_dep("common")).unwrap();
        dag.add_edge(idx_b, idx_cv2, make_dep("common")).unwrap();

        let resolve = BuckalResolve { dag, index_map };

        // Total: root + dep_a + dep_b + common@1.0 + common@2.0 = 5 nodes
        assert_eq!(resolve.nodes().count(), 5);

        // Both versions of common exist as separate nodes
        assert!(resolve.find_by_name("common", Some("1.0.0")).is_some());
        assert!(resolve.find_by_name("common", Some("2.0.0")).is_some());

        // find_by_name without version returns one (non-deterministic which)
        assert!(resolve.find_by_name("common", None).is_some());

        // Count nodes named "common" — should be exactly 2
        let common_nodes: Vec<&BuckalNode> =
            resolve.nodes().filter(|n| n.name == "common").collect();
        assert_eq!(
            common_nodes.len(),
            2,
            "expected 2 nodes named 'common', got {}",
            common_nodes.len()
        );

        // dep_a depends on common@1.0.0 only
        let a_deps = resolve.dependencies(&make_pkg_id("dep-a"));
        assert_eq!(a_deps.len(), 1);
        assert_eq!(a_deps[0].name, "common");
        assert_eq!(a_deps[0].version, "1.0.0");

        // dep_b depends on common@2.0.0 only
        let b_deps = resolve.dependencies(&make_pkg_id("dep-b"));
        assert_eq!(b_deps.len(), 1);
        assert_eq!(b_deps[0].name, "common");
        assert_eq!(b_deps[0].version, "2.0.0");

        // common@1.0.0 dependents should be [dep_a] only
        let cv1_dependents = resolve.dependents(&common_v1_id);
        assert_eq!(cv1_dependents.len(), 1);
        assert_eq!(cv1_dependents[0].name, "dep-a");

        // common@2.0.0 dependents should be [dep_b] only
        let cv2_dependents = resolve.dependents(&common_v2_id);
        assert_eq!(cv2_dependents.len(), 1);
        assert_eq!(cv2_dependents[0].name, "dep-b");

        // root depends on both dep_a and dep_b
        let root_deps = resolve.dependencies(&make_pkg_id("root"));
        assert_eq!(root_deps.len(), 2);
        let root_dep_names: Vec<&str> = root_deps.iter().map(|n| n.name.as_str()).collect();
        assert!(root_dep_names.contains(&"dep-a"));
        assert!(root_dep_names.contains(&"dep-b"));

        // Fingerprints of common@1.0.0 and common@2.0.0 must differ
        assert_ne!(
            resolve.fingerprint_of(&common_v1_id, &test_workspace_root()),
            resolve.fingerprint_of(&common_v2_id, &test_workspace_root()),
            "different versions should produce different fingerprints"
        );
    }

    fn make_dep_with_kind(name: &str, kind: DependencyKind) -> BuckalDep {
        BuckalDep {
            name: name.to_string(),
            dep_kinds: vec![BuckalDepKind { kind, target: None }],
        }
    }

    #[test]
    fn test_deps_of() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("a", "1.0.0");
        let node_b = make_node("b", "1.0.0");
        let node_c = make_node("c", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_b = dag.add_node(node_b.clone());
        let idx_c = dag.add_node(node_c.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_b.package_id.clone(), idx_b);
        index_map.insert(node_c.package_id.clone(), idx_c);

        dag.add_edge(idx_a, idx_b, make_dep("b")).unwrap();
        dag.add_edge(idx_b, idx_c, make_dep("c")).unwrap();

        let resolve = BuckalResolve { dag, index_map };

        // A → B: one pair with dep name "b" pointing to node "b"
        let a_deps = resolve.deps_of(&make_pkg_id("a"));
        assert_eq!(a_deps.len(), 1);
        assert_eq!(a_deps[0].0.name, "b");
        assert_eq!(a_deps[0].1.name, "b");

        // B → C: one pair with dep name "c" pointing to node "c"
        let b_deps = resolve.deps_of(&make_pkg_id("b"));
        assert_eq!(b_deps.len(), 1);
        assert_eq!(b_deps[0].0.name, "c");
        assert_eq!(b_deps[0].1.name, "c");

        // C is a leaf — no deps
        let c_deps = resolve.deps_of(&make_pkg_id("c"));
        assert!(c_deps.is_empty());

        // Unknown package — empty
        let unknown_deps = resolve.deps_of(&make_pkg_id("unknown"));
        assert!(unknown_deps.is_empty());
    }

    #[test]
    fn test_deps_of_edge_metadata() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("a", "1.0.0");
        let node_b = make_node("b", "1.0.0");
        let node_c = make_node("c", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_b = dag.add_node(node_b.clone());
        let idx_c = dag.add_node(node_c.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_b.package_id.clone(), idx_b);
        index_map.insert(node_c.package_id.clone(), idx_c);

        dag.add_edge(
            idx_a,
            idx_b,
            make_dep_with_kind("b", DependencyKind::Normal),
        )
        .unwrap();
        dag.add_edge(
            idx_a,
            idx_c,
            make_dep_with_kind("c", DependencyKind::Development),
        )
        .unwrap();

        let resolve = BuckalResolve { dag, index_map };

        let a_deps = resolve.deps_of(&make_pkg_id("a"));
        assert_eq!(a_deps.len(), 2);

        let dep_names: Vec<&str> = a_deps.iter().map(|(dep, _)| dep.name.as_str()).collect();
        assert!(dep_names.contains(&"b"));
        assert!(dep_names.contains(&"c"));

        // Verify each edge carries the correct DependencyKind
        for (dep, node) in &a_deps {
            assert_eq!(dep.dep_kinds.len(), 1);
            match node.name.as_str() {
                "b" => assert!(matches!(dep.dep_kinds[0].kind, DependencyKind::Normal)),
                "c" => {
                    assert!(matches!(dep.dep_kinds[0].kind, DependencyKind::Development))
                }
                other => panic!("unexpected dep node: {}", other),
            }
        }
    }

    #[test]
    fn test_fingerprint_sensitive_to_edges() {
        // resolve1: node A with no deps
        let mut dag1 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map1 = HashMap::new();
        let node_a1 = make_node("a", "1.0.0");
        let idx_a1 = dag1.add_node(node_a1.clone());
        index_map1.insert(node_a1.package_id.clone(), idx_a1);
        let resolve1 = BuckalResolve {
            dag: dag1,
            index_map: index_map1,
        };

        // resolve2: same node A with an edge to B (dep name "b")
        let mut dag2 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map2 = HashMap::new();
        let node_a2 = make_node("a", "1.0.0");
        let node_b2 = make_node("b", "1.0.0");
        let idx_a2 = dag2.add_node(node_a2.clone());
        let idx_b2 = dag2.add_node(node_b2.clone());
        index_map2.insert(node_a2.package_id.clone(), idx_a2);
        index_map2.insert(node_b2.package_id.clone(), idx_b2);
        dag2.add_edge(idx_a2, idx_b2, make_dep("b")).unwrap();
        let resolve2 = BuckalResolve {
            dag: dag2,
            index_map: index_map2,
        };

        // resolve3: same node A with edge to B but dep name "b-renamed"
        let mut dag3 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map3 = HashMap::new();
        let node_a3 = make_node("a", "1.0.0");
        let node_b3 = make_node("b", "1.0.0");
        let idx_a3 = dag3.add_node(node_a3.clone());
        let idx_b3 = dag3.add_node(node_b3.clone());
        index_map3.insert(node_a3.package_id.clone(), idx_a3);
        index_map3.insert(node_b3.package_id.clone(), idx_b3);
        dag3.add_edge(idx_a3, idx_b3, make_dep("b-renamed"))
            .unwrap();
        let resolve3 = BuckalResolve {
            dag: dag3,
            index_map: index_map3,
        };

        let fp1 = resolve1.fingerprint_of(&make_pkg_id("a"), &test_workspace_root());
        let fp2 = resolve2.fingerprint_of(&make_pkg_id("a"), &test_workspace_root());
        let fp3 = resolve3.fingerprint_of(&make_pkg_id("a"), &test_workspace_root());

        // Adding a dep changes the fingerprint
        assert_ne!(
            fp1, fp2,
            "adding a dependency edge should change the fingerprint"
        );

        // Different edge metadata (dep name) changes the fingerprint
        assert_ne!(
            fp2, fp3,
            "different edge metadata should change the fingerprint"
        );
    }

    /// When a package is both a normal and dev dependency of the same parent,
    /// `from_metadata()` should merge dep_kinds onto a single edge rather than
    /// creating parallel edges. This ensures `dependencies()` and `dependents()`
    /// return each node exactly once.
    #[test]
    fn test_normal_and_dev_dep_merged_into_single_edge() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("a", "1.0.0");
        let node_b = make_node("b", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_b = dag.add_node(node_b.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_b.package_id.clone(), idx_b);

        // Simulate what from_metadata does: add normal dep in pass 1
        dag.add_edge(
            idx_a,
            idx_b,
            make_dep_with_kind("b", DependencyKind::Normal),
        )
        .unwrap();

        // Pass 2: merge dev dep_kinds into existing edge instead of adding parallel edge
        if let Some(edge_idx) = dag.find_edge(idx_a, idx_b) {
            dag[edge_idx].dep_kinds.push(BuckalDepKind {
                kind: DependencyKind::Development,
                target: None,
            });
        }

        let resolve = BuckalResolve { dag, index_map };

        // dependencies() should return b exactly once (not duplicated)
        let a_deps = resolve.dependencies(&make_pkg_id("a"));
        assert_eq!(
            a_deps.len(),
            1,
            "expected 1 dependency, got {}",
            a_deps.len()
        );
        assert_eq!(a_deps[0].name, "b");

        // dependents() should return a exactly once
        let b_dependents = resolve.dependents(&make_pkg_id("b"));
        assert_eq!(
            b_dependents.len(),
            1,
            "expected 1 dependent, got {}",
            b_dependents.len()
        );
        assert_eq!(b_dependents[0].name, "a");

        // deps_of() should return a single edge with both dep_kinds
        let a_deps_of = resolve.deps_of(&make_pkg_id("a"));
        assert_eq!(
            a_deps_of.len(),
            1,
            "expected 1 edge, got {}",
            a_deps_of.len()
        );
        assert_eq!(a_deps_of[0].0.dep_kinds.len(), 2);
        assert!(
            a_deps_of[0]
                .0
                .dep_kinds
                .iter()
                .any(|dk| dk.kind == DependencyKind::Normal)
        );
        assert!(
            a_deps_of[0]
                .0
                .dep_kinds
                .iter()
                .any(|dk| dk.kind == DependencyKind::Development)
        );
    }

    /// When a package appears as a normal dep under one name and a dev-dep under
    /// a different (renamed) alias, Pass 2 must create a separate edge instead of
    /// merging into the existing one. This preserves the alias for downstream BUCK
    /// rule generation (resolve_dep_label uses dep.name to detect renames).
    ///
    /// Example Cargo.toml:
    ///   [dependencies]
    ///   foo = "1.0"
    ///   [dev-dependencies]
    ///   bar = { package = "foo", version = "1.0" }
    #[test]
    fn test_renamed_dev_dep_preserves_separate_edge() {
        let mut dag = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut index_map = HashMap::new();

        let node_a = make_node("a", "1.0.0");
        let node_foo = make_node("foo", "1.0.0");

        let idx_a = dag.add_node(node_a.clone());
        let idx_foo = dag.add_node(node_foo.clone());

        index_map.insert(node_a.package_id.clone(), idx_a);
        index_map.insert(node_foo.package_id.clone(), idx_foo);

        // Pass 1: normal dep named "foo"
        dag.add_edge(
            idx_a,
            idx_foo,
            make_dep_with_kind("foo", DependencyKind::Normal),
        )
        .unwrap();

        // Pass 2: dev dep with renamed alias "bar" pointing to the same package.
        // Because the name differs, this must NOT merge into the existing edge.
        if let Some(edge_idx) = dag.find_edge(idx_a, idx_foo)
            && dag[edge_idx].name == "bar"
        {
            dag[edge_idx].dep_kinds.push(BuckalDepKind {
                kind: DependencyKind::Development,
                target: None,
            });
        } else {
            // Name doesn't match — add a separate edge (what the fix does)
            dag.add_edge(
                idx_a,
                idx_foo,
                make_dep_with_kind("bar", DependencyKind::Development),
            )
            .unwrap();
        }

        let resolve = BuckalResolve { dag, index_map };

        // deps_of should return TWO edges: "foo" (Normal) and "bar" (Dev)
        let a_deps = resolve.deps_of(&make_pkg_id("a"));
        assert_eq!(a_deps.len(), 2, "expected 2 edges, got {}", a_deps.len());

        let edge_names: Vec<&str> = a_deps.iter().map(|(dep, _)| dep.name.as_str()).collect();
        assert!(
            edge_names.contains(&"foo"),
            "expected edge named 'foo', got {:?}",
            edge_names
        );
        assert!(
            edge_names.contains(&"bar"),
            "expected edge named 'bar', got {:?}",
            edge_names
        );

        // The "foo" edge should be Normal only
        let foo_edge = a_deps.iter().find(|(dep, _)| dep.name == "foo").unwrap();
        assert_eq!(foo_edge.0.dep_kinds.len(), 1);
        assert!(matches!(
            foo_edge.0.dep_kinds[0].kind,
            DependencyKind::Normal
        ));

        // The "bar" edge should be Dev only
        let bar_edge = a_deps.iter().find(|(dep, _)| dep.name == "bar").unwrap();
        assert_eq!(bar_edge.0.dep_kinds.len(), 1);
        assert!(matches!(
            bar_edge.0.dep_kinds[0].kind,
            DependencyKind::Development
        ));

        // Both edges point to the same node
        assert_eq!(foo_edge.1.name, "foo");
        assert_eq!(bar_edge.1.name, "foo");
    }

    /// First-party nodes at different checkout locations should produce identical fingerprints.
    #[test]
    fn test_fingerprint_portable_across_paths() {
        // Node checked out at /home/alice/project
        let alice_root = Utf8PathBuf::from("/home/alice/project");
        let mut node_alice = BuckalNode {
            package_id: PackageId {
                repr: "path+file:///home/alice/project#foo@1.0.0".to_string(),
            },
            name: "foo".to_string(),
            version: "1.0.0".to_string(),
            features: vec![],
            kind: NodeKind::FirstParty {
                relative_path: "".to_string(),
            },
            edition: Edition::E2021,
            manifest_path: Utf8PathBuf::from("/home/alice/project/Cargo.toml"),
            targets: vec![BuckalTarget {
                name: "foo".to_string(),
                kind: vec![TargetKind::Lib],
                src_path: Utf8PathBuf::from("/home/alice/project/src/lib.rs"),
                doctest: true,
                test: true,
            }],
            source: None,
            links: None,
            checksum: None,
        };

        let mut dag1 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut map1 = HashMap::new();
        let idx1 = dag1.add_node(node_alice.clone());
        map1.insert(node_alice.package_id.clone(), idx1);
        let resolve1 = BuckalResolve {
            dag: dag1,
            index_map: map1,
        };

        // Same node checked out at /home/bob/work/project
        let bob_root = Utf8PathBuf::from("/home/bob/work/project");
        node_alice.package_id = PackageId {
            repr: "path+file:///home/bob/work/project#foo@1.0.0".to_string(),
        };
        node_alice.manifest_path = Utf8PathBuf::from("/home/bob/work/project/Cargo.toml");
        node_alice.targets[0].src_path = Utf8PathBuf::from("/home/bob/work/project/src/lib.rs");

        let mut dag2 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut map2 = HashMap::new();
        let idx2 = dag2.add_node(node_alice.clone());
        map2.insert(node_alice.package_id.clone(), idx2);
        let resolve2 = BuckalResolve {
            dag: dag2,
            index_map: map2,
        };

        let fp_alice = resolve1.fingerprint_of(
            &PackageId {
                repr: "path+file:///home/alice/project#foo@1.0.0".to_string(),
            },
            &alice_root,
        );
        let fp_bob = resolve2.fingerprint_of(&node_alice.package_id, &bob_root);

        assert_eq!(
            fp_alice, fp_bob,
            "fingerprints should be portable across checkout locations"
        );
    }

    /// Third-party nodes with different ~/.cargo registry paths should produce identical fingerprints.
    #[test]
    fn test_fingerprint_portable_third_party() {
        let make_third_party = |cargo_home: &str| -> (BuckalNode, Utf8PathBuf) {
            let node = BuckalNode {
                package_id: PackageId {
                    repr: "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.200"
                        .to_string(),
                },
                name: "serde".to_string(),
                version: "1.0.200".to_string(),
                features: vec!["derive".to_string(), "std".to_string()],
                kind: NodeKind::ThirdParty,
                edition: Edition::E2021,
                manifest_path: Utf8PathBuf::from(format!(
                    "{}/.cargo/registry/src/index.crates.io/serde-1.0.200/Cargo.toml",
                    cargo_home
                )),
                targets: vec![BuckalTarget {
                    name: "serde".to_string(),
                    kind: vec![TargetKind::Lib],
                    src_path: Utf8PathBuf::from(format!(
                        "{}/.cargo/registry/src/index.crates.io/serde-1.0.200/src/lib.rs",
                        cargo_home
                    )),
                    doctest: true,
                    test: true,
                }],
                source: Some("registry+https://github.com/rust-lang/crates.io-index".to_string()),
                links: None,
                checksum: Some("abc123".to_string()),
            };
            (node, Utf8PathBuf::from(format!("{}/project", cargo_home)))
        };

        let (node1, root1) = make_third_party("/home/alice");
        let (node2, root2) = make_third_party("/home/bob");

        let mut dag1 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut map1 = HashMap::new();
        let idx1 = dag1.add_node(node1.clone());
        map1.insert(node1.package_id.clone(), idx1);
        let resolve1 = BuckalResolve {
            dag: dag1,
            index_map: map1,
        };

        let mut dag2 = Dag::<BuckalNode, BuckalDep, u32>::new();
        let mut map2 = HashMap::new();
        let idx2 = dag2.add_node(node2.clone());
        map2.insert(node2.package_id.clone(), idx2);
        let resolve2 = BuckalResolve {
            dag: dag2,
            index_map: map2,
        };

        let fp1 = resolve1.fingerprint_of(&node1.package_id, &root1);
        let fp2 = resolve2.fingerprint_of(&node2.package_id, &root2);

        assert_eq!(
            fp1, fp2,
            "third-party fingerprints should be portable across different cargo homes"
        );
    }
}
