use std::{
    io,
    process::{Command, Stdio},
};

use crate::config::Config;

pub struct Buck2Command {
    command: Command,
}

impl Buck2Command {
    /// Create a new Buck2 command
    pub fn new() -> Self {
        let config = Config::load();
        let mut command = Command::new(&config.buck2_binary);
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        Self { command }
    }

    /// Add a subcommand (build, init, clean, etc.)
    pub fn subcommand(mut self, subcmd: &str) -> Self {
        self.command.arg(subcmd);
        self
    }

    /// Add an argument
    pub fn arg<S: AsRef<str>>(mut self, arg: S) -> Self {
        self.command.arg(arg.as_ref());
        self
    }

    /// Set verbosity level (converts to Buck2 -v flags)
    pub fn verbosity(mut self, level: u8) -> Self {
        match level {
            1 => self.command.arg("-v=3"),
            2 => self.command.arg("-v=4"),
            _ => &mut self.command,
        };
        self
    }

    /// Execute the command and return the status
    pub fn status(mut self) -> io::Result<std::process::ExitStatus> {
        self.command.status()
    }

    /// Execute the command and capture output
    pub fn output(mut self) -> io::Result<std::process::Output> {
        self.command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
    }

    /// Execute the command with inherited stdio and expect success
    pub fn execute(self) -> io::Result<()> {
        let status = self.status()?;
        if !status.success() {
            return Err(io::Error::other("Buck2 command failed"));
        }
        Ok(())
    }

    /// Create a build command with target
    pub fn build(target: &str) -> Self {
        Self::new().subcommand("build").arg(target)
    }

    /// Create an init command
    pub fn init() -> Self {
        Self::new().subcommand("init")
    }

    /// Create a clean command
    pub fn clean() -> Self {
        Self::new().subcommand("clean")
    }

    /// Create a root command
    pub fn root() -> Self {
        Self::new().subcommand("root")
    }

    /// Crate a targets command
    pub fn targets() -> Self {
        Self::new().subcommand("targets")
    }

    /// Create a uquery command
    pub fn uquery() -> Self {
        Self::new().subcommand("uquery")
    }
}

impl Default for Buck2Command {
    fn default() -> Self {
        Self::new()
    }
}
