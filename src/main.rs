mod assets;
mod buck;
mod buck2;
mod buckify;
mod bundles;
mod cache;
mod cli;
mod commands;
mod config;
mod context;
mod platform;
mod utils;

use std::sync::OnceLock;

use clap::Parser;

pub const RUST_CRATES_ROOT: &str = "third-party/rust/crates";
pub const BUCKAL_BUNDLES_REPO: &str = "buck2hub/buckal-bundles";
// fallback commit hash used when fetching the latest from BUCKAL_BUNDLES_REPO fails
pub const DEFAULT_BUNDLE_HASH: &str = "22bd38c79d2348d9a6591b7156c42d615377eaad";

pub fn main() {
    let args = cli::Cli::parse();
    args.run();
}

pub fn build_version() -> &'static str {
    static VERSION_STRING: OnceLock<String> = OnceLock::new();
    VERSION_STRING.get_or_init(|| {
        let pkg_version = env!("CARGO_PKG_VERSION");
        let is_dev = option_env!("DEV_BUILD").unwrap_or("false") == "true";
        if is_dev {
            let git_hash = option_env!("GIT_HASH").unwrap_or("unknown");
            let commit_date = option_env!("COMMIT_DATE").unwrap_or("unknown");
            format!("{}-dev ({} {})", pkg_version, git_hash, commit_date)
        } else {
            pkg_version.to_string()
        }
    })
}

pub fn user_agent() -> &'static str {
    static USER_AGENT_STRING: OnceLock<String> = OnceLock::new();
    USER_AGENT_STRING.get_or_init(|| {
        let pkg_version = env!("CARGO_PKG_VERSION");
        format!("buckal/{}", pkg_version)
    })
}
