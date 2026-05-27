use std::collections::BTreeMap;

use anyhow::{Error, Result, anyhow};
use cargo_metadata::{PackageId, camino::Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::buckal_error;
use crate::utils::{UnwrapOrExit, get_cache_path};

use crate::config::RepoPatchConfig;
use crate::resolve::BuckalResolve;

// type Fingerprint = [u8; 32];

/// CACHE_VERSION is incremented whenever the cache format or logic changes in a way that is not backward-compatible.
///
/// Version 2: Added multi-platform support to the cache format.
/// Version 3: Switched to BuckalNode-based fingerprinting (DAG refactor).
/// Version 4: Include version patch config in fingerprints.
/// Version 5: Canonicalized workspace-internal path-source PackageIds to the
///            `($WORKSPACE)` `file://`-URL form. Pre-v5 caches stored the
///            absolute workspace path verbatim (always so on Windows, where
///            canonicalization used to no-op), so diffing an older cache against a
///            v5-format snapshot would report spurious add/remove entries.
///
/// Migration strategy:
/// - If found < expected (stale cache from older Buckal): ignore the old cache and rebuild.
/// - If found > expected (cache from newer Buckal): exit immediately and prompt the user to upgrade.
const CACHE_VERSION: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Serialize for Fingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("fingerprint must be 32 bytes"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Fingerprint(arr))
    }
}

pub trait BuckalHash {
    fn fingerprint(&self) -> Fingerprint;
}

/// Placeholder written into `buckal.snap` in place of the absolute workspace
/// root, so the cache file is portable across checkouts at different paths.
const WORKSPACE_ID_PLACEHOLDER: &str = "path+file://($WORKSPACE)";

/// The `path+file://…` package-id prefix Cargo emits for crates under
/// `workspace_root`.
///
/// `cargo metadata` reports `workspace_root` as a native filesystem path, but
/// path-source package ids embed it as a `file://` URL — forward slashes, a
/// leading slash before a Windows drive letter, percent-encoding for characters
/// like spaces. Building the prefix via [`url::Url::from_file_path`], the same
/// conversion Cargo itself uses, keeps the two in lockstep on every platform
/// (the previous hand-rolled substring match silently failed on Windows and on
/// any path containing escaped characters).
fn workspace_id_prefix(workspace_root: &Utf8PathBuf) -> String {
    let url = url::Url::from_file_path(workspace_root.as_std_path())
        .expect("workspace_root from `cargo metadata` is always an absolute path");
    format!("path+{url}")
}

/// Whether `rest` — what follows a workspace-root prefix in a package-id repr —
/// is a real boundary (a subpath `/…` or the version tag `#…`) rather than the
/// prefix merely being a substring of a sibling directory's path.
fn is_id_boundary(rest: &str) -> bool {
    rest.starts_with('/') || rest.starts_with('#')
}

pub trait PackageIdExt {
    /// ($WORKSPACE) → workspace_root
    fn resolve(&self, workspace_root: &Utf8PathBuf) -> Self;

    /// workspace_root → ($WORKSPACE)
    fn canonicalize(&self, workspace_root: &Utf8PathBuf) -> Self;
}

impl PackageIdExt for PackageId {
    fn resolve(&self, workspace_root: &Utf8PathBuf) -> Self {
        match self.repr.strip_prefix(WORKSPACE_ID_PLACEHOLDER) {
            Some(rest) if is_id_boundary(rest) => PackageId {
                repr: format!("{}{rest}", workspace_id_prefix(workspace_root)),
            },
            _ => self.clone(),
        }
    }

    fn canonicalize(&self, workspace_root: &Utf8PathBuf) -> Self {
        let prefix = workspace_id_prefix(workspace_root);
        match self.repr.strip_prefix(prefix.as_str()) {
            Some(rest) if is_id_boundary(rest) => PackageId {
                repr: format!("{WORKSPACE_ID_PLACEHOLDER}{rest}"),
            },
            _ => self.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuckalCache {
    fingerprints: BTreeMap<PackageId, Fingerprint>,
    version: u32,
}

impl BuckalCache {
    pub fn from_resolve(
        resolve: &BuckalResolve,
        workspace_root: &Utf8PathBuf,
        patch_config: &RepoPatchConfig,
    ) -> Self {
        let fingerprints = resolve
            .nodes()
            .map(|node| {
                (
                    node.package_id.canonicalize(workspace_root),
                    resolve.fingerprint_of(&node.package_id, workspace_root, patch_config),
                )
            })
            .collect();
        Self {
            fingerprints,
            version: CACHE_VERSION,
        }
    }

    pub fn new_empty() -> Self {
        Self {
            fingerprints: BTreeMap::new(),
            version: CACHE_VERSION,
        }
    }

    /// Read and parse the on-disk cache, exiting if it was written by a newer
    /// Buckal. The returned cache may still be a stale (older) version — callers
    /// decide whether to reject or migrate it.
    fn read_from_disk() -> Result<Self, Error> {
        let cache_path = get_cache_path().unwrap_or_exit_ctx("failed to get cache path");
        if !cache_path.exists() {
            return Err(anyhow!("Cache file does not exist"));
        }
        let content = std::fs::read_to_string(&cache_path)?;
        let cache = toml::from_str::<BuckalCache>(&content)
            .map_err(|e| anyhow!("Failed to parse cache file: {}", e))?;
        if cache.version > CACHE_VERSION {
            buckal_error!(
                "Cache was written by a newer version of Buckal (found v{}, expected v{}). Please upgrade Buckal.",
                cache.version,
                CACHE_VERSION
            );
            std::process::exit(1);
        }
        Ok(cache)
    }

    pub fn load() -> Result<Self, Error> {
        let cache = Self::read_from_disk()?;
        if cache.version < CACHE_VERSION {
            return Err(anyhow!(
                "Cache version is stale (found {}, expected {})",
                cache.version,
                CACHE_VERSION
            ));
        }
        Ok(cache)
    }

    /// Like [`load`](Self::load), but upgrades an older-but-migratable cache in
    /// place instead of rejecting it.
    ///
    /// A bare `load()` returns `Err` on any stale version, which `migrate`
    /// turns into [`new_empty`](Self::new_empty) — and an empty `last` cache
    /// reports nothing as `Removed`, leaving orphaned BUCK targets behind on a
    /// version bump. `migrate` cannot fall back to rebuilding the prior state
    /// from `cargo metadata` (the way add/remove/update do) because there is no
    /// pending manifest edit: current metadata already equals the new state, so
    /// removed packages are simply absent from it.
    ///
    /// The v4→v5 bump only changed how workspace path-source `PackageId`s are
    /// keyed (verbatim absolute path → `($WORKSPACE)`), not the struct layout,
    /// so we can preserve removal detection by re-keying the old fingerprints
    /// with the fixed canonicalizer. Fingerprint *values* are kept as-is; any
    /// that no longer match a freshly computed snapshot just re-emit as
    /// `Changed`, which is safe (and on Windows actively rewrites BUCK files
    /// that had embedded an absolute path).
    pub fn load_migrated(workspace_root: &Utf8PathBuf) -> Result<Self, Error> {
        let cache = Self::read_from_disk()?;
        if cache.version == CACHE_VERSION {
            return Ok(cache);
        }
        if cache.version == 4 {
            return Ok(cache.rekeyed_v4_to_v5(workspace_root));
        }
        Err(anyhow!(
            "Cache version is stale and not migratable (found {}, expected {})",
            cache.version,
            CACHE_VERSION
        ))
    }

    /// Re-key a v4 cache's fingerprints to the v5 `($WORKSPACE)` form. v5 only
    /// changed key canonicalization, so the package set is preserved verbatim —
    /// which is what keeps `diff()` reporting `Removed` across the bump.
    fn rekeyed_v4_to_v5(self, workspace_root: &Utf8PathBuf) -> Self {
        Self {
            fingerprints: self
                .fingerprints
                .into_iter()
                .map(|(id, fp)| (id.canonicalize(workspace_root), fp))
                .collect(),
            version: CACHE_VERSION,
        }
    }

    pub fn save(&self) {
        let cache_path = get_cache_path().unwrap_or_exit();
        let content = toml::to_string_pretty(self).unwrap_or_exit();
        let comment = "# @generated by `cargo buckal`\n# Not intended for manual editing.";
        std::fs::write(cache_path, format!("{}\n{}", comment, content)).unwrap_or_exit();
    }

    pub fn diff(&self, other: &BuckalCache, workspace_root: &Utf8PathBuf) -> BuckalChange {
        let mut _diff = BuckalChange::default();
        for (id, fp) in &self.fingerprints {
            if let Some(other_fp) = other.fingerprints.get(id) {
                if fp != other_fp {
                    _diff
                        .changes
                        .insert(id.resolve(workspace_root), ChangeType::Changed);
                }
            } else {
                // new package added in self
                _diff
                    .changes
                    .insert(id.resolve(workspace_root), ChangeType::Added);
            }
        }
        for id in other.fingerprints.keys() {
            if !self.fingerprints.contains_key(id) {
                // redundant package removed in self
                _diff
                    .changes
                    .insert(id.resolve(workspace_root), ChangeType::Removed);
            }
        }
        _diff
    }
}

#[derive(Debug, Default)]
pub struct BuckalChange {
    pub changes: BTreeMap<PackageId, ChangeType>,
}

#[derive(Debug)]
pub enum ChangeType {
    Added,
    Removed,
    Changed,
}

#[cfg(test)]
mod tests {
    use super::{CACHE_VERSION, PackageIdExt};
    use cargo_metadata::{PackageId, camino::Utf8PathBuf};

    // `cargo metadata` reports `workspace_root` as a *native* filesystem path,
    // but a path-source package id embeds it as the body of a `file://` *URL*:
    // forward slashes throughout, a leading slash before a Windows drive letter,
    // and percent-encoding for characters like spaces. `canonicalize` / `resolve`
    // must bridge the two so `buckal.snap` stores the path-independent
    // `($WORKSPACE)` placeholder. On Unix a simple absolute path happens to equal
    // its URL body, so the bug only surfaces on Windows — except for paths
    // containing characters cargo percent-encodes (`C:\Users\Jane Doe\…`), which
    // break the bare-substring approach on every platform.
    //
    // The shapes below are picked per host because `url::Url::from_file_path`
    // (used by the fix, and by Cargo itself) only accepts a path that is
    // absolute *on the current OS*.

    fn ws() -> Utf8PathBuf {
        if cfg!(windows) {
            Utf8PathBuf::from(r"D:\proj")
        } else {
            Utf8PathBuf::from("/work/proj")
        }
    }

    /// The `path+file://…` prefix Cargo emits for path crates under [`ws`].
    fn ws_prefix() -> &'static str {
        if cfg!(windows) {
            "path+file:///D:/proj"
        } else {
            "path+file:///work/proj"
        }
    }

    fn ws_spaced() -> Utf8PathBuf {
        if cfg!(windows) {
            Utf8PathBuf::from(r"D:\Code\Jane Doe\proj")
        } else {
            Utf8PathBuf::from("/work/Jane Doe/proj")
        }
    }

    fn ws_spaced_prefix() -> &'static str {
        if cfg!(windows) {
            "path+file:///D:/Code/Jane%20Doe/proj"
        } else {
            "path+file:///work/Jane%20Doe/proj"
        }
    }

    #[test]
    fn canonicalize_workspace_internal_path_crate() {
        let id = PackageId {
            repr: format!("{}/crates/foo#0.1.0", ws_prefix()),
        };
        assert_eq!(
            id.canonicalize(&ws()).repr,
            "path+file://($WORKSPACE)/crates/foo#0.1.0",
            "a path crate under the workspace root must be rewritten to ($WORKSPACE)"
        );
    }

    #[test]
    fn resolve_reproduces_cargo_metadata_repr() {
        let canon = PackageId {
            repr: "path+file://($WORKSPACE)/crates/foo#0.1.0".to_string(),
        };
        assert_eq!(
            canon.resolve(&ws()).repr,
            format!("{}/crates/foo#0.1.0", ws_prefix()),
            "resolve must reproduce the exact repr `cargo metadata` emits"
        );
    }

    #[test]
    fn canonicalize_handles_workspace_root_package_itself() {
        let id = PackageId {
            repr: format!("{}#0.7.0", ws_prefix()),
        };
        assert_eq!(
            id.canonicalize(&ws()).repr,
            "path+file://($WORKSPACE)#0.7.0"
        );
    }

    #[test]
    fn canonicalize_handles_percent_encoded_workspace_path() {
        // A workspace under a directory whose name contains a space: Cargo
        // percent-encodes it in the package id, so a literal substring match on
        // the native path fails on every platform.
        let id = PackageId {
            repr: format!("{}/crates/foo#0.1.0", ws_spaced_prefix()),
        };
        assert_eq!(
            id.canonicalize(&ws_spaced()).repr,
            "path+file://($WORKSPACE)/crates/foo#0.1.0"
        );
        let back = PackageId {
            repr: "path+file://($WORKSPACE)/crates/foo#0.1.0".to_string(),
        };
        assert_eq!(
            back.resolve(&ws_spaced()).repr,
            format!("{}/crates/foo#0.1.0", ws_spaced_prefix())
        );
    }

    #[test]
    fn canonicalize_leaves_non_workspace_path_crate_untouched() {
        // A path dep that lives *outside* the workspace must be left alone — the
        // ($WORKSPACE) placeholder can't express it. Guards against over-reach.
        let outside = if cfg!(windows) {
            "path+file:///D:/other/lib/bar#0.1.0"
        } else {
            "path+file:///work/other/lib/bar#0.1.0"
        };
        let id = PackageId {
            repr: outside.to_string(),
        };
        assert_eq!(id.canonicalize(&ws()).repr, id.repr);
    }

    #[test]
    fn canonicalize_leaves_registry_id_untouched() {
        let id = PackageId {
            repr: "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0".to_string(),
        };
        assert_eq!(id.canonicalize(&ws()).repr, id.repr);
    }

    #[test]
    fn canonicalize_does_not_match_sibling_directory() {
        // `<root>` must not be matched as a bare substring of a sibling directory
        // that merely shares a name prefix (`proj` vs `proj_sidecar`).
        let id = PackageId {
            repr: format!("{}_sidecar/lib/bar#0.1.0", ws_prefix()),
        };
        assert_eq!(id.canonicalize(&ws()).repr, id.repr);
    }

    #[test]
    fn cache_version_bumped_for_workspace_pathfmt_change() {
        // v5 is the bump for ($WORKSPACE) path-id canonicalization: pre-v5 caches
        // store the absolute workspace path verbatim (always so on Windows), so an
        // older cache must be rebuilt rather than diffed against the v5 form. Pinned
        // exactly — not `>=` — so a future bump is a deliberate, visible event:
        // when you bump CACHE_VERSION, update this assertion and add the rationale
        // to the CACHE_VERSION doc comment.
        assert_eq!(CACHE_VERSION, 5);
    }

    #[test]
    fn migrating_v4_cache_preserves_removal_detection() {
        use super::{BuckalCache, ChangeType, Fingerprint};
        use std::collections::BTreeMap;

        // A v4 snapshot as written on Windows: workspace path-source ids stored
        // with the absolute path verbatim (the pre-v5 canonicalize no-op).
        let foo_abs = PackageId {
            repr: format!("{}/crates/foo#0.1.0", ws_prefix()),
        };
        let bar_abs = PackageId {
            repr: format!("{}/crates/bar#0.1.0", ws_prefix()),
        };
        let mut v4 = BTreeMap::new();
        v4.insert(foo_abs.clone(), Fingerprint::new([1u8; 32]));
        v4.insert(bar_abs.clone(), Fingerprint::new([2u8; 32]));
        let v4_cache = BuckalCache {
            fingerprints: v4,
            version: 4,
        };

        // The new resolve dropped `bar` from the manifest; its keys are v5-canonical.
        let foo_canon = foo_abs.canonicalize(&ws());
        let mut v5 = BTreeMap::new();
        v5.insert(foo_canon.clone(), Fingerprint::new([1u8; 32]));
        let new_cache = BuckalCache {
            fingerprints: v5,
            version: CACHE_VERSION,
        };

        let migrated = v4_cache.rekeyed_v4_to_v5(&ws());
        let changes = new_cache.diff(&migrated, &ws());

        // `bar` must be reported as Removed. Discarding the stale cache (the old
        // new_empty() path) would silently keep its orphaned BUCK target — the
        // regression Codex flagged.
        let bar_resolved = bar_abs.canonicalize(&ws()).resolve(&ws());
        assert!(
            matches!(
                changes.changes.get(&bar_resolved),
                Some(ChangeType::Removed)
            ),
            "removed workspace crate must be detected as Removed after v4->v5 migration; got {:?}",
            changes.changes
        );
        // `foo` is unchanged (same fingerprint, key now aligned), so the
        // migration introduces no spurious diff entry for it.
        let foo_resolved = foo_canon.resolve(&ws());
        assert!(
            !changes.changes.contains_key(&foo_resolved),
            "unchanged crate must not appear in the diff after migration; got {:?}",
            changes.changes
        );
    }
}
