use clap::Parser;

use crate::build_version;

#[derive(Parser, Debug)]
#[command(name = "buckal", version = build_version(), about = "A cargo plugin for Buck2", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub enum Commands {
    Buckal(BuckalArgs),
}

#[derive(Parser, Debug)]
pub struct BuckalArgs {
    #[command(subcommand)]
    pub subcommands: BuckalSubCommands,
}

#[derive(Parser, Debug)]
pub enum BuckalSubCommands {
    Add(crate::commands::add::AddArgs),
    Autoremove(crate::commands::autoremove::AutoremoveArgs),
    Build(crate::commands::build::BuildArgs),
    Clean(crate::commands::clean::CleanArgs),
    Init(crate::commands::init::InitArgs),
    Migrate(crate::commands::migrate::MigrateArgs),
    New(crate::commands::new::NewArgs),
    Remove(crate::commands::remove::RemoveArgs),
    Test(Box<crate::commands::test::TestArgs>),
    Update(crate::commands::update::UpdateArgs),
    Version(crate::commands::version::VersionArgs),
}

impl Cli {
    pub fn run(&self) {
        match &self.command {
            Commands::Buckal(args) => match &args.subcommands {
                BuckalSubCommands::Add(args) => crate::commands::add::execute(args),
                BuckalSubCommands::Autoremove(args) => crate::commands::autoremove::execute(args),
                BuckalSubCommands::Build(args) => crate::commands::build::execute(args),
                BuckalSubCommands::Clean(args) => crate::commands::clean::execute(args),
                BuckalSubCommands::Init(args) => crate::commands::init::execute(args),
                BuckalSubCommands::Migrate(args) => crate::commands::migrate::execute(args),
                BuckalSubCommands::New(args) => crate::commands::new::execute(args),
                BuckalSubCommands::Remove(args) => crate::commands::remove::execute(args),
                BuckalSubCommands::Test(args) => crate::commands::test::execute(args),
                BuckalSubCommands::Update(args) => crate::commands::update::execute(args),
                BuckalSubCommands::Version(args) => crate::commands::version::execute(args),
            },
        }
    }
}
