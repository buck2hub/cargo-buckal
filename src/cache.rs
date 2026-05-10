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
///
/// Migration strategy:
/// - If found < expected (stale cache from older Buckal): ignore the old cache and rebuild.
/// - If found > expected (cache from newer Buckal): exit immediately and prompt the user to upgrade.
const CACHE_VERSION: u32 = 4;

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

pub trait PackageIdExt {
    /// ($WORKSPACE) → workspace_root
    fn resolve(&self, workspace_root: &Utf8PathBuf) -> Self;

    /// workspace_root → ($WORKSPACE)
    fn canonicalize(&self, workspace_root: &Utf8PathBuf) -> Self;
}

impl PackageIdExt for PackageId {
    fn resolve(&self, workspace_root: &Utf8PathBuf) -> Self {
        if self.repr.starts_with("path+file://($WORKSPACE)") {
            PackageId {
                repr: self
                    .repr
                    .clone()
                    .replace("($WORKSPACE)", workspace_root.as_str()),
            }
        } else {
            self.clone()
        }
    }

    fn canonicalize(&self, workspace_root: &Utf8PathBuf) -> Self {
        if self
            .repr
            .starts_with(format!("path+file://{}", workspace_root.as_str()).as_str())
        {
            PackageId {
                repr: self
                    .repr
                    .clone()
                    .replace(workspace_root.as_str(), "($WORKSPACE)"),
            }
        } else {
            self.clone()
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

    pub fn load() -> Result<Self, Error> {
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
        if cache.version < CACHE_VERSION {
            return Err(anyhow!(
                "Cache version is stale (found {}, expected {})",
                cache.version,
                CACHE_VERSION
            ));
        }
        Ok(cache)
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
        // Pre-v4 caches store absolute path-crate keys (verbatim on Windows); v4
        // stores the ($WORKSPACE) form. The version bump forces a rebuild instead
        // of a misdiff between the two representations on first run after upgrade.
        assert!(
            CACHE_VERSION >= 4,
            "CACHE_VERSION must be bumped when the workspace path-id format changes"
        );
    }
}
