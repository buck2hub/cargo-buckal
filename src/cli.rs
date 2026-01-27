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

    /// Migrate existing Cargo packages to Buck2
    Migrate(crate::commands::migrate::MigrateArgs),

    /// Create a new package
    New(crate::commands::new::NewArgs),

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
                        BuckalSubCommands::Migrate(args) => commands::migrate::execute(args),
                        BuckalSubCommands::New(args) => commands::new::execute(args),
                        BuckalSubCommands::Remove(args) => commands::remove::execute(args),
                        BuckalSubCommands::Test(args) => commands::test::execute(args),
                        BuckalSubCommands::Update(args) => commands::update::execute(args),
                    },
                    None => {
                        // If no subcommand is provided, print help information
                        // This would not normally be reached because of `arg_required_else_help`
                    }
                }
            }
        }
    }
}
