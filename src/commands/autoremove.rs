use std::collections::BTreeSet;

use anyhow::Result;
use cargo_metadata::MetadataCommand;
use clap::Parser;
use walkdir::WalkDir;

use crate::{
    RUST_ROOT, buckal_log, buckal_note,
    utils::{UnwrapOrExit, ensure_prerequisites, get_buck2_root},
};

#[derive(Parser, Debug)]
pub struct AutoremoveArgs {
    /// Don’t actually remove the dependencies
    #[arg(name = "dry-run", long, default_value = "false")]
    pub dry_run: bool,
    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: Option<String>,
}

pub fn execute(args: &AutoremoveArgs) {
    ensure_prerequisites().unwrap_or_exit();

    let buck2_root = get_buck2_root().unwrap_or_exit();
    let cargo_metadata = if let Some(manifest) = &args.manifest_path {
        MetadataCommand::new()
            .manifest_path(manifest)
            .exec()
            .unwrap()
    } else {
        MetadataCommand::new().exec().unwrap()
    };
    let used_packages = cargo_metadata
        .packages
        .into_iter()
        .map(|p| {
            format!(
                "{}/{}/{}",
                extract_kind_str(&p.id.repr).unwrap_or_exit(),
                p.name,
                p.version
            )
        })
        .collect::<BTreeSet<_>>();

    if args.dry_run {
        buckal_note!("The following packages would be removed:");
    }

    let third_party_dir = buck2_root.join(RUST_ROOT);
    for entry in WalkDir::new(&third_party_dir).min_depth(3).max_depth(3) {
        let entry_path = entry.as_ref().unwrap().path();
        let entry_label = entry_path
            .strip_prefix(&third_party_dir)
            .ok()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        if !used_packages.contains(&entry_label) {
            let entry_display = entry_path
                .strip_prefix(&buck2_root)
                .ok()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if args.dry_run {
                println!("  {}", entry_display);
            } else {
                buckal_log!("Removing", format!("{}", entry_display));
                std::fs::remove_dir_all(entry.as_ref().unwrap().path()).unwrap_or_exit();
            }
        }
    }

    if !args.dry_run {
        for entry in WalkDir::new(&third_party_dir).min_depth(2).max_depth(2) {
            let is_empty = entry
                .as_ref()
                .unwrap()
                .path()
                .read_dir()
                .unwrap_or_exit()
                .next()
                .is_none();
            if is_empty {
                std::fs::remove_dir_all(entry.as_ref().unwrap().path()).unwrap_or_exit();
            }
        }
    }
}

/// Extract the source kind (registry or git) from a package ID string
fn extract_kind_str(string: &str) -> Result<&str> {
    let (kind, _) = string
        .split_once('+')
        .ok_or_else(|| anyhow::format_err!("invalid source `{}`", string))?;
    match kind {
        "registry" => Ok("crates"),
        _ => Ok(kind),
    }
}
