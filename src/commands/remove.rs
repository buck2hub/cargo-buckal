use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use log::debug;
use toml_edit::DocumentMut;

use crate::buckal_log;
use crate::{
    buckify::flush_root,
    cache::BuckalCache,
    context::BuckalContext,
    utils::{UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_last_cache, section},
};

#[derive(Parser, Debug)]
pub struct RemoveArgs {
    /// Package(s) to remove
    #[clap(value_name = "DEP_ID", num_args = 1..)]
    pub packages: Vec<String>,

    /// Remove dependencies in workspace mode
    #[arg(long, short = 'W')]
    pub workspace: bool,

    /// Remove from dev-dependencies
    #[arg(long, default_value = "false")]
    pub dev: bool,

    /// Remove from build-dependencies
    #[arg(long, default_value = "false")]
    pub build: bool,

    /// Path to Cargo.toml
    #[arg(long, conflicts_with = "workspace")]
    pub manifest_path: Option<String>,
}

pub fn execute(args: &RemoveArgs) {
    ensure_prerequisites().unwrap_or_exit();

    check_buck2_package().unwrap_or_exit();

    let last_cache = get_last_cache();

    if args.workspace {
        section("Buckal Console");
        handle_workspace_remove(args).unwrap_or_exit_ctx("failed to remove workspace dependency");
    } else {
        handle_classic_remove(args).unwrap_or_exit_ctx("failed to execute cargo remove");
        section("Buckal Console");
    }

    // Refresh Cargo metadata
    if let Some(manifest) = &args.manifest_path {
        let _ = MetadataCommand::new().manifest_path(manifest).exec();
    } else {
        let _ = MetadataCommand::new().exec();
    }

    let ctx = BuckalContext::new(args.manifest_path.clone());
    flush_root(&ctx);

    let new_cache = BuckalCache::new(&ctx.nodes_map, &ctx.workspace_root);
    let changes = new_cache.diff(&last_cache, &ctx.workspace_root);

    changes.apply(&ctx);
    new_cache.save();
}

fn handle_classic_remove(args: &RemoveArgs) -> Result<()> {
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .arg("remove")
        .args(&args.packages)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if args.dev {
        cargo_cmd.arg("--dev");
    }
    if args.build {
        cargo_cmd.arg("--build");
    }
    if let Some(manifest) = &args.manifest_path {
        cargo_cmd.arg("--manifest-path").arg(manifest);
    }

    let status = cargo_cmd
        .status()
        .context("failed to execute `cargo remove`")?;

    if !status.success() {
        return Err(anyhow!("cargo remove exited with failure status"));
    }
    Ok(())
}

fn handle_workspace_remove(args: &RemoveArgs) -> Result<()> {
    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")?;

    let workspace_root = metadata.workspace_root.into_std_path_buf();
    let root_manifest = workspace_root.join("Cargo.toml");
    let current_dir = std::env::current_dir()?;
    let current_manifest = current_dir.join("Cargo.toml");

    if !current_manifest.exists() {
        return Err(anyhow!("Current directory does not contain a Cargo.toml"));
    }

    let mut member_doc = fs::read_to_string(&current_manifest)?.parse::<DocumentMut>()?;
    let mut root_doc = fs::read_to_string(&root_manifest)?.parse::<DocumentMut>()?;

    let table_key = if args.dev {
        "dev-dependencies"
    } else if args.build {
        "build-dependencies"
    } else {
        "dependencies"
    };

    let mut member_modified = false;
    let mut root_modified = false;

    let other_members: Vec<PathBuf> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| metadata.packages.iter().find(|p| &p.id == id))
        .map(|p| p.manifest_path.clone().into_std_path_buf())
        .filter(|path| path != &current_manifest)
        .collect();

    for pkg in &args.packages {
        let was_removed_from_member = remove_dependency_from_table(&mut member_doc, table_key, pkg);

        if was_removed_from_member {
            buckal_log!("Removing", format!("Member: {} (from {})", pkg, table_key));
            member_modified = true;

            if !is_used_by_any_member(&other_members, pkg)? {
                if remove_dependency_from_root(&mut root_doc, pkg) {
                    buckal_log!("Removing", format!("Root: {} (unused in workspace)", pkg));
                    root_modified = true;
                } else {
                    debug!(
                        "Skipping Root: {} not found in [workspace.dependencies]",
                        pkg
                    );
                }
            } else {
                buckal_log!("Keeping", format!("Root: {} (used by other members)", pkg));
            }
        } else {
            buckal_log!(
                "Unchanged",
                format!("Member: {} not found in {}", pkg, table_key)
            );
        }
    }

    if member_modified {
        fs::write(&current_manifest, member_doc.to_string())?;
    }
    if root_modified {
        fs::write(&root_manifest, root_doc.to_string())?;
    }

    Ok(())
}

fn remove_dependency_from_table(doc: &mut DocumentMut, table_name: &str, pkg_name: &str) -> bool {
    let Some(table) = doc.get_mut(table_name).and_then(|t| t.as_table_mut()) else {
        return false;
    };
    table.remove(pkg_name).is_some()
}

fn remove_dependency_from_root(doc: &mut DocumentMut, pkg_name: &str) -> bool {
    let Some(ws) = doc.get_mut("workspace").and_then(|t| t.as_table_mut()) else {
        return false;
    };
    let Some(deps) = ws.get_mut("dependencies").and_then(|t| t.as_table_mut()) else {
        return false;
    };
    deps.remove(pkg_name).is_some()
}

fn is_used_by_any_member(member_paths: &[PathBuf], pkg_name: &str) -> Result<bool> {
    debug!(
        "Scanning {} other workspace members for usage of '{}'...",
        member_paths.len(),
        pkg_name
    );

    for path in member_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read member manifest: {:?}", path))?;
        let doc = content.parse::<DocumentMut>()?;

        let tables_to_check = ["dependencies", "dev-dependencies", "build-dependencies"];

        for table_key in tables_to_check {
            let Some(table) = doc.get(table_key).and_then(|i| i.as_table()) else {
                continue;
            };

            if table.contains_key(pkg_name) {
                debug!("Found usage in {:?} [{}]", path, table_key);
                return Ok(true);
            }
        }
    }

    Ok(false)
}
