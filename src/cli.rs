use clap::Parser;

use crate::{build_version, commands};

#[derive(Parser, Debug)]
#[command(bin_name = "cargo")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub enum Commands {
    #[command(
        about = "A cargo plugin for Buck2 integration",
        long_about = "Seamlessly build Cargo projects with Buck2 — the simpler alternative to Reindeer"
    )]
    Buckal(BuckalArgs),
}

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct BuckalArgs {
    #[command(subcommand)]
    pub subcommands: Option<BuckalSubCommands>,
    #[arg(long, short = 'V', help = "Print version")]
    pub version: bool,
}

#[derive(Parser, Debug)]
pub enum BuckalSubCommands {
    /// Add dependencies to a manifest file
    Add(crate::commands::add::AddArgs),

    /// Automatically remove unused dependencies
    Autoremove(crate::commands::autoremove::AutoremoveArgs),

    /// Compile the current package
    Build(crate::commands::build::BuildArgs),

    /// Remove generated artifacts
    Clean(crate::commands::clean::CleanArgs),

    /// Create a new package in an existing directory
    Init(crate::commands::init::InitArgs),

    /// Log in to a registry
    Login(crate::commands::login::LoginArgs),

    /// Remove a token from the registry locally
    Logout(crate::commands::logout::LogoutArgs),

    /// Migrate existing Cargo packages to Buck2
    Migrate(crate::commands::migrate::MigrateArgs),

    /// Create a new package
    New(crate::commands::new::NewArgs),

    /// Push third-party BUCK files to a registry
    Push(crate::commands::push::PushArgs),

    /// Remove dependencies from a manifest file
    Remove(crate::commands::remove::RemoveArgs),

    /// Execute unit and integration tests of a package
    Test(Box<crate::commands::test::TestArgs>),

    /// Update dependencies in a manifest file
    Update(crate::commands::update::UpdateArgs),
}

impl Cli {
    pub fn run(&self) {
        match &self.command {
            Commands::Buckal(args) => {
                if args.version {
                    println!("buckal {}", build_version());
                    return;
                }
                match &args.subcommands {
                    Some(subcommand) => match subcommand {
                        BuckalSubCommands::Add(args) => commands::add::execute(args),
                        BuckalSubCommands::Autoremove(args) => commands::autoremove::execute(args),
                        BuckalSubCommands::Build(args) => commands::build::execute(args),
                        BuckalSubCommands::Clean(args) => commands::clean::execute(args),
                        BuckalSubCommands::Init(args) => commands::init::execute(args),
                        BuckalSubCommands::Login(args) => commands::login::execute(args),
                        BuckalSubCommands::Logout(args) => commands::logout::execute(args),
                        BuckalSubCommands::Migrate(args) => commands::migrate::execute(args),
                        BuckalSubCommands::New(args) => commands::new::execute(args),
                        BuckalSubCommands::Push(args) => commands::push::execute(args),
                        BuckalSubCommands::Remove(args) => commands::remove::execute(args),
                        BuckalSubCommands::Test(args) => commands::test::execute(args),
                        BuckalSubCommands::Update(args) => commands::update::execute(args),
                    },
                    None => {
                        // If no subcommand is provided, print help information
                        // This is unreachable due to `arg_required_else_help`, but kept as defensive programming
                        unreachable!("`arg_required_else_help` should prevent this branch")
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::validate_target_triple;

    #[test]
    fn test_cli_build_accepts_target() {
        let cli = Cli::try_parse_from([
            "cargo",
            "buckal",
            "build",
            "--target",
            "x86_64-unknown-linux-gnu",
        ])
        .expect("failed to parse build args with --target");

        match cli.command {
            Commands::Buckal(args) => match args.subcommands {
                Some(BuckalSubCommands::Build(build_args)) => {
                    assert_eq!(
                        build_args.target.as_deref(),
                        Some("x86_64-unknown-linux-gnu")
                    );
                    assert!(build_args.target_platforms.is_none());
                }
                other => panic!("expected build subcommand, got {other:?}"),
            },
        }
    }

    #[test]
    fn test_cli_test_accepts_target() {
        let cli = Cli::try_parse_from([
            "cargo",
            "buckal",
            "test",
            "--target",
            "x86_64-unknown-linux-gnu",
        ])
        .expect("failed to parse test args with --target");

        match cli.command {
            Commands::Buckal(args) => match args.subcommands {
                Some(BuckalSubCommands::Test(test_args)) => {
                    assert_eq!(
                        test_args.target.as_deref(),
                        Some("x86_64-unknown-linux-gnu")
                    );
                    assert!(test_args.target_platforms.is_none());
                }
                other => panic!("expected test subcommand, got {other:?}"),
            },
        }
    }

    #[test]
    fn test_cli_build_rejects_target_and_target_platforms() {
        let result = Cli::try_parse_from([
            "cargo",
            "buckal",
            "build",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target-platforms",
            "//platforms:x86_64-unknown-linux-gnu",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_test_rejects_target_and_target_platforms() {
        let result = Cli::try_parse_from([
            "cargo",
            "buckal",
            "test",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target-platforms",
            "//platforms:x86_64-unknown-linux-gnu",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_build_invalid_target_fails_validation() {
        let cli =
            Cli::try_parse_from(["cargo", "buckal", "build", "--target", "not-a-real-target"])
                .expect("failed to parse build args with invalid --target");

        match cli.command {
            Commands::Buckal(args) => match args.subcommands {
                Some(BuckalSubCommands::Build(build_args)) => {
                    let err = validate_target_triple(build_args.target.as_deref().unwrap())
                        .expect_err("expected invalid target triple to fail validation");
                    assert!(err.contains("not a valid rustc target"));
                }
                other => panic!("expected build subcommand, got {other:?}"),
            },
        }
    }

    #[test]
    fn test_cli_test_invalid_target_fails_validation() {
        let cli = Cli::try_parse_from(["cargo", "buckal", "test", "--target", "not-a-real-target"])
            .expect("failed to parse test args with invalid --target");

        match cli.command {
            Commands::Buckal(args) => match args.subcommands {
                Some(BuckalSubCommands::Test(test_args)) => {
                    let err = validate_target_triple(test_args.target.as_deref().unwrap())
                        .expect_err("expected invalid target triple to fail validation");
                    assert!(err.contains("not a valid rustc target"));
                }
                other => panic!("expected test subcommand, got {other:?}"),
            },
        }
    }
}
