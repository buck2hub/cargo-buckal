use cargo_metadata::Package;
use cargo_util_schemas::core::PackageIdSpec;
use regex::Regex;

use crate::{
    buck::{parse_buck_file, patch_buck_rules},
    buckal_log,
    cache::{BuckalChange, ChangeType},
    context::BuckalContext,
    utils::{UnwrapOrExit, get_buck2_root, get_url_path, get_vendor_dir},
};

use super::{
    buckify_dep_node, buckify_root_node, cross, gen_buck_content, vendor_package, windows,
};

impl BuckalChange {
    pub fn apply(&self, ctx: &BuckalContext) {
        // This function applies changes to the BUCK files of detected packages in the cache diff, but skips the root package.
        let re: Regex = Regex::new(r"^([^+#]+)\+([^#]+)#([^@]+)@([^+#]+)(?:\+(.+))?$")
            .expect("error creating regex");
        let skip_pattern = format!("path+file://{}", ctx.workspace_root);

        for (id, change_type) in &self.changes {
            match change_type {
                ChangeType::Added | ChangeType::Changed => {
                    // Skip root package
                    if let Some(root) = &ctx.root
                        && id == &root.id
                    {
                        continue;
                    }

                    if let Some(node) = ctx.nodes_map.get(id) {
                        let package = ctx.packages_map.get(id).unwrap();

                        buckal_log!(
                            if let ChangeType::Added = change_type {
                                "Adding"
                            } else {
                                "Flushing"
                            },
                            format!("{} v{}", package.name, package.version)
                        );

                        // Vendor package sources
                        let vendor_dir = if !is_third_party(package) {
                            package.manifest_path.parent().unwrap().to_owned()
                        } else {
                            vendor_package(package)
                        };

                        // Generate BUCK rules
                        let mut buck_rules = if !is_third_party(package) {
                            buckify_root_node(node, ctx)
                        } else {
                            buckify_dep_node(node, ctx)
                        };

                        // Patch BUCK Rules
                        let buck_path = vendor_dir.join("BUCK");
                        if buck_path.exists() {
                            // Skip merging manual changes if `--no-merge` is set
                            if !ctx.no_merge && !ctx.repo_config.patch_fields.is_empty() {
                                let existing_rules = parse_buck_file(&buck_path)
                                    .expect("Failed to parse existing BUCK file");
                                patch_buck_rules(
                                    &existing_rules,
                                    &mut buck_rules,
                                    &ctx.repo_config.patch_fields,
                                );
                            }
                        } else {
                            std::fs::File::create(&buck_path).expect("Failed to create BUCK file");
                        }

                        // Generate the BUCK file
                        let mut buck_content = gen_buck_content(&buck_rules);
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
    }
}

pub fn flush_root(ctx: &BuckalContext) {
    // Generate BUCK file for root package
    // Skip if root package is not found (in virtual workspace)
    if let Some(root) = &ctx.root {
        buckal_log!("Flushing", format!("{} v{}", root.name, root.version));
        let root_node = ctx.nodes_map.get(&root.id).expect("Root node not found");

        let manifest_dir = root
            .manifest_path
            .parent()
            .expect("Failed to get manifest directory")
            .to_owned();
        let buck_path = manifest_dir.join("BUCK");

        // Generate BUCK rules
        let buck_rules = buckify_root_node(root_node, ctx);

        // Generate the BUCK file
        let mut buck_content = gen_buck_content(&buck_rules);
        buck_content = windows::patch_root_windows_rustc_flags(buck_content, ctx, root);
        buck_content = cross::patch_rust_test_target_compatible_with(buck_content);
        std::fs::write(&buck_path, buck_content).expect("Failed to write BUCK file");
    }
}

/// Check if a package is a third-party dependency
pub(super) fn is_third_party(package: &Package) -> bool {
    if package.source.is_some() {
        true
    } else {
        let package_id_spec =
            PackageIdSpec::parse(&package.id.repr).unwrap_or_exit_ctx("failed to parse package ID");
        let buck2_root = get_buck2_root().unwrap_or_exit_ctx("failed to get Buck2 root");
        if let Some(url) = package_id_spec.url() {
            let url_path = get_url_path(url);
            url_path.strip_prefix(buck2_root.as_str()).is_none()
        } else {
            // If there's no URL, we treat it as a first-party package
            false
        }
    }
}
