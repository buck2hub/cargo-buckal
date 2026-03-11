use clap::Parser;

pub fn main() {
    let args = cargo_buckal::cli::Cli::parse();
    args.run();
}
