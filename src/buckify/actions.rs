use std::{
    collections::BTreeMap,
    io::{BufWriter, Write},
};

use regex::Regex;

use cargo_metadata::camino::Utf8PathBuf;

use crate::{
    RUST_CRATES_ROOT,
    buck::{Alias, parse_buck_file, patch_buck_rules},
    buckal_log,
    cache::{BuckalChange, ChangeType},
    context::BuckalContext,
    utils::{UnwrapOrExit, get_buck2_root, get_vendor_dir},
};

use super::{
    buckify_dep_node, buckify_root_node, cross, gen_buck_content, vendor_package, windows,
};

impl BuckalChange {
    pub fn apply(&self, ctx: &BuckalContext) {
        // This function applies changes to the BUCK files of detected packages in the cache diff, but skips the root package.
        let re = Regex::new(r"^([^+#]+)\+([^#]+)#([^@]+)@([^+#]+)(?:\+(.+))?$")
            .expect("error creating regex");
        let skip_pattern = format!("path+file://{}", ctx.workspace_root);

        for (id, change_type) in &self.changes {
            match change_type {
                ChangeType::Added | ChangeType::Changed => {
                    // Skip root package
                    if id == &ctx.root.id {
                        continue;
                    }

                    if let Some(node) = ctx.nodes_map.get(id) {
                        let package = ctx.packages_map.get(id).unwrap();

                        if ctx.separate && package.source.is_none() {
                            // Skip first-party packages if `--separate` is set
                            continue;
                        }

                        buckal_log!(
                            if let ChangeType::Added = change_type {
                                "Adding"
                            } else {
                                "Flushing"
                            },
                            format!("{} v{}", package.name, package.version)
                        );

                        // Vendor package sources
                        let vendor_dir = if package.source.is_none() {
                            package.manifest_path.parent().unwrap().to_owned()
                        } else {
                            vendor_package(package)
                        };

                        // Generate BUCK rules
                        let mut buck_rules = if package.source.is_none() {
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
                    let vendor_dir = get_vendor_dir(name, version)
                        .unwrap_or_exit_ctx("failed to get vendor directory");
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
    buckal_log!(
        "Flushing",
        format!("{} v{}", ctx.root.name, ctx.root.version)
    );
    let root_node = ctx
        .nodes_map
        .get(&ctx.root.id)
        .expect("Root node not found");

    if ctx.repo_config.inherit_workspace_deps {
        buckal_log!(
            "Generating",
            "third-party alias rules (inherit_workspace_deps=true)"
        );
        generate_third_party_aliases(ctx);
    }

    let cwd = std::env::current_dir().expect("Failed to get current directory");
    let buck_path = Utf8PathBuf::from(cwd.to_str().unwrap()).join("BUCK");

    // Generate BUCK rules
    let buck_rules = buckify_root_node(root_node, ctx);

    // Generate the BUCK file
    let mut buck_content = gen_buck_content(&buck_rules);
    buck_content = windows::patch_root_windows_rustc_flags(buck_content, ctx);
    buck_content = cross::patch_rust_test_target_compatible_with(buck_content);
    std::fs::write(&buck_path, buck_content).expect("Failed to write BUCK file");
}

fn generate_third_party_aliases(ctx: &BuckalContext) {
    let root = get_buck2_root().expect("failed to get buck2 root");
    let dir = root.join("third-party/rust");
    std::fs::create_dir_all(&dir).expect("failed to create third-party/rust dir");

    let buck_file = dir.join("BUCK");

    let mut grouped: BTreeMap<String, Vec<&cargo_metadata::Package>> = BTreeMap::new();

    for (pkg_id, pkg) in &ctx.packages_map {
        // only workspace members (first-party)
        if pkg.source.is_some() {
            continue;
        }

        let node = match ctx.nodes_map.get(pkg_id) {
            Some(n) => n,
            None => continue,
        };

        for dep in &node.deps {
            let dep_pkg = ctx.packages_map.get(&dep.pkg).unwrap();
            if dep_pkg.source.is_some() {
                grouped
                    .entry(dep_pkg.name.to_string())
                    .or_default()
                    .push(dep_pkg);
            }
        }
    }

    let file = std::fs::File::create(&buck_file).expect("failed to create third-party/rust/BUCK");
    let mut writer = BufWriter::new(file);

    writeln!(writer, "# @generated by cargo-buckal\n").expect("failed to write header");

    for (crate_name, mut versions) in grouped {
        versions.sort_by(|a, b| a.version.cmp(&b.version));
        let latest = versions.last().expect("empty version list");

        let actual = format!(
            "//{RUST_CRATES_ROOT}/{}/{}:{}",
            crate_name, latest.version, crate_name
        );

        let rule = Alias {
            name: crate_name.clone(),
            actual,
            visibility: ["PUBLIC"].into_iter().map(String::from).collect(),
        };
        let rendered = serde_starlark::to_string(&rule).expect("failed to serialize alias");
        writeln!(writer, "{}", rendered).expect("write failed");
    }

    writer.flush().expect("failed to flush alias rules");

    buckal_log!(
        "Generated",
        format!("third-party alias rules at {}", buck_file)
    );
}
