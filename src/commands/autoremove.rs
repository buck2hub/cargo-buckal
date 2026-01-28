use std::collections::BTreeSet;

use cargo_metadata::MetadataCommand;
use clap::Parser;
use walkdir::WalkDir;

use crate::{
    RUST_CRATES_ROOT, buckal_log, buckal_note,
    utils::{UnwrapOrExit, ensure_prerequisites, get_buck2_root},
};

#[derive(Parser, Debug)]
pub struct AutoremoveArgs {
    /// Don’t actually remove the dependencies
    #[arg(name = "dry-run", long, default_value = "false")]
    pub dry_run: bool,
}

pub fn execute(args: &AutoremoveArgs) {
    ensure_prerequisites().unwrap_or_exit();

    let buck2_root = get_buck2_root().unwrap_or_exit();
    let cargo_metadata = MetadataCommand::new().exec().unwrap();
    let packages_map = cargo_metadata
        .packages
        .into_iter()
        .map(|p| format!("{}/{}", p.name, p.version))
        .collect::<BTreeSet<_>>();

    if args.dry_run {
        buckal_note!("The following packages would be removed:");
    }

    let third_party_dir = buck2_root.join(RUST_CRATES_ROOT);
    for entry in WalkDir::new(&third_party_dir).min_depth(2).max_depth(2) {
        let entry_path = entry.as_ref().unwrap().path();
        let entry_label = entry_path
            .strip_prefix(&third_party_dir)
            .ok()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        if !packages_map.contains(&entry_label) {
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
        for entry in WalkDir::new(&third_party_dir).min_depth(1).max_depth(1) {
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
