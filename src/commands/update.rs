use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use log::debug;

use crate::{
    buckify::flush_root,
    cache::BuckalCache,
    context::BuckalContext,
    utils::{UnwrapOrExit, check_buck2_package, ensure_prerequisites, get_last_cache, section},
};

#[derive(Parser, Debug)]
pub struct UpdateArgs {
    /// Package to update
    #[clap(value_name = "SPEC", num_args = 0..)]
    pub packages: Vec<String>,

    /// Only update the workspace packages
    #[arg(long, short = 'w')]
    pub workspace: bool,

    /// Don't actually write the lockfile
    #[arg(long)]
    pub dry_run: bool,
}

pub fn execute(args: &UpdateArgs) {
    ensure_prerequisites().unwrap_or_exit();

    check_buck2_package().unwrap_or_exit();

    let last_cache = get_last_cache();

    handle_cargo_update(args).unwrap_or_exit_ctx("failed to execute cargo update");

    if args.dry_run {
        return;
    }

    section("Buckal Console");

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

fn handle_cargo_update(args: &UpdateArgs) -> Result<()> {
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.arg("update");

    if args.workspace {
        cargo_cmd.arg("--workspace");
    }

    if args.dry_run {
        cargo_cmd.arg("--dry-run");
    }

    cargo_cmd.args(&args.packages);

    cargo_cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cargo_cmd
        .status()
        .context("failed to execute `cargo update`")?;

    if !status.success() {
        return Err(anyhow!("cargo update exited with failure status"));
    }
    Ok(())
}
