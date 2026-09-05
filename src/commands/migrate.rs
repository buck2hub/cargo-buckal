use std::path::PathBuf;

use clap::Parser;

use crate::{
    RUST_CRATES_ROOT, RUST_GIT_ROOT,
    assets::extract_buck2_assets,
    buck2::Buck2Command,
    buckal_error,
    bundles::{fetch_buckal_cell, init_buckal_cell, init_modifier},
    cache::BuckalCache,
    context::BuckalContext,
    utils::{UnwrapOrExit, append_buck_out_to_gitignore, ensure_prerequisites, get_buck2_root},
};

#[derive(Parser, Debug)]
pub struct MigrateArgs {
    /// Do not use cached data from previous runs
    #[clap(long, name = "no-cache")]
    pub no_cache: bool,

    /// Merge manual edits with generated content
    #[clap(long)]
    pub merge: bool,

    /// Initialize Buck2 in the specified directory (defaults to current directory)
    #[clap(long, value_name = "PATH", default_missing_value = ".", num_args = 0..=1, conflicts_with = "fetch")]
    pub init: Option<PathBuf>,

    /// Fetch latest bundles from remote repository
    #[clap(long)]
    pub fetch: bool,

    /// Path to Cargo.toml
    #[arg(long, conflicts_with = "init")]
    pub manifest_path: Option<String>,
}

pub fn execute(args: &MigrateArgs) {
    // Ensure all prerequisites are installed before proceeding
    ensure_prerequisites().unwrap_or_exit();

    // Initialize Buck2 project if requested
    // Compared to `cargo buckal init`, here we only setup Buck2 related files
    if let Some(init_path) = &args.init {
        let cwd = std::env::current_dir().unwrap_or_exit();
        // Resolve and canonicalize the init path
        let init_root = std::fs::canonicalize(init_path).unwrap_or_exit_ctx(format!(
            "failed to resolve init path `{}`",
            init_path.display()
        ));

        let existing_root = get_buck2_root().ok();
        let root_for_checks = existing_root
            .as_ref()
            .map(|root| root.as_std_path())
            .unwrap_or_else(|| init_root.as_path());
        let toolchains_dir = root_for_checks.join("toolchains");
        let platforms_dir = root_for_checks.join("platforms");
        if toolchains_dir.is_dir() || platforms_dir.is_dir() {
            buckal_error!(
                "`toolchains/` or `platforms/` directory already exists under `{}`. Please delete them first.",
                root_for_checks.display()
            );
            std::process::exit(1);
        }

        // Change to init_root directory for Buck2 initialization
        std::env::set_current_dir(&init_root).unwrap_or_exit_ctx(format!(
            "failed to change directory to `{}`",
            init_root.display()
        ));

        Buck2Command::init().execute().unwrap_or_exit();

        // Restore original directory
        std::env::set_current_dir(&cwd).unwrap_or_exit_ctx(format!(
            "failed to change directory back to `{}`",
            cwd.display()
        ));

        let buck2_root = existing_root.unwrap_or_else(|| {
            get_buck2_root().unwrap_or_exit_ctx("failed to get Buck2 project root")
        });

        let crates_dir = buck2_root.join(RUST_CRATES_ROOT);
        std::fs::create_dir_all(&crates_dir)
            .unwrap_or_exit_ctx(format!("failed to create directory at `{}`", crates_dir));
        let git_dir = buck2_root.join(RUST_GIT_ROOT);
        std::fs::create_dir_all(&git_dir)
            .unwrap_or_exit_ctx(format!("failed to create directory at `{}`", git_dir));

        append_buck_out_to_gitignore(buck2_root.as_std_path())
            .unwrap_or_exit_ctx("failed to update `.gitignore`");

        // Configure the buckal cell in .buckconfig
        init_buckal_cell(buck2_root.as_std_path()).unwrap_or_exit();

        extract_buck2_assets(buck2_root.as_std_path())
            .unwrap_or_exit_ctx("failed to extract buck2 assets");

        // Init cfg modifiers
        init_modifier(buck2_root.as_std_path()).unwrap_or_exit();
    }

    // Fetch latest bundles if requested
    if args.fetch {
        let cwd = std::env::current_dir().unwrap_or_exit();
        fetch_buckal_cell(&cwd).unwrap_or_exit();
    }

    // get cargo metadata and generate context
    let mut ctx = BuckalContext::new(args.manifest_path.clone());
    ctx.no_merge = !args.merge;

    // Process dep nodes
    // For migrate, a missing cache means "first run" — use empty so everything is
    // treated as Added and BUCK files are generated from scratch. A stale-but-
    // migratable cache (e.g. a v4 snapshot after the v5 path-id bump) is upgraded
    // in place rather than discarded, so packages dropped from the manifest are
    // still detected as Removed. This differs from add/remove/update, which use
    // get_last_cache() to rebuild the prior state from metadata before the edit.
    let last_cache = if args.no_cache {
        BuckalCache::new_empty()
    } else {
        BuckalCache::load_migrated(&ctx.workspace_root).unwrap_or_else(|_| BuckalCache::new_empty())
    };
    let new_cache =
        BuckalCache::from_resolve(&ctx.resolve, &ctx.workspace_root, &ctx.repo_config.patch);
    let changes = new_cache.diff(&last_cache, &ctx.workspace_root);

    // Apply changes to BUCK files
    changes.apply(&ctx);

    // Flush the new cache
    new_cache.save();
}
