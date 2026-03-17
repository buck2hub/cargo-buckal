use cargo_metadata::camino::Utf8Path;
use regex::Regex;

use crate::{
    buck::{Rule, parse_buck_file, patch_buck_rules},
    buckal_log,
    buckify::emit::emit_export_file,
    cache::{BuckalChange, ChangeType},
    context::BuckalContext,
    resolve::{BuckalNode, NodeKind},
    utils::{UnwrapOrExit, get_vendor_dir},
};

use super::{
    buckify_dep_node, buckify_root_node, cross, gen_buck_content, vendor_package, windows,
};

impl BuckalChange {
    /// Apply the changes to the BUCK files based on the detected package changes in the cache diff.
    pub fn apply(&self, ctx: &BuckalContext) {
        let re: Regex = Regex::new(r"^([^+#]+)\+([^#]+)#([^@]+)@([^+#]+)(?:\+(.+))?$")
            .expect("error creating regex");
        let skip_pattern = format!("path+file://{}", ctx.workspace_root);

        let mut workspace_emitted = false;

        for (id, change_type) in &self.changes {
            match change_type {
                ChangeType::Added | ChangeType::Changed => {
                    if let Some(node) = ctx.resolve.nodes().find(|n| &n.package_id == id) {
                        buckal_log!(
                            if let ChangeType::Added = change_type {
                                "Adding"
                            } else {
                                "Flushing"
                            },
                            format!("{} v{}", node.name, node.version)
                        );

                        let is_third_party_pkg = is_third_party(node);

                        // Vendor package sources
                        let vendor_dir = if !is_third_party_pkg {
                            node.manifest_path.parent().unwrap().to_owned()
                        } else {
                            vendor_package(node)
                        };

                        // Generate BUCK rules
                        let mut buck_rules = if !is_third_party_pkg {
                            buckify_root_node(node, ctx)
                        } else {
                            buckify_dep_node(node, ctx)
                        };

                        // Export workspace manifest
                        let workspace_manifest_path = ctx.workspace_root.join("Cargo.toml");
                        if ctx.workspace_inherit
                            && !workspace_emitted
                            && node.manifest_path == workspace_manifest_path
                        {
                            buck_rules.push(Rule::ExportFile(emit_export_file()));
                            workspace_emitted = true;
                        }

                        // Patch BUCK Rules
                        let buck_path = vendor_dir.join("BUCK");
                        merge_rules(&buck_path, &mut buck_rules, ctx);

                        // Generate the BUCK file
                        let mut buck_content = gen_buck_content(&buck_rules);
                        if !is_third_party_pkg {
                            buck_content =
                                windows::patch_root_windows_rustc_flags(buck_content, ctx, node);
                        }
                        buck_content = cross::patch_rust_test_target_compatible_with(buck_content);
                        std::fs::write(&buck_path, buck_content)
                            .expect("Failed to write BUCK file");
                    }
                }
                ChangeType::Removed => {
                    // Skip workspace_root package
                    if id.repr.starts_with(skip_pattern.as_str()) {
                        continue;
                    }

                    let caps = re.captures(&id.repr).expect("Failed to parse package ID");
                    let name = &caps[3];
                    let version = &caps[4];

                    buckal_log!("Removing", format!("{} v{}", name, version));
                    let vendor_dir =
                        get_vendor_dir(id).unwrap_or_exit_ctx("failed to get vendor directory");
                    if vendor_dir.exists() {
                        std::fs::remove_dir_all(&vendor_dir)
                            .expect("Failed to remove vendor directory");
                    }
                    if let Some(package_dir) = vendor_dir.parent()
                        && package_dir.exists()
                        && package_dir.read_dir().unwrap().next().is_none()
                    {
                        std::fs::remove_dir_all(package_dir)
                            .expect("Failed to remove empty package directory");
                    }
                }
            }
        }

        // Export workspace manifest for virtual workspace
        if !workspace_emitted && ctx.workspace_inherit {
            let buck_path = ctx.workspace_root.join("BUCK");
            let mut rules = if buck_path.exists() {
                parse_buck_file(&buck_path)
                    .unwrap_or_exit_ctx(format!("Failed to parse {}", buck_path))
                    .values()
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let export_file = Rule::ExportFile(emit_export_file());
            if !rules.contains(&export_file) {
                rules.push(export_file);
            }
            let buck_content = gen_buck_content(&rules);
            std::fs::write(&buck_path, buck_content).expect("Failed to write BUCK file");
        }
    }
}

/// Check if a node represents a third-party dependency
pub(super) fn is_third_party(node: &BuckalNode) -> bool {
    matches!(node.kind, NodeKind::ThirdParty)
}

/// Merge existing BUCK rules with new ones, preserving manual changes in specified fields.
fn merge_rules(buck_path: &Utf8Path, buck_rules: &mut [Rule], ctx: &BuckalContext) {
    if buck_path.exists() {
        // Skip merging manual changes if `--no-merge` is set
        if !ctx.no_merge && !ctx.repo_config.patch_fields.is_empty() {
            let existing_rules = parse_buck_file(buck_path)
                .unwrap_or_exit_ctx(format!("Failed to parse {}", buck_path));
            patch_buck_rules(&existing_rules, buck_rules, &ctx.repo_config.patch_fields);
        }
    } else {
        std::fs::File::create(buck_path)
            .unwrap_or_exit_ctx(format!("Failed to create {}", buck_path));
    }
}
