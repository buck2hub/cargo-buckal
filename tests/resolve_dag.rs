use std::collections::HashMap;

use cargo_buckal::cache::{BuckalCache, ChangeType};
use cargo_buckal::resolve::{BuckalNode, BuckalResolve, NodeKind};
use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{MetadataCommand, PackageId};

/// Helper: build a BuckalResolve from cargo metadata at `manifest_dir`.
fn resolve_from_manifest(manifest_dir: &str) -> BuckalResolve {
    let manifest_path = std::path::Path::new(manifest_dir).join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("cargo metadata failed");
    let packages_map: HashMap<PackageId, cargo_metadata::Package> = metadata
        .packages
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let resolve = metadata.resolve.expect("no resolve in metadata");
    let nodes_map: HashMap<PackageId, cargo_metadata::Node> = resolve
        .nodes
        .into_iter()
        .map(|n| (n.id.clone(), n))
        .collect();
    let root_path = std::path::Path::new(metadata.workspace_root.as_str());
    BuckalResolve::from_metadata(&nodes_map, &packages_map, &HashMap::new(), root_path, false)
        .expect("from_metadata should succeed for this fixture")
}

#[test]
#[ignore]
fn test_dag_first_party_demo() {
    let resolve = resolve_from_manifest("/tmp/buckal-test/first-party-demo");

    // Should contain the 3 first-party crates
    assert!(
        resolve.find_by_name("demo-root", None).is_some(),
        "demo-root not found"
    );
    assert!(
        resolve.find_by_name("demo-lib", None).is_some(),
        "demo-lib not found"
    );
    assert!(
        resolve.find_by_name("demo-util", None).is_some(),
        "demo-util not found"
    );

    // All 3 first-party crates should be FirstParty
    for name in &["demo-root", "demo-lib", "demo-util"] {
        let node = resolve.find_by_name(name, None).unwrap();
        assert!(
            matches!(&node.kind, NodeKind::FirstParty { .. }),
            "{} should be FirstParty, got {:?}",
            name,
            node.kind
        );
    }

    // demo-root relative_path should be "" (it's at the workspace root)
    let root_node = resolve.find_by_name("demo-root", None).unwrap();
    match &root_node.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(
                relative_path, "",
                "demo-root should have empty relative_path"
            );
        }
        _ => panic!("expected FirstParty"),
    }

    // demo-lib relative path should be "crates/demo-lib"
    let lib_node = resolve.find_by_name("demo-lib", None).unwrap();
    match &lib_node.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(relative_path, "crates/demo-lib");
        }
        _ => panic!("expected FirstParty"),
    }

    // demo-util relative path should be "crates/demo-util"
    let util_node = resolve.find_by_name("demo-util", None).unwrap();
    match &util_node.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(relative_path, "crates/demo-util");
        }
        _ => panic!("expected FirstParty"),
    }

    // serde should be ThirdParty
    let serde_node = resolve.find_by_name("serde", None).unwrap();
    assert!(
        matches!(&serde_node.kind, NodeKind::ThirdParty),
        "serde should be ThirdParty"
    );

    // demo-lib depends on serde, so serde's dependents should include demo-lib
    let serde_dependents = resolve.dependents(&serde_node.package_id);
    let serde_dependent_names: Vec<&str> =
        serde_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        serde_dependent_names.contains(&"demo-lib"),
        "serde dependents should include demo-lib, got {:?}",
        serde_dependent_names
    );

    // demo-lib's dependencies should include demo-util and serde
    let lib_deps = resolve.dependencies(&lib_node.package_id);
    let lib_dep_names: Vec<&str> = lib_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        lib_dep_names.contains(&"demo-util"),
        "demo-lib should depend on demo-util, got {:?}",
        lib_dep_names
    );
    assert!(
        lib_dep_names.contains(&"serde"),
        "demo-lib should depend on serde, got {:?}",
        lib_dep_names
    );

    // Total node count should include all transitive deps
    let total_nodes = resolve.nodes().count();
    assert!(
        total_nodes >= 5,
        "expected at least 5 nodes (3 first-party + serde + serde_derive), got {}",
        total_nodes
    );

    // Cache construction should work
    let cache = BuckalCache::from_resolve(
        &resolve,
        &cargo_metadata::camino::Utf8PathBuf::from("/tmp/buckal-test/first-party-demo"),
    );
    // Verify cache has entries for all nodes
    let cache_str = toml::to_string_pretty(&cache).unwrap();
    assert!(
        cache_str.contains("fingerprints"),
        "cache should contain fingerprints section"
    );
}

#[test]
#[ignore]
fn test_dag_monorepo_demo() {
    let resolve = resolve_from_manifest("/tmp/buckal-test/monorepo-demo/project");

    // Should contain the 2 workspace members (virtual workspace - no root package)
    assert!(
        resolve.find_by_name("sub-lib", None).is_some(),
        "sub-lib not found"
    );
    assert!(
        resolve.find_by_name("sub-app", None).is_some(),
        "sub-app not found"
    );

    // Both should be FirstParty
    let sub_lib = resolve.find_by_name("sub-lib", None).unwrap();
    let sub_app = resolve.find_by_name("sub-app", None).unwrap();
    assert!(
        matches!(&sub_lib.kind, NodeKind::FirstParty { .. }),
        "sub-lib should be FirstParty"
    );
    assert!(
        matches!(&sub_app.kind, NodeKind::FirstParty { .. }),
        "sub-app should be FirstParty"
    );

    // Verify relative paths
    match &sub_lib.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(relative_path, "sub-lib");
        }
        _ => panic!("expected FirstParty"),
    }
    match &sub_app.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(relative_path, "sub-app");
        }
        _ => panic!("expected FirstParty"),
    }

    // sub-app depends on sub-lib
    let app_deps = resolve.dependencies(&sub_app.package_id);
    let app_dep_names: Vec<&str> = app_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        app_dep_names.contains(&"sub-lib"),
        "sub-app should depend on sub-lib, got {:?}",
        app_dep_names
    );

    // sub-lib's dependents should include sub-app
    let lib_dependents = resolve.dependents(&sub_lib.package_id);
    let lib_dependent_names: Vec<&str> = lib_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        lib_dependent_names.contains(&"sub-app"),
        "sub-lib dependents should include sub-app, got {:?}",
        lib_dependent_names
    );

    // serde should be ThirdParty and present
    let serde_node = resolve.find_by_name("serde", None).unwrap();
    assert!(matches!(&serde_node.kind, NodeKind::ThirdParty));

    // sub-lib depends on serde (workspace dep)
    let lib_deps = resolve.dependencies(&sub_lib.package_id);
    let lib_dep_names: Vec<&str> = lib_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        lib_dep_names.contains(&"serde"),
        "sub-lib should depend on serde, got {:?}",
        lib_dep_names
    );

    // Cache construction should work
    let cache = BuckalCache::from_resolve(
        &resolve,
        &cargo_metadata::camino::Utf8PathBuf::from("/tmp/buckal-test/monorepo-demo/project"),
    );
    let cache_str = toml::to_string_pretty(&cache).unwrap();
    assert!(
        cache_str.contains("fingerprints"),
        "cache should contain fingerprints section"
    );
}

#[test]
#[ignore]
fn test_dag_fd_find() {
    let resolve = resolve_from_manifest("/tmp/buckal-test/fd");

    // fd-find is the only first-party package
    let fd = resolve.find_by_name("fd-find", None).unwrap();
    assert!(
        matches!(&fd.kind, NodeKind::FirstParty { .. }),
        "fd-find should be FirstParty"
    );
    match &fd.kind {
        NodeKind::FirstParty { relative_path } => {
            assert_eq!(relative_path, "", "fd-find is at workspace root");
        }
        _ => panic!("expected FirstParty"),
    }

    // All other packages should be ThirdParty
    let third_party_count = resolve
        .nodes()
        .filter(|n| matches!(&n.kind, NodeKind::ThirdParty))
        .count();
    let first_party_count = resolve
        .nodes()
        .filter(|n| matches!(&n.kind, NodeKind::FirstParty { .. }))
        .count();
    assert_eq!(first_party_count, 1, "only fd-find is first-party");
    assert!(
        third_party_count >= 80,
        "fd has a large transitive dep graph, got {}",
        third_party_count
    );

    // Total nodes should match the full resolve graph (~103 packages)
    let total = resolve.nodes().count();
    assert!(total >= 100, "expected at least 100 nodes, got {}", total);
    assert_eq!(total, first_party_count + third_party_count);

    // Spot-check key dependencies of fd-find
    let fd_deps = resolve.dependencies(&fd.package_id);
    let fd_dep_names: Vec<&str> = fd_deps.iter().map(|n| n.name.as_str()).collect();
    for expected in &["regex", "clap", "ignore", "anyhow", "jiff"] {
        assert!(
            fd_dep_names.contains(expected),
            "fd-find should depend on {}, got {:?}",
            expected,
            fd_dep_names
        );
    }

    // clap's dependents should include fd-find
    let clap = resolve.find_by_name("clap", None).unwrap();
    assert!(matches!(&clap.kind, NodeKind::ThirdParty));
    let clap_dependents = resolve.dependents(&clap.package_id);
    let clap_dependent_names: Vec<&str> = clap_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        clap_dependent_names.contains(&"fd-find"),
        "clap dependents should include fd-find, got {:?}",
        clap_dependent_names
    );

    // Verify a transitive dependency chain: fd-find -> regex -> regex-syntax
    let regex_node = resolve.find_by_name("regex", None).unwrap();
    let regex_deps = resolve.dependencies(&regex_node.package_id);
    let regex_dep_names: Vec<&str> = regex_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        regex_dep_names.contains(&"regex-syntax"),
        "regex should depend on regex-syntax, got {:?}",
        regex_dep_names
    );

    // regex-syntax's dependents should include both regex and fd-find (fd depends on it directly)
    let regex_syntax = resolve.find_by_name("regex-syntax", None).unwrap();
    let rs_dependents = resolve.dependents(&regex_syntax.package_id);
    let rs_dependent_names: Vec<&str> = rs_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        rs_dependent_names.contains(&"regex"),
        "regex-syntax dependents should include regex, got {:?}",
        rs_dependent_names
    );

    // find_by_name with version filtering
    let clap_version = &clap.version;
    assert!(resolve.find_by_name("clap", Some(clap_version)).is_some());
    assert!(resolve.find_by_name("clap", Some("0.0.0")).is_none());

    // Cache construction and fingerprint determinism
    let ws_root = cargo_metadata::camino::Utf8PathBuf::from("/tmp/buckal-test/fd");
    let cache1 = BuckalCache::from_resolve(&resolve, &ws_root);
    let cache2 = BuckalCache::from_resolve(&resolve, &ws_root);
    let s1 = toml::to_string_pretty(&cache1).unwrap();
    let s2 = toml::to_string_pretty(&cache2).unwrap();
    assert_eq!(
        s1, s2,
        "cache should be deterministic across repeated construction"
    );
    assert!(s1.contains("fingerprints"));

    // Diff of identical caches should produce no changes
    let diff = cache1.diff(&cache2, &ws_root);
    assert!(
        diff.changes.is_empty(),
        "diff of identical caches should be empty, got {} changes",
        diff.changes.len()
    );
}

/// Helper: resolve a fixture workspace relative to CARGO_MANIFEST_DIR.
fn resolve_from_fixture(fixture_name: &str) -> BuckalResolve {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{fixture_name}"));
    assert!(
        fixture_dir.join("Cargo.toml").exists(),
        "fixture not found at {}",
        fixture_dir.display()
    );
    resolve_from_manifest(fixture_dir.to_str().unwrap())
}

/// Integration test: diamond dependency with version conflict using a real Cargo workspace.
///
/// The fixture at `tests/fixtures/diamond-deps/` is a workspace with three crates:
/// - `uses-itoa-old` depends on `itoa = "0.4"`
/// - `uses-itoa-new` depends on `itoa = "1"`
/// - `diamond-root` depends on both `uses-itoa-old` and `uses-itoa-new`
///
/// This creates a true diamond dependency: `diamond-root` transitively pulls in
/// two semver-incompatible versions of `itoa` through different intermediate crates.
/// Cargo resolves both `itoa 0.4.x` and `itoa 1.x` since they are semver-incompatible,
/// producing two separate nodes in the graph for the same crate name.
#[test]
fn test_diamond_deps_version_conflict() {
    let resolve = resolve_from_fixture("diamond-deps");

    // Both workspace members should be FirstParty
    let old_crate = resolve
        .find_by_name("uses-itoa-old", None)
        .expect("uses-itoa-old not found");
    let new_crate = resolve
        .find_by_name("uses-itoa-new", None)
        .expect("uses-itoa-new not found");
    assert!(matches!(&old_crate.kind, NodeKind::FirstParty { .. }));
    assert!(matches!(&new_crate.kind, NodeKind::FirstParty { .. }));

    // Both itoa versions should exist as separate ThirdParty nodes
    let itoa_nodes: Vec<&BuckalNode> = resolve.nodes().filter(|n| n.name == "itoa").collect();
    assert_eq!(
        itoa_nodes.len(),
        2,
        "expected 2 itoa nodes (0.4.x and 1.x), got {}: {:?}",
        itoa_nodes.len(),
        itoa_nodes.iter().map(|n| &n.version).collect::<Vec<_>>()
    );

    // Verify the two itoa nodes have different major versions
    let itoa_versions: Vec<&str> = itoa_nodes.iter().map(|n| n.version.as_str()).collect();
    assert!(
        itoa_versions.iter().any(|v: &&str| v.starts_with("0.4")),
        "expected an itoa 0.4.x, got {:?}",
        itoa_versions
    );
    assert!(
        itoa_versions.iter().any(|v: &&str| v.starts_with("1.")),
        "expected an itoa 1.x, got {:?}",
        itoa_versions
    );

    // Both itoa nodes should be ThirdParty
    for node in &itoa_nodes {
        assert!(
            matches!(&node.kind, NodeKind::ThirdParty),
            "itoa {} should be ThirdParty",
            node.version
        );
    }

    // uses-itoa-old should depend on itoa 0.4.x
    let old_deps = resolve.dependencies(&old_crate.package_id);
    let old_itoa = old_deps
        .iter()
        .find(|n| n.name == "itoa")
        .expect("uses-itoa-old should depend on itoa");
    assert!(
        old_itoa.version.starts_with("0.4"),
        "uses-itoa-old should depend on itoa 0.4.x, got {}",
        old_itoa.version
    );

    // uses-itoa-new should depend on itoa 1.x
    let new_deps = resolve.dependencies(&new_crate.package_id);
    let new_itoa = new_deps
        .iter()
        .find(|n| n.name == "itoa")
        .expect("uses-itoa-new should depend on itoa");
    assert!(
        new_itoa.version.starts_with("1."),
        "uses-itoa-new should depend on itoa 1.x, got {}",
        new_itoa.version
    );

    // itoa 0.4.x dependents should include uses-itoa-old but not uses-itoa-new
    let itoa_old_node = itoa_nodes
        .iter()
        .find(|n| n.version.starts_with("0.4"))
        .unwrap();
    let old_dependents = resolve.dependents(&itoa_old_node.package_id);
    let old_dep_names: Vec<&str> = old_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        old_dep_names.contains(&"uses-itoa-old"),
        "itoa 0.4.x dependents should include uses-itoa-old, got {:?}",
        old_dep_names
    );
    assert!(
        !old_dep_names.contains(&"uses-itoa-new"),
        "itoa 0.4.x dependents should NOT include uses-itoa-new"
    );

    // itoa 1.x dependents should include uses-itoa-new but not uses-itoa-old
    let itoa_new_node = itoa_nodes
        .iter()
        .find(|n| n.version.starts_with("1."))
        .unwrap();
    let new_dependents = resolve.dependents(&itoa_new_node.package_id);
    let new_dep_names: Vec<&str> = new_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        new_dep_names.contains(&"uses-itoa-new"),
        "itoa 1.x dependents should include uses-itoa-new, got {:?}",
        new_dep_names
    );
    assert!(
        !new_dep_names.contains(&"uses-itoa-old"),
        "itoa 1.x dependents should NOT include uses-itoa-old"
    );

    // find_by_name with version filtering should find the correct node
    let found_old = resolve
        .find_by_name("itoa", Some(&itoa_old_node.version))
        .unwrap();
    assert!(found_old.version.starts_with("0.4"));
    let found_new = resolve
        .find_by_name("itoa", Some(&itoa_new_node.version))
        .unwrap();
    assert!(found_new.version.starts_with("1."));

    // Fingerprints of the two itoa versions must differ
    let ws_root = Utf8PathBuf::from("/tmp");
    assert_ne!(
        resolve.fingerprint_of(&itoa_old_node.package_id, &ws_root),
        resolve.fingerprint_of(&itoa_new_node.package_id, &ws_root),
        "different itoa versions should have different fingerprints"
    );

    // diamond-root should be a FirstParty node
    let diamond_root = resolve
        .find_by_name("diamond-root", None)
        .expect("diamond-root not found");
    assert!(matches!(&diamond_root.kind, NodeKind::FirstParty { .. }));

    // diamond-root should depend on both uses-itoa-old and uses-itoa-new
    let root_deps = resolve.dependencies(&diamond_root.package_id);
    let root_dep_names: Vec<&str> = root_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        root_dep_names.contains(&"uses-itoa-old"),
        "diamond-root should depend on uses-itoa-old, got {:?}",
        root_dep_names
    );
    assert!(
        root_dep_names.contains(&"uses-itoa-new"),
        "diamond-root should depend on uses-itoa-new, got {:?}",
        root_dep_names
    );

    // Traversing the graph from diamond-root should reach both itoa versions transitively
    let mut transitive_itoa_versions: Vec<String> = Vec::new();
    for dep in &root_deps {
        for transitive in resolve.dependencies(&dep.package_id) {
            if transitive.name == "itoa" {
                transitive_itoa_versions.push(transitive.version.clone());
            }
        }
    }
    assert!(
        transitive_itoa_versions
            .iter()
            .any(|v| v.starts_with("0.4")),
        "diamond-root should transitively reach itoa 0.4.x, got {:?}",
        transitive_itoa_versions
    );
    assert!(
        transitive_itoa_versions.iter().any(|v| v.starts_with("1.")),
        "diamond-root should transitively reach itoa 1.x, got {:?}",
        transitive_itoa_versions
    );

    // Cache construction should work
    let fixture_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diamond-deps");
    let ws_root = cargo_metadata::camino::Utf8PathBuf::from(fixture_dir.to_str().unwrap());
    let cache = BuckalCache::from_resolve(&resolve, &ws_root);
    let cache_str = toml::to_string_pretty(&cache).unwrap();
    assert!(
        cache_str.contains("fingerprints"),
        "cache should contain fingerprints section"
    );
}

/// Integration test: diamond dependency with semver-compatible versions.
///
/// The fixture at `tests/fixtures/diamond-deps-compat/` is a workspace with three crates:
/// - `uses-itoa-loose` depends on `itoa = "1.0"` (>=1.0.0, <2.0.0)
/// - `uses-itoa-pinned` depends on `itoa = "1.0.5"` (>=1.0.5, <2.0.0)
/// - `compat-root` depends on both
///
/// Since both constraints are semver-compatible, Cargo unifies them into a single
/// `itoa` version (>=1.0.5) — producing exactly one node in the graph, unlike the
/// incompatible diamond which produces two.
#[test]
fn test_diamond_deps_semver_compatible() {
    let resolve = resolve_from_fixture("diamond-deps-compat");

    // All three workspace members should be FirstParty
    let loose = resolve
        .find_by_name("uses-itoa-loose", None)
        .expect("uses-itoa-loose not found");
    let pinned = resolve
        .find_by_name("uses-itoa-pinned", None)
        .expect("uses-itoa-pinned not found");
    let root = resolve
        .find_by_name("compat-root", None)
        .expect("compat-root not found");
    assert!(matches!(&loose.kind, NodeKind::FirstParty { .. }));
    assert!(matches!(&pinned.kind, NodeKind::FirstParty { .. }));
    assert!(matches!(&root.kind, NodeKind::FirstParty { .. }));

    // Cargo should unify both constraints into exactly ONE itoa node
    let itoa_nodes: Vec<&BuckalNode> = resolve.nodes().filter(|n| n.name == "itoa").collect();
    assert_eq!(
        itoa_nodes.len(),
        1,
        "semver-compatible constraints should unify to 1 itoa node, got {}: {:?}",
        itoa_nodes.len(),
        itoa_nodes.iter().map(|n| &n.version).collect::<Vec<_>>()
    );

    let itoa = itoa_nodes[0];
    assert!(
        itoa.version.starts_with("1."),
        "unified itoa should be 1.x, got {}",
        itoa.version
    );
    assert!(matches!(&itoa.kind, NodeKind::ThirdParty));

    // Both intermediate crates should depend on the SAME itoa node
    let loose_deps = resolve.dependencies(&loose.package_id);
    let pinned_deps = resolve.dependencies(&pinned.package_id);
    let loose_itoa = loose_deps
        .iter()
        .find(|n| n.name == "itoa")
        .expect("uses-itoa-loose should depend on itoa");
    let pinned_itoa = pinned_deps
        .iter()
        .find(|n| n.name == "itoa")
        .expect("uses-itoa-pinned should depend on itoa");
    assert_eq!(
        loose_itoa.package_id, pinned_itoa.package_id,
        "both crates should resolve to the same itoa: {} vs {}",
        loose_itoa.version, pinned_itoa.version
    );

    // The single itoa node should have both crates as dependents
    let itoa_dependents = resolve.dependents(&itoa.package_id);
    let dep_names: Vec<&str> = itoa_dependents.iter().map(|n| n.name.as_str()).collect();
    assert!(
        dep_names.contains(&"uses-itoa-loose"),
        "itoa dependents should include uses-itoa-loose, got {:?}",
        dep_names
    );
    assert!(
        dep_names.contains(&"uses-itoa-pinned"),
        "itoa dependents should include uses-itoa-pinned, got {:?}",
        dep_names
    );

    // compat-root should depend on both intermediate crates
    let root_deps = resolve.dependencies(&root.package_id);
    let root_dep_names: Vec<&str> = root_deps.iter().map(|n| n.name.as_str()).collect();
    assert!(root_dep_names.contains(&"uses-itoa-loose"));
    assert!(root_dep_names.contains(&"uses-itoa-pinned"));

    // Traversing from compat-root through both paths should reach the same itoa
    let mut transitive_itoa_ids: Vec<&PackageId> = Vec::new();
    for dep in &root_deps {
        for transitive in resolve.dependencies(&dep.package_id) {
            if transitive.name == "itoa" {
                transitive_itoa_ids.push(&transitive.package_id);
            }
        }
    }
    assert_eq!(
        transitive_itoa_ids.len(),
        2,
        "should reach itoa through 2 paths, got {}",
        transitive_itoa_ids.len()
    );
    assert_eq!(
        transitive_itoa_ids[0], transitive_itoa_ids[1],
        "both paths should reach the same itoa node"
    );
}

/// Integration test: dev-dependency pseudo-cycle is rejected as an error.
///
/// The fixture at `tests/fixtures/dev-dep-cycle/` is a workspace with two crates:
/// - `core-lib` has `[dev-dependencies] test-utils = { path = "../test-utils" }`
/// - `test-utils` has `[dependencies] core-lib = { path = "../core-lib" }`
///
/// `cargo metadata` flattens this into a single graph, creating a cycle:
/// `core-lib → test-utils → core-lib`. Cargo allows this, but Buck2 requires
/// a strictly acyclic graph, so `from_metadata` must return an error.
#[test]
fn test_dev_dependency_cycle() {
    let fixture_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dev-dep-cycle");
    let manifest_path = fixture_dir.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("cargo metadata failed");
    let packages_map: HashMap<PackageId, cargo_metadata::Package> = metadata
        .packages
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let resolve = metadata.resolve.expect("no resolve in metadata");
    let nodes_map: HashMap<PackageId, cargo_metadata::Node> = resolve
        .nodes
        .into_iter()
        .map(|n| (n.id.clone(), n))
        .collect();
    let root_path = std::path::Path::new(metadata.workspace_root.as_str());

    let result =
        BuckalResolve::from_metadata(&nodes_map, &packages_map, &HashMap::new(), root_path, false);

    let err_msg = result
        .err()
        .expect("dev-dep cycle should cause from_metadata to fail")
        .to_string();
    assert!(
        err_msg.contains("dev-dependency cycle detected"),
        "error should mention dev-dependency cycle, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("core-lib") || err_msg.contains("test-utils"),
        "error should identify the cycle-forming packages, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("doc.rust-lang.org/cargo/reference/resolver.html#dev-dependency-cycles"),
        "error should include Cargo docs link, got: {}",
        err_msg
    );
}

/// Verify that `cargo test` succeeds for a mutual dev-dependency cycle.
///
/// Despite the flattened `cargo metadata` graph containing a cycle
/// (`crate-a → crate-b → crate-a`), Cargo can build all targets including
/// tests because dev-dependencies are only used for test/example builds,
/// not library builds. The actual build order is:
///
/// ```text
/// 1. Build crate-a lib  (no deps — dev-deps not used)
/// 2. Build crate-b lib  (no deps — dev-deps not used)
/// 3. Build crate-a tests (needs crate-b lib — already built ✓)
/// 4. Build crate-b tests (needs crate-a lib — already built ✓)
/// ```
///
/// Each test target exercises the dev-dependency: `crate-a`'s tests call
/// `crate_b::b_value()` and `crate-b`'s tests call `crate_a::a_value()`.
#[test]
fn test_dev_dev_cycle_cargo_test_succeeds() {
    let fixture_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dev-dev-cycle");

    let output = std::process::Command::new("cargo")
        .arg("test")
        .current_dir(&fixture_dir)
        .output()
        .expect("failed to run cargo test");

    assert!(
        output.status.success(),
        "cargo test should succeed for mutual dev-dep cycle.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify both test targets actually ran (test names appear in stdout)
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test tests::test_a_uses_b")
            && stdout.contains("test tests::test_b_uses_a"),
        "both test targets should run.\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verify that a project without Cargo.lock can still produce a valid resolve and cache.
///
/// This is the scenario for freshly initialized lib crates where `cargo buckal add` is
/// the first command run. The old code called `BuckalContext::new()` which panicked on
/// missing Cargo.lock; the fix passes empty checksums instead.
#[test]
fn test_resolve_without_lockfile() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let cargo_toml = tmp.path().join("Cargo.toml");
    std::fs::write(
        &cargo_toml,
        "[package]\nname = \"no-lock\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "").unwrap();

    // Cargo.lock does NOT exist — this is the key precondition
    assert!(!tmp.path().join("Cargo.lock").exists());

    // cargo metadata works fine without Cargo.lock
    let metadata = MetadataCommand::new()
        .manifest_path(&cargo_toml)
        .exec()
        .expect("cargo metadata should work without Cargo.lock");

    let packages_map: HashMap<PackageId, cargo_metadata::Package> = metadata
        .packages
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let nodes_map: HashMap<PackageId, cargo_metadata::Node> = metadata
        .resolve
        .expect("resolve should be present")
        .nodes
        .into_iter()
        .map(|n| (n.id.clone(), n))
        .collect();

    // Build resolve with empty checksums (no Cargo.lock available)
    let resolve = BuckalResolve::from_metadata(
        &nodes_map,
        &packages_map,
        &HashMap::new(),
        tmp.path(),
        false,
    )
    .expect("from_metadata should succeed for a single-crate project");

    // Should have the one local package
    let node = resolve
        .find_by_name("no-lock", None)
        .expect("no-lock not found");
    assert!(matches!(&node.kind, NodeKind::FirstParty { .. }));

    // Cache construction should work despite missing checksums
    let ws_root = cargo_metadata::camino::Utf8PathBuf::from(tmp.path().to_str().unwrap());
    let cache = BuckalCache::from_resolve(&resolve, &ws_root);
    let cache_str = toml::to_string_pretty(&cache).unwrap();
    assert!(
        cache_str.contains("fingerprints"),
        "cache should contain fingerprints section"
    );
}

/// Verify that building a fallback cache with --manifest-path targets the correct workspace.
///
/// When the user runs `cargo buckal add --manifest-path subcrate/Cargo.toml`,
/// the fallback cache must be built from the subcrate's metadata, not from cwd.
/// Otherwise the diff compares the wrong workspaces and may produce spurious
/// "Removed" entries that delete BUCK files in the wrong locations.
#[test]
fn test_fallback_cache_honors_manifest_path() {
    // Build caches from two different fixture workspaces
    let fixture_a = resolve_from_fixture("diamond-deps");
    let fixture_b = resolve_from_fixture("diamond-deps-compat");

    let ws_a = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diamond-deps");
    let ws_b =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diamond-deps-compat");

    let cache_a = BuckalCache::from_resolve(
        &fixture_a,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_a.to_str().unwrap()),
    );
    let cache_b = BuckalCache::from_resolve(
        &fixture_b,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_b.to_str().unwrap()),
    );

    let str_a = toml::to_string_pretty(&cache_a).unwrap();
    let str_b = toml::to_string_pretty(&cache_b).unwrap();

    // The two caches must differ — if manifest_path is ignored, both would
    // resolve to the same cwd workspace and produce identical caches.
    assert_ne!(
        str_a, str_b,
        "caches from different workspaces must differ; \
         using the wrong manifest_path would make them identical"
    );

    // diamond-deps has two itoa versions; diamond-deps-compat has one
    // Both reference itoa, but the caches differ because the workspaces
    // have different package sets and fingerprints.
    assert!(
        str_a.contains("itoa"),
        "diamond-deps cache should reference itoa"
    );
    assert!(
        str_b.contains("itoa"),
        "diamond-deps-compat cache should also reference itoa"
    );

    // diamond-deps-compat has uses-itoa-loose; diamond-deps does not
    assert!(
        str_b.contains("uses-itoa-loose"),
        "diamond-deps-compat cache should reference uses-itoa-loose"
    );
    assert!(
        !str_a.contains("uses-itoa-loose"),
        "diamond-deps cache should not reference uses-itoa-loose"
    );
}

/// Verify that diffing against an empty cache loses removal detection.
///
/// When `migrate` falls back to `BuckalCache::new_empty()` (e.g. after a cache version
/// bump), packages that were removed from the dependency tree are never detected as
/// "Removed" — their stale vendored dirs/BUCK files are left behind. Diffing against a
/// real fallback cache correctly detects these removals.
#[test]
fn test_empty_cache_loses_removals() {
    // Build two caches from different workspaces:
    // - "old" has diamond-deps packages (includes itoa 0.4, itoa 1.x, etc.)
    // - "new" has diamond-deps-compat packages (uses-itoa-loose, uses-itoa-pinned, etc.)
    let old_resolve = resolve_from_fixture("diamond-deps");
    let new_resolve = resolve_from_fixture("diamond-deps-compat");

    let ws_old =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diamond-deps");
    let ws_new =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diamond-deps-compat");

    let old_cache = BuckalCache::from_resolve(
        &old_resolve,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_old.to_str().unwrap()),
    );
    let new_cache = BuckalCache::from_resolve(
        &new_resolve,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_new.to_str().unwrap()),
    );
    let empty_cache = BuckalCache::new_empty();

    // Diffing new against OLD cache: should detect Removed entries
    // (packages in old but not new = removed)
    let diff_real = new_cache.diff(
        &old_cache,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_new.to_str().unwrap()),
    );
    let removed_count = diff_real
        .changes
        .values()
        .filter(|c| matches!(c, ChangeType::Removed))
        .count();
    assert!(
        removed_count > 0,
        "diffing against a real old cache should detect removals, got 0"
    );

    // Diffing new against EMPTY cache: no Removed entries — this is the bug
    let diff_empty = new_cache.diff(
        &empty_cache,
        &cargo_metadata::camino::Utf8PathBuf::from(ws_new.to_str().unwrap()),
    );
    let removed_empty = diff_empty
        .changes
        .values()
        .filter(|c| matches!(c, ChangeType::Removed))
        .count();
    assert_eq!(
        removed_empty, 0,
        "diffing against an empty cache cannot detect removals"
    );

    // This demonstrates the problem: using new_empty() as fallback loses removals.
    // The fix is to use get_last_cache() which rebuilds from metadata.
    assert!(
        removed_count > removed_empty,
        "real fallback cache detects more removals ({}) than empty cache ({})",
        removed_count,
        removed_empty
    );
}

/// Integration test: normal-dependency cycle (both edges are `[dependencies]`).
///
/// The fixture at `tests/fixtures/normal-dep-cycle/` is a workspace with two crates:
/// - `crate-a` has `[dependencies] crate-b = { path = "../crate-b" }`
/// - `crate-b` has `[dependencies] crate-a = { path = "../crate-a" }`
///
/// This is a hard cycle — Cargo rejects it outright. `cargo metadata` itself fails
/// with "cyclic package dependency", so `BuckalResolve::from_metadata` is never reached.
#[test]
fn test_normal_dep_cycle_rejected_by_cargo() {
    let fixture_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/normal-dep-cycle");
    assert!(
        fixture_dir.join("Cargo.toml").exists(),
        "fixture not found at {}",
        fixture_dir.display()
    );

    let result = MetadataCommand::new()
        .manifest_path(fixture_dir.join("Cargo.toml"))
        .exec();

    assert!(
        result.is_err(),
        "cargo metadata should fail for a normal-dep cycle, but it succeeded"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("cyclic package dependency"),
        "error should mention cyclic dependency, got: {}",
        err_msg
    );
}

/// Integration test: dev-dependency cycle where BOTH edges are `[dev-dependencies]`.
///
/// The fixture at `tests/fixtures/dev-dev-cycle/` is a workspace with two crates:
/// - `crate-a` has `[dev-dependencies] crate-b = { path = "../crate-b" }`
/// - `crate-b` has `[dev-dependencies] crate-a = { path = "../crate-a" }`
///
/// `cargo metadata` succeeds (dev-deps don't create build-time cycles), but the
/// flattened graph contains a cycle. `from_metadata` must return an error because
/// Buck2 requires a strictly acyclic graph.
#[test]
fn test_dev_dev_cycle_rejected() {
    let fixture_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dev-dev-cycle");
    let manifest_path = fixture_dir.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("cargo metadata should succeed for dev-dev cycle");
    let packages_map: HashMap<PackageId, cargo_metadata::Package> = metadata
        .packages
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let resolve = metadata.resolve.expect("no resolve in metadata");
    let nodes_map: HashMap<PackageId, cargo_metadata::Node> = resolve
        .nodes
        .into_iter()
        .map(|n| (n.id.clone(), n))
        .collect();
    let root_path = std::path::Path::new(metadata.workspace_root.as_str());

    let result =
        BuckalResolve::from_metadata(&nodes_map, &packages_map, &HashMap::new(), root_path, false);

    let err_msg = result
        .err()
        .expect("dev-dev cycle should cause from_metadata to fail")
        .to_string();
    assert!(
        err_msg.contains("dev-dependency cycle detected"),
        "error should mention dev-dependency cycle, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("crate-a") || err_msg.contains("crate-b"),
        "error should identify the cycle-forming packages, got: {}",
        err_msg
    );
}
