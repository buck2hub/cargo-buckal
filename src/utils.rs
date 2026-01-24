use std::collections::HashMap;
use std::{io, process::Command, str::FromStr};

use cargo_metadata::MetadataCommand;
use cargo_metadata::camino::Utf8PathBuf;
use cargo_platform::Cfg;
use colored::Colorize;
use inquire::Select;

use crate::RUST_CRATES_ROOT;
use crate::buck2::Buck2Command;
use crate::cache::BuckalCache;

#[macro_export]
macro_rules! buckal_log {
    ($action:expr, $msg:expr) => {{
        let colored = match $action {
            "Adding" => ::colored::Colorize::green($action),
            "Creating" => ::colored::Colorize::green($action),
            "Flushing" => ::colored::Colorize::green($action),
            "Removing" => ::colored::Colorize::yellow($action),
            "Fetching" => ::colored::Colorize::cyan($action),
            _ => ::colored::Colorize::blue($action),
        };
        println!("{:>12} {}", ::colored::Colorize::bold(colored), $msg);
    }};
}

#[macro_export]
macro_rules! buckal_error {
    ($msg:expr) => {{
        let error_prefix = ::colored::Colorize::red("error:");
        eprintln!("{} {}", ::colored::Colorize::bold(error_prefix), $msg);
    }};

    ($fmt:expr, $($arg:tt)*) => {{
        let error_prefix = ::colored::Colorize::red("error:");
        eprintln!(
            "{} {}",
            ::colored::Colorize::bold(error_prefix),
            format_args!($fmt, $($arg)*)
        );
    }};
}

#[macro_export]
macro_rules! buckal_note {
    ($msg:expr) => {{
        let note_prefix = ::colored::Colorize::cyan("note:");
        eprintln!("{} {}", ::colored::Colorize::bold(note_prefix), $msg);
    }};

    ($fmt:expr, $($arg:tt)*) => {{
        let note_prefix = ::colored::Colorize::cyan("note:");
        eprintln!(
            "{} {}",
            ::colored::Colorize::bold(note_prefix),
            format_args!($fmt, $($arg)*)
        );
    }};
}

#[macro_export]
macro_rules! buckal_warn {
    ($msg:expr) => {{
        let warn_prefix = ::colored::Colorize::yellow("warn:");
        eprintln!("{} {}", ::colored::Colorize::bold(warn_prefix), $msg);
    }};

    ($fmt:expr, $($arg:tt)*) => {{
        let warn_prefix = ::colored::Colorize::yellow("warn:");
        eprintln!(
            "{} {}",
            ::colored::Colorize::bold(warn_prefix),
            format_args!($fmt, $($arg)*)
        );
    }};
}

pub fn check_buck2_installed() -> bool {
    Buck2Command::new()
        .arg("--help")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn prompt_buck2_installation() -> io::Result<bool> {
    println!();
    println!(
        "{} {}",
        "⚠️".yellow(),
        "Buck2 is not installed or not found in PATH.".yellow()
    );
    println!(
        "{} {}",
        "🔧".blue(),
        "Buck2 is required to use cargo buckal.".blue()
    );
    println!();

    let options = vec![
        "🚀 Install automatically (recommended)",
        "📖 Exit and show manual installation guide",
    ];

    let ans = Select::new("How would you like to install Buck2?", options)
        .prompt()
        .map_err(|e| io::Error::other(format!("Selection error: {}", e)))?;

    match ans {
        "🚀 Install automatically (recommended)" => {
            println!();
            println!(
                "{} {}",
                "🚀".green(),
                "Installing Buck2 automatically...".green()
            );

            if let Err(e) = install_buck2_automatically() {
                println!("{} {}: {}", "❌".red(), "Installation failed".red(), e);
                println!();
                show_manual_installation();
                return Ok(false);
            }

            println!(
                "{} {}",
                "✅".green(),
                "Buck2 installation completed!".green()
            );
            println!("{} {}", "🔍".blue(), "Verifying installation...".blue());

            // Check if installation was successful
            if check_buck2_installed() {
                println!("{} {}", "🎉".green(), "Buck2 is now available!".green());
                Ok(true)
            } else {
                println!(
                    "{} {}",
                    "⚠️".yellow(),
                    "Buck2 installation completed but not found in PATH.".yellow()
                );
                println!(
                    "{} {}",
                    "💡".bright_blue(),
                    "You may need to restart your terminal or source your shell profile."
                        .bright_blue()
                );
                Ok(false)
            }
        }
        "📖 Exit and show manual installation guide" => {
            show_manual_installation();
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn install_buck2_automatically() -> io::Result<()> {
    println!("{} {}", "📦".cyan(), "Installing Rust nightly...".cyan());
    let status = Command::new("rustup")
        .args(["install", "nightly-2025-06-20"])
        .status()?;

    if !status.success() {
        return Err(io::Error::other("Failed to install Rust nightly"));
    }

    println!(
        "{} {}",
        "📦".cyan(),
        "Installing Buck2 from GitHub...".cyan()
    );
    let status = Command::new("cargo")
        .args([
            "+nightly-2025-06-20",
            "install",
            "--git",
            "https://github.com/facebook/buck2.git",
            "buck2",
        ])
        .status()?;

    if !status.success() {
        return Err(io::Error::other("Failed to install Buck2"));
    }

    Ok(())
}

fn show_manual_installation() {
    println!();
    println!(
        "{} {}",
        "📖".green(),
        "Manual Buck2 Installation Guide".green().bold()
    );
    println!();

    println!(
        "{}",
        "Choose one of the following installation methods:".bright_magenta()
    );
    println!();

    // Method 1: Cargo install
    println!(
        "{}",
        "Method 1: Install via Cargo (Recommended)".cyan().bold()
    );
    println!("{}", "1. Install Rust nightly (prerequisite)".cyan());
    println!("   {}", "rustup install nightly-2025-06-20".bright_white());
    println!();
    println!("{}", "2. Install Buck2 from GitHub".cyan());
    println!(
        "   {}",
        "cargo +nightly-2025-06-20 install --git https://github.com/facebook/buck2.git buck2"
            .bright_white()
    );
    println!();
    println!("{}", "3. Add to your PATH (if not already)".cyan());
    println!(
        "   {}",
        "# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)".bright_black()
    );
    println!("   {}", "Linux/macOS:".bright_black());
    println!("   {}", "export PATH=$HOME/.cargo/bin:$PATH".bright_white());
    println!("   {}", "Windows PowerShell:".bright_black());
    println!(
        "   {}",
        "$Env:PATH += \";$HOME\\.cargo\\bin\"".bright_white()
    );
    println!();

    println!("{}", "─".repeat(60).bright_black());
    println!();

    // Method 2: Direct download
    println!("{}", "Method 2: Download Pre-built Binary".yellow().bold());
    println!("{}", "1. Download from GitHub releases".yellow());
    println!(
        "   {}",
        "https://github.com/facebook/buck2/releases/tag/latest"
            .bright_white()
            .underline()
    );
    println!();
    println!("{}", "2. Extract and place in your PATH".yellow());
    println!(
        "   {}",
        "# Extract the downloaded file and move to a directory in your PATH".bright_black()
    );
    println!(
        "   {}",
        "# For example: /usr/local/bin (Linux/macOS) or C:\\bin (Windows)".bright_black()
    );
    println!();

    println!("{}", "─".repeat(60).bright_black());
    println!();

    // Verification
    println!("{} {}", "✅".green(), "Verify Installation".green().bold());
    println!("   {}", "buck2 --help".bright_white());
    println!();

    println!(
        "{} {}",
        "💡".bright_blue(),
        "Note: After installation, restart your terminal or source your shell profile."
            .bright_blue()
    );
    println!();

    println!(
        "{} {}",
        "📚".bright_cyan(),
        "For detailed instructions and troubleshooting, refer to:".bright_cyan()
    );
    println!(
        "   {}",
        "https://buck2.build/docs/getting_started/install/"
            .cyan()
            .underline()
    );
    println!();

    println!(
        "{} {}",
        "🔄".yellow(),
        "Once Buck2 is installed, run your cargo buckal command again.".yellow()
    );
    println!();
}

pub fn ensure_buck2_installed() -> io::Result<()> {
    if !check_buck2_installed() {
        let installed = prompt_buck2_installation()?;
        if !installed {
            return Err(io::Error::other(
                "Buck2 is required but not installed. Please install Buck2 and try again.",
            ));
        }
    }
    Ok(())
}

pub fn get_buck2_root() -> io::Result<Utf8PathBuf> {
    // This function should return the root directory of the Buck2 project.
    let out_put = Buck2Command::root().arg("--kind").arg("project").output()?;
    if out_put.status.success() {
        let path_str = String::from_utf8_lossy(&out_put.stdout).trim().to_string();
        Ok(Utf8PathBuf::from(path_str))
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&out_put.stderr).to_string(),
        ))
    }
}

/// Check if a platform target exists using buck2 uquery
pub fn platform_exists(platform_target: &str) -> bool {
    let output = crate::buck2::Buck2Command::uquery()
        .arg(platform_target)
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

pub fn check_buck2_package() -> io::Result<()> {
    // This function checks if the current directory is a valid Buck2 package.
    let cwd = std::env::current_dir().expect("Failed to get current directory");
    let buck_file = cwd.join("BUCK");
    if !buck_file.exists() {
        return Err(io::Error::other(format!(
            "could not find `BUCK` in `{}`. Are you in a Buck2 package?",
            cwd.display(),
        )));
    }
    Ok(())
}

pub fn get_target() -> String {
    let output = Command::new("rustc")
        .arg("-Vv")
        .output()
        .expect("rustc failed to run");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for line in stdout.lines() {
        if let Some(line) = line.strip_prefix("host: ") {
            return String::from(line);
        }
    }
    panic!("Failed to find host: {stdout}");
}

pub fn get_cfgs() -> Vec<Cfg> {
    let output = Command::new("rustc")
        .arg("--print=cfg")
        .output()
        .expect("rustc failed to run");
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .lines()
        .map(|line| Cfg::from_str(line).unwrap())
        .collect()
}

pub fn get_cache_path() -> io::Result<Utf8PathBuf> {
    Ok(get_buck2_root()?.join("buckal.snap"))
}

pub fn get_vendor_dir(name: &str, version: &str) -> io::Result<Utf8PathBuf> {
    Ok(get_buck2_root()?.join(format!("{RUST_CRATES_ROOT}/{}/{}", name, version)))
}

pub fn get_last_cache() -> BuckalCache {
    // This function retrieves the last saved BuckalCache from the cache file.
    // If the cache file does not exist, it returns a snapshot of the current state.
    if let Ok(last_cache) = BuckalCache::load() {
        last_cache
    } else {
        let cargo_metadata = MetadataCommand::new().exec().unwrap_or_exit();
        let resolve = cargo_metadata.resolve.unwrap();
        let nodes_map = resolve
            .nodes
            .into_iter()
            .map(|n| (n.id.to_owned(), n))
            .collect::<HashMap<_, _>>();
        BuckalCache::new(&nodes_map, &cargo_metadata.workspace_root)
    }
}

pub fn section(title: &str) {
    let content = format!("---- {} ----", title);
    let width = 60;

    if content.len() >= width {
        println!("{}", content);
        return;
    }

    let total_padding = width - content.len();
    let left_padding = total_padding / 2;
    let right_padding = total_padding - left_padding;

    let left_pad = "-".repeat(left_padding);
    let right_pad = "-".repeat(right_padding);

    println!("{}{}{}", left_pad, content, right_pad);
}

pub fn check_python3_installed() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Quick check if rustc is available before spawning multiple threads.
pub fn check_rustc_installed() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn ensure_rustc_installed() -> io::Result<()> {
    if !check_rustc_installed() {
        return Err(io::Error::other(
            "rustc is required but not installed. Please install Rust and try again.",
        ));
    }
    Ok(())
}

pub fn ensure_python3_installed() -> io::Result<()> {
    if !check_python3_installed() {
        return Err(io::Error::other(
            "Python 3 is required but not installed. Please install Python 3 and try again.",
        ));
    }
    Ok(())
}

pub fn ensure_prerequisites() -> io::Result<()> {
    ensure_rustc_installed()?;
    ensure_buck2_installed()?;
    ensure_python3_installed()?;
    Ok(())
}

pub trait UnwrapOrExit<T> {
    fn unwrap_or_exit(self) -> T;
    fn unwrap_or_exit_ctx(self, context: impl std::fmt::Display) -> T;
}

impl<T, E: std::fmt::Display> UnwrapOrExit<T> for Result<T, E> {
    fn unwrap_or_exit(self) -> T {
        match self {
            Ok(value) => value,
            Err(error) => {
                buckal_error!(error);
                std::process::exit(1);
            }
        }
    }

    fn unwrap_or_exit_ctx(self, context: impl std::fmt::Display) -> T {
        match self {
            Ok(value) => value,
            Err(error) => {
                buckal_error!("{}:\n{}", context, error);
                std::process::exit(1);
            }
        }
    }
}
