# DAG refactor (BuckalNode / BuckalResolve)

This document describes the dependency-graph refactor that replaced the flat `HashMap`-based
representation with a proper DAG (directed acyclic graph) built on `daggy`.

## Motivation

The previous approach stored dependency data in three separate `HashMap`s directly from
`cargo_metadata`:

```
OLD: BuckalContext
  ├── nodes_map:     HashMap<PackageId, Node>       (features, dep edges)
  ├── packages_map:  HashMap<PackageId, Package>    (targets, manifest_path, source)
  └── checksums_map: HashMap<String, String>        (Cargo.lock checksums)

  Every consumer did multi-map lookups:

    let node = ctx.nodes_map.get(&id);
    let pkg  = ctx.packages_map.get(&id);   // separate lookup
    let cksum = ctx.checksums_map.get(&key); // yet another lookup
```

This had several problems:

1. **No graph structure.** There was no way to query reverse dependencies ("who depends on X?")
   or traverse the dependency tree. Every consumer did flat lookups across multiple maps.

2. **Diamond dependency bugs.** When two crates depend on different semver-incompatible versions
   of the same crate (e.g., `itoa 0.4` and `itoa 1.0`), Cargo resolves both as separate nodes.
   The flat maps had no structural way to distinguish these edges, which could produce incorrect
   BUCK labels.

3. **Workspace-inherited fields.** The old context did not track whether the workspace uses
   `workspace.dependencies` inheritance. Parsing workspace-inherited package fields (like
   `version` or `edition`) could crash.

4. **Incomplete cache fingerprints.** The old `BuckalHash` was implemented on
   `cargo_metadata::Node`, which only included features and dep edges. Changes to a package's
   targets, source, or manifest_path would not invalidate the cache.

5. **Python runtime dependency.** BUCK file parsing relied on `pyo3`, requiring a Python runtime.

## New data model

### Architecture overview

```
cargo metadata ──> BuckalResolve::from_metadata()
                          |
                          v
   ┌──────────────────────────────────────────┐
   │            BuckalResolve                  │
   │                                           │
   │   dag: Dag<BuckalNode, BuckalDep>           │
   │   index_map: HashMap<PackageId, NodeIdx>  │
   │                                           │
   │   Methods:                                │
   │     .get(pkg_id)        -> &BuckalNode    │
   │     .dependencies(id)   -> [&BuckalNode]  │
   │     .dependents(id)     -> [&BuckalNode]  │
   │     .deps_of(id)        -> [(&BuckalDep,  │
   │                              &BuckalNode)]│
   │     .find_by_name(name) -> [&BuckalNode]  │
   │     .nodes()            -> iter           │
   │     .fingerprint_of(id, ws) -> Fingerprint │
   └──────────────────────────────────────────┘
                    |
                    v
   ┌──────────────────────────────────────────┐
   │            BuckalContext                   │
   │                                           │
   │   root: Option<PackageId>                 │
   │   resolve: BuckalResolve  <── dep graph   │
   │   workspace_root                          │
   │   workspace_inherit: bool                 │
   │   no_merge, repo_config                   │
   └──────────────────────────────────────────┘
```

### `BuckalNode`

A unified struct that merges data from `cargo_metadata::Node` and `Package`:

```rust
pub struct BuckalNode {
    pub package_id: PackageId,
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
    pub kind: NodeKind,           // FirstParty { relative_path } or ThirdParty
    pub edition: Edition,
    pub manifest_path: Utf8PathBuf,
    pub targets: Vec<BuckalTarget>,
    pub source: Option<String>,
    pub links: Option<String>,
    pub checksum: Option<String>,
}
```

Key improvements over the old split representation:

- **Single lookup** gives you everything about a package.
- **`NodeKind`** is computed once at construction time (based on whether the package path is under
  the workspace root), eliminating repeated `package.source.is_none()` checks.
- **`BuckalDep`** is used as the graph edge weight (not stored in `BuckalNode`), carrying dependency
  name and `BuckalDepKind` (kind + platform constraint) for each edge.
- **`BuckalTarget`** is a serializable replacement for `cargo_metadata::Target`.

### `BuckalResolve`

A dependency DAG wrapping `daggy::Dag<BuckalNode, BuckalDep>`:

```rust
pub struct BuckalResolve {
    pub dag: Dag<BuckalNode, BuckalDep, u32>,
    pub index_map: HashMap<PackageId, NodeIndex<u32>>,
}
```

Methods:

| Method | Description |
|---|---|
| `from_metadata(...)` | Constructs the dependency graph from raw cargo metadata |
| `get(pkg_id)` | O(1) node lookup by `PackageId` |
| `dependencies(pkg_id)` | Direct dependency nodes (children in the graph) |
| `dependents(pkg_id)` | Reverse dependency nodes (parents in the graph) |
| `deps_of(pkg_id)` | `(edge_weight, child_node)` pairs for iterating with metadata |
| `find_by_name(name, version)` | Find a node by crate name and optional version |
| `nodes()` | Iterator over all nodes |
| `fingerprint_of(pkg_id, workspace_root)` | Location-independent fingerprint covering node + edges |

### Updated `BuckalContext`

```rust
pub struct BuckalContext {
    pub root: Option<PackageId>,       // was Option<Package>
    pub resolve: BuckalResolve,        // replaces 3 HashMaps
    pub workspace_root: Utf8PathBuf,
    pub workspace_inherit: bool,       // new
    pub no_merge: bool,
    pub repo_config: RepoConfig,
}
```

## What changed

| Aspect | Before | After |
|---|---|---|
| Data model | 3 flat `HashMap`s | Single `BuckalResolve` DAG |
| Package info | Split across `Node` + `Package` | Unified `BuckalNode` |
| First/third-party | Checked at each use site | `NodeKind` computed once |
| Reverse deps | Not possible | `resolve.dependents(pkg_id)` |
| Root package | `Option<Package>` (full clone) | `Option<PackageId>` (lightweight) |
| Cache fingerprint | `Node` only | `BuckalNode` (all fields) |
| Cache version | 2 | 3 |
| Python dependency | `pyo3` for BUCK parsing | Removed; pure Rust AST parser |
| New dependency | — | `daggy = "0.9"` |
| Workspace tracking | None | `workspace_inherit` field |

### Buckify module changes

All functions in `src/buckify/` were updated to accept `&BuckalNode` instead of `&Node`:

```
BEFORE                                      AFTER
──────                                      ─────
buckify_dep_node(node: &Node, ctx)          buckify_dep_node(node: &BuckalNode, ctx)
  let pkg = ctx.packages_map.get(&id);        // node already has all fields
  let targets = &pkg.targets;                  let targets = &node.targets;

set_deps(rule, node, kind, ctx)             set_deps(rule, node, kind, ctx)
  let dep_pkg = ctx.packages_map.get(..);     for (dep, dep_node) in ctx.resolve.deps_of(..)
```

- `buckify_dep_node(node: &BuckalNode, ctx)` — no longer needs a separate `packages_map` lookup.
- `set_deps(rule, node, kind, ctx)` — resolves deps via `ctx.resolve.deps_of(&node.package_id)`.
- `emit_buildscript_run(node, build_target, ctx)` — iterates with `ctx.resolve.deps_of()`.
- `emit_cargo_manifest(node, ctx)` — new workspace manifest export logic for first-party crates.

The `buckify/` module was also restructured from a single `buckify.rs` file to a `buckify/mod.rs`
directory module.

### Cache changes

- Cache version bumped from 2 to 3.
- Fingerprints are computed by `BuckalResolve::fingerprint_of()`, which hashes individual
  `BuckalNode` fields selectively (excluding absolute `manifest_path`, normalizing
  `targets[*].src_path` to relative, canonicalizing `PackageId` via `PackageIdExt`) plus
  all outgoing edge weights (`BuckalDep`) and canonicalized child `PackageId`s, sorted for
  determinism. This ensures fingerprints are portable across checkout locations.
- Old caches (version < 3) are silently discarded and rebuilt.
- Future caches (version > 3) produce an error prompting the user to upgrade.

### Library extraction

`src/lib.rs` was extracted from `main.rs`, making the crate usable as a library. This enables
integration tests to import `cargo_buckal::resolve` and `cargo_buckal::cache` directly.

## Diamond dependency handling

The graph correctly models diamond dependencies where two intermediate crates depend on different
semver-incompatible versions of the same crate.

### Version conflict (two separate nodes)

```
         diamond-root
          /        \
         v          v
  uses-itoa-old   uses-itoa-new
         |              |
         v              v
    itoa 0.4.x     itoa 1.x         <-- two distinct BuckalNodes
```

Each version of `itoa` is a separate `BuckalNode` with distinct edges. You can query
`resolve.dependents(itoa_04_id)` to get only `uses-itoa-old`, not `uses-itoa-new`.

### Semver-compatible (unified into one node)

```
        compat-root
         /        \
        v          v
  uses-itoa-loose  uses-itoa-pinned
        \          /
         v        v
        itoa 1.x.y               <-- single BuckalNode (Cargo unifies)
```

When the constraints are semver-compatible (e.g., `itoa = "1.0"` and `itoa = "1.0.5"`), Cargo
unifies them into a single node, and the graph reflects this correctly — both intermediate crates
point to the same `BuckalNode`.

## Test coverage

### Unit tests (`src/resolve.rs`)

- `test_three_node_chain` — linear A -> B -> C chain with correct `dependents`/`dependencies`.
- `test_first_party_relative_path` — `NodeKind::FirstParty` stores the correct relative path.
- `test_find_by_name` — lookup with and without version filter.
- `test_fingerprint_stability_and_sensitivity` — deterministic hashing; different versions produce
  different fingerprints.
- `test_diamond_dependency_version_conflict` — pure unit test constructing a diamond with two
  versions of the same crate.

### Integration tests (`tests/resolve_dag.rs`)

- `test_diamond_deps_version_conflict` — uses `tests/fixtures/diamond-deps/` workspace with
  real `cargo metadata` to verify two separate `itoa` nodes.
- `test_diamond_deps_semver_compatible` — uses `tests/fixtures/diamond-deps-compat/` to verify
  Cargo unifies compatible constraints into one node.
- `test_dev_dependency_cycle` — uses `tests/fixtures/dev-dep-cycle/` to verify that
  dev-dependency cycles cause `from_metadata()` to fail fast with an actionable error message
  (Buck2 requires strictly acyclic graphs).
- `test_dev_dev_cycle_cargo_test_succeeds` — verifies that `cargo test` works on mutual
  dev-dependency cycles (Cargo handles them fine; buckal intentionally rejects them).
- `test_dev_dev_cycle_rejected` — uses `tests/fixtures/dev-dev-cycle/` to verify both-dev-dep
  cycles are also rejected.
- `test_normal_dep_cycle_rejected_by_cargo` — verifies Cargo itself rejects normal-dep cycles
  at the metadata level.
- `test_resolve_without_lockfile` — verifies resolve and cache construction work without a
  `Cargo.lock` file.
- `test_fallback_cache_honors_manifest_path` — verifies `--manifest-path` produces the correct
  cache for different workspaces.
- `test_empty_cache_loses_removals` — verifies that diffing against an empty cache cannot
  detect removed packages.
- `test_dag_first_party_demo` (`#[ignore]`) — clones a demo workspace, verifies first-party
  detection and edges.
- `test_dag_monorepo_demo` (`#[ignore]`) — virtual workspace with inter-member dependencies.
- `test_dag_fd_find` (`#[ignore]`) — real-world test against the `fd` project (~100+ nodes).

### Test fixtures

- `tests/fixtures/diamond-deps/` — workspace with `uses-itoa-old` (itoa 0.4) and
  `uses-itoa-new` (itoa 1).
- `tests/fixtures/diamond-deps-compat/` — workspace with `uses-itoa-loose` (itoa 1.0) and
  `uses-itoa-pinned` (itoa 1.0.5).
- `tests/fixtures/dev-dep-cycle/` — workspace with `core-lib` (dev-depends on `test-utils`)
  and `test-utils` (depends on `core-lib`), forming a dev-dependency cycle.
- `tests/fixtures/dev-dev-cycle/` — workspace with `crate-a` and `crate-b` mutually
  dev-depending on each other.
- `tests/fixtures/normal-dep-cycle/` — workspace with `crate-a` and `crate-b` mutually
  depending on each other via normal `[dependencies]` (Cargo rejects this at metadata level).
