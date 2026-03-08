use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio, exit},
};

use clap::Parser;

use crate::{
    RUST_CRATES_ROOT, RUST_GIT_ROOT,
    assets::extract_buck2_assets,
    buck2::Buck2Command,
    buckal_error, buckal_log, buckal_note,
    bundles::{init_buckal_cell, init_modifier},
    utils::{
        UnwrapOrExit, append_buck_out_to_gitignore, ensure_prerequisites, find_buck2_project_root,
    },
};

#[derive(Parser, Debug)]
pub struct NewArgs {
    /// Path to create the new package
    pub path: String,
    /// Use a binary (application) template [default]
    #[arg(long, default_value = "false")]
    pub bin: bool,
    /// Use a library template
    #[arg(long, default_value = "false")]
    pub lib: bool,
    /// Specify the Rust edition to use
    #[arg(long)]
    pub edition: Option<String>,
    /// Set the package name
    #[arg(long)]
    pub name: Option<String>,
    /// Create only a Buck2 project without Cargo initialization
    #[arg(long, default_value = "false", conflicts_with_all = ["bin", "lib", "edition", "name"])]
    pub repo: bool,
    /// Set up a Buck2 project with a simple package
    #[arg(long, default_value = "false", conflicts_with = "repo")]
    pub lite: bool,
}

pub fn execute(args: &NewArgs) {
    // Ensure all prerequisites are installed before proceeding
    ensure_prerequisites().unwrap_or_exit();

    if !args.repo && !args.lite {
        ensure_new_path_within_buck2_project(&args.path).unwrap_or_exit();
    }

    // Use `cargo new` to initialize the directory
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.arg("new").arg(&args.path);
    if args.bin {
        cargo_cmd.arg("--bin");
    }
    if args.lib {
        cargo_cmd.arg("--lib");
    }
    if let Some(edition) = &args.edition {
        cargo_cmd.arg("--edition").arg(edition);
    }
    if let Some(name) = &args.name {
        cargo_cmd.arg("--name").arg(name);
    }

    // Suppress output if `--repo` is set
    if args.repo {
        cargo_cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    // execute the cargo command
    let status = cargo_cmd
        .status()
        .unwrap_or_exit_ctx("failed to execute `cargo new`");
    if !status.success() {
        buckal_error!("failed to initialize directory");
        exit(1);
    }

    // If `--repo` is set, remove the generated `src` directory and `Cargo.toml`
    if args.repo {
        buckal_log!(
            "Creating",
            format!("buck2 repository named `{}`", args.path)
        );
        std::fs::remove_dir_all(format!("{}/src", args.path)).unwrap_or_exit();
        std::fs::remove_file(format!("{}/Cargo.toml", args.path)).unwrap_or_exit();
    }

    if args.repo || args.lite {
        // Init a new buck2 repo
        Buck2Command::init()
            .arg(&args.path)
            .execute()
            .unwrap_or_exit();
        std::fs::create_dir_all(format!("{}/{}", args.path, RUST_CRATES_ROOT))
            .unwrap_or_exit_ctx("failed to create third-party directory");
        std::fs::create_dir_all(format!("{}/{}", args.path, RUST_GIT_ROOT))
            .unwrap_or_exit_ctx("failed to create third-party directory");
        append_buck_out_to_gitignore(Path::new(&args.path))
            .unwrap_or_exit_ctx("failed to update `.gitignore`");

        // Configure the buckal cell in .buckconfig
        let cwd = std::env::current_dir().unwrap_or_exit();
        let repo_path = cwd.join(&args.path);
        init_buckal_cell(&repo_path).unwrap_or_exit();

        extract_buck2_assets(&repo_path).unwrap_or_exit_ctx("failed to extract buck2 assets");

        // Init cfg modifiers
        init_modifier(&repo_path).unwrap_or_exit();
    } else {
        // Create a new buck2 cell
        let _buck = std::fs::File::create(format!("{}/BUCK", args.path))
            .unwrap_or_exit_ctx("failed to create `BUCK` file");
    }

    if args.repo {
        buckal_note!(
            "You should manually configure a Cargo workspace before running `cargo buckal new <path>` to create packages."
        );
    }
}

fn ensure_new_path_within_buck2_project(path: &str) -> std::io::Result<()> {
    let cwd = std::env::current_dir()?;
    let absolute_target = absolutize_path(path, &cwd);
    let probe_path = if absolute_target.exists() {
        absolute_target.as_path()
    } else {
        absolute_target.parent().unwrap_or(cwd.as_path())
    };

    if find_buck2_project_root(probe_path).is_some() {
        return Ok(());
    }

    Err(std::io::Error::other(format!(
        "No Buck2 project root (.buckconfig) found for `{}`. \
Run `cargo buckal new {} --repo` (or `--lite`) to initialize a project first.",
        probe_path.display(),
        path
    )))
}

fn absolutize_path(path: &str, cwd: &Path) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::ensure_new_path_within_buck2_project;
    use tempfile::TempDir;

    #[test]
    fn test_ensure_new_path_within_buck2_project_accepts_buckconfig_ancestor() {
        let root = TempDir::new().expect("failed to create temp dir");
        let package = root.path().join("crates").join("demo");
        std::fs::write(root.path().join(".buckconfig"), "[project]\nignore=.git\n")
            .expect("failed to write .buckconfig");

        let result = ensure_new_path_within_buck2_project(package.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_new_path_within_buck2_project_rejects_missing_buckconfig() {
        let root = TempDir::new().expect("failed to create temp dir");
        let package = root.path().join("crates").join("demo");
        std::fs::create_dir_all(root.path().join("crates")).expect("failed to create parent dir");

        let result = ensure_new_path_within_buck2_project(package.to_str().unwrap());
        assert!(result.is_err());
    }
}
