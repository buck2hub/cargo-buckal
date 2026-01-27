use std::fs;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use log::debug;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value, value};

use crate::buckal_log;
use crate::{
    buckify::flush_root,
    cache::BuckalCache,
    context::BuckalContext,
    utils::{UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_last_cache, section},
};

#[derive(Parser, Debug)]
pub struct AddArgs {
    /// Package to add as a dependency
    pub package: String,

    /// Add dependencies in workspace mode
    #[arg(long, short = 'W')]
    pub workspace: bool,

    /// Space or comma separated list of features to activate
    #[arg(long, short = 'F')]
    pub features: Option<String>,

    /// Rename the dependency
    #[arg(long)]
    pub rename: Option<String>,

    /// Add as a development dependency
    #[arg(long, default_value = "false")]
    pub dev: bool,

    /// Add as a build dependency
    #[arg(long, default_value = "false")]
    pub build: bool,
}

pub fn execute(args: &AddArgs) {
    ensure_prerequisites().unwrap_or_exit();

    check_buck2_package().unwrap_or_exit();

    let last_cache = get_last_cache();

    if args.workspace {
        section("Buckal Console");
        handle_workspace_add(args).unwrap_or_exit_ctx("failed to add workspace dependency");
    } else {
        handle_classic_add(args).unwrap_or_exit_ctx("failed to execute cargo add");
        section("Buckal Console");
    }

    debug!("Syncing: Refreshing Cargo metadata...");
    let _ = MetadataCommand::new().exec();

    let ctx = BuckalContext::new();
    flush_root(&ctx);

    let workspace_root = ctx.root.manifest_path.parent().unwrap().to_path_buf();
    let new_cache = BuckalCache::new(&ctx.nodes_map, &workspace_root);
    let changes = new_cache.diff(&last_cache, &workspace_root);

    changes.apply(&ctx);
    new_cache.save();
}

fn handle_classic_add(args: &AddArgs) -> Result<()> {
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.arg("add").arg(&args.package);
    if let Some(features) = &args.features {
        cargo_cmd.arg("--features").arg(features);
    }
    if let Some(rename) = &args.rename {
        cargo_cmd.arg("--rename").arg(rename);
    }
    if args.dev {
        cargo_cmd.arg("--dev");
    }
    if args.build {
        cargo_cmd.arg("--build");
    }

    cargo_cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let status = cargo_cmd.status()?;
    if !status.success() {
        return Err(anyhow!("cargo add exited with failure status"));
    }
    Ok(())
}

fn handle_workspace_add(args: &AddArgs) -> Result<()> {
    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")?;

    let workspace_root = metadata.workspace_root.into_std_path_buf();
    let root_manifest = workspace_root.join("Cargo.toml");
    let current_dir = std::env::current_dir()?;
    let current_manifest = current_dir.join("Cargo.toml");

    let (name_req, version_req) = parse_package_spec(&args.package);
    let dep_key = args.rename.as_deref().unwrap_or(name_req);

    let mut root_doc = fs::read_to_string(&root_manifest)?.parse::<DocumentMut>()?;
    let workspace_table = root_doc["workspace"]
        .as_table_mut()
        .context("Root Cargo.toml missing [workspace] table")?;
    if !workspace_table.contains_key("dependencies") {
        workspace_table.insert("dependencies", Item::Table(Table::new()));
    }
    let ws_deps = workspace_table["dependencies"].as_table_mut().unwrap();

    if let Some(item) = ws_deps.get(dep_key) {
        let current_ver = item
            .as_value()
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        debug!(
            "Skipping Root: {} is already in workspace (v{})",
            dep_key, current_ver
        );
    } else {
        let version_to_write = if let Some(v) = version_req {
            v.to_string()
        } else {
            fetch_latest_version(name_req)?
        };

        buckal_log!("Adding", format!("{} v{}", dep_key, version_to_write));
        ws_deps.insert(dep_key, value(version_to_write));
        fs::write(&root_manifest, root_doc.to_string())?;
    }

    if current_manifest != root_manifest {
        if !current_manifest.exists() {
            debug!("Current directory is not a crate, skipping member update.");
            return Ok(());
        }

        let mut member_doc = fs::read_to_string(&current_manifest)?.parse::<DocumentMut>()?;
        let table_key = if args.dev {
            "dev-dependencies"
        } else if args.build {
            "build-dependencies"
        } else {
            "dependencies"
        };

        let deps_table = member_doc
            .entry(table_key)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .context(format!("Failed to parse [{}]", table_key))?;

        if deps_table.contains_key(dep_key) {
            debug!("Skipping Member: {} is already in {}", dep_key, table_key);
        } else {
            let mut inline_table = InlineTable::new();
            inline_table.insert("workspace", Value::from(true));

            if let Some(features_str) = &args.features {
                let features_list: Array = features_str
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !features_list.is_empty() {
                    inline_table.insert("features", Value::from(features_list));
                }
            }
            if args.rename.is_some() {
                inline_table.insert("package", Value::from(name_req));
            }

            debug!("Adding Member: {} = {{ workspace = true }}", dep_key);
            deps_table.insert(dep_key, value(inline_table));
            fs::write(&current_manifest, member_doc.to_string())?;
        }
    }

    Ok(())
}

fn parse_package_spec(spec: &str) -> (&str, Option<&str>) {
    if let Some((name, ver)) = spec.split_once('@') {
        (name, Some(ver))
    } else {
        (spec, None)
    }
}

fn fetch_latest_version(crate_name: &str) -> Result<String> {
    debug!("Querying: Checking latest version for {}...", crate_name);
    let output = Command::new("cargo")
        .arg("search")
        .arg(crate_name)
        .arg("--limit=1")
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("Failed to search crate version"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if !line.starts_with(crate_name) {
            continue;
        }

        let Some(start) = line.find('"') else {
            continue;
        };
        let Some(end) = line[start + 1..].find('"') else {
            continue;
        };

        return Ok(line[start + 1..start + 1 + end].to_string());
    }
    Err(anyhow!(
        "Could not determine latest version for {}",
        crate_name
    ))
}
