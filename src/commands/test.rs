use clap::Parser;

use crate::{
    buck2::Buck2Command,
    buckal_error, buckal_note,
    filter::{FilterCaller, TargetFilter, get_available_targets},
    utils::{
        UnwrapOrExit, ensure_prerequisites, get_buck2_root, get_target, is_inside_buck2_project,
        platform_exists, validate_target_triple,
    },
};

#[derive(Parser, Debug)]
pub struct TestArgs {
    /// Test all targets
    #[arg(long)]
    pub all_targets: bool,

    /// Test only this package's library
    #[arg(long)]
    pub lib: bool,

    /// Test only the specified binary
    #[arg(long, value_name = "NAME")]
    pub bin: Vec<String>,

    /// Test all binaries
    #[arg(long)]
    pub bins: bool,

    /// Test only the specified example
    #[arg(long, value_name = "NAME")]
    pub example: Vec<String>,

    /// Test all examples
    #[arg(long)]
    pub examples: bool,

    /// Test only the specified test target
    #[arg(long, value_name = "NAME")]
    pub test: Vec<String>,

    /// Test all targets that have `test = true` set
    #[arg(long)]
    pub tests: bool,

    /// Test only the specified bench target
    #[arg(long, value_name = "NAME")]
    pub bench: Vec<String>,

    /// Test all targets that have `bench = true` set
    #[arg(long)]
    pub benches: bool,

    /// Compile, but don't run tests
    #[arg(long)]
    pub no_run: bool,

    /// Run all tests regardless of failure
    #[arg(long)]
    pub no_fail_fast: bool,

    /// Number of threads to use during execution (default to cores)
    #[arg(short = 'j', long, value_name = "THREADS")]
    pub num_threads: Option<usize>,

    /// Build for the target triple (e.g., x86_64-unknown-linux-gnu)
    #[arg(long, value_name = "TRIPLE", conflicts_with = "target_platforms")]
    pub target: Option<String>,

    /// Build for the target platform (passed to buck2 --target-platforms)
    #[arg(long, value_name = "PLATFORM", conflicts_with = "target")]
    pub target_platforms: Option<String>,

    /// Build artifacts in release mode, with optimizations
    #[arg(short, long)]
    pub release: bool,

    /// Use verbose output (`-vv` very verbose output)
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// If specified, only run tests containing this string in their names
    #[arg(value_name = "TESTNAME")]
    pub test_name: Option<String>,

    /// Arguments for the test executor
    #[arg(last = true)]
    pub args: Vec<String>,
}

pub fn execute(args: &TestArgs) {
    // Ensure all prerequisites are installed before proceeding
    ensure_prerequisites().unwrap_or_exit();

    // Check if the current directory is a valid Buck2 package
    is_inside_buck2_project().unwrap_or_exit();

    if args.verbose > 2 {
        buckal_error!("maximum verbosity");
        std::process::exit(1);
    }

    // Get the root directory of the Buck2 project
    let buck2_root = get_buck2_root().unwrap_or_exit();
    let cwd = std::env::current_dir().unwrap_or_exit_ctx("failed to get current directory");
    let mut relative = cwd
        .strip_prefix(&buck2_root)
        .unwrap_or_exit_ctx("test command should invoke inside a Buck2 project")
        .to_string_lossy()
        .into_owned();

    // Normalize path separators for Buck2 (always use forward slashes)
    relative = relative.replace('\\', "/");

    // Construct the target filter based on provided arguments
    let mut target_filter = TargetFilter::from_raw_arguments(
        args.lib,
        args.bin.clone(),
        args.bins,
        args.test.clone(),
        args.tests,
        args.example.clone(),
        args.examples,
        args.bench.clone(),
        args.benches,
        args.all_targets,
        FilterCaller::Test,
    )
    .unwrap_or_exit();

    if args.test_name.is_some() && !target_filter.is_specific() {
        // If arg `TESTNAME` is provided and no specific targets are requested,
        // we assumed that the user knows what exactly they wants to test.
        target_filter = TargetFilter::all_test_targets();
    }

    let available_targets = get_available_targets(&relative).unwrap_or_exit();

    let mut buck2_cmd = if args.no_run {
        Buck2Command::build().verbosity(args.verbose)
    } else {
        Buck2Command::test().verbosity(args.verbose)
    };

    if let Some(num_threads) = args.num_threads {
        buck2_cmd = buck2_cmd.arg("-j").arg(num_threads.to_string());
    }

    let target_platforms = if let Some(triple) = &args.target {
        // Validate the target triple and get the corresponding platform
        match validate_target_triple(triple) {
            Ok(platform) => Some(platform),
            Err(e) => {
                buckal_error!(e);
                std::process::exit(1);
            }
        }
    } else if let Some(platform) = &args.target_platforms {
        Some(platform.clone())
    } else {
        let platform = format!("//platforms:{}", get_target());
        if platform_exists(&platform) {
            Some(platform)
        } else {
            None
        }
    };
    if let Some(platform) = &target_platforms {
        buck2_cmd = buck2_cmd.arg("--target-platforms").arg(platform);
    }

    if args.release {
        buck2_cmd = buck2_cmd.arg("-m").arg("release");
    }

    if args.no_fail_fast {
        buck2_cmd = buck2_cmd.arg("--keep-going");
    }

    let mut target_specified = false;

    // Add test targets to the command based on the filter
    for target in &available_targets {
        if target_filter.target_run(target) {
            if let Some(test_name) = &args.test_name
                && !target.name().contains(test_name)
            {
                continue;
            }
            buck2_cmd = buck2_cmd.arg(target.label());
            target_specified = true;
        }
    }

    if !target_specified {
        buckal_error!("all targets filtered out, nothing to test");
        buckal_note!(
            "please check the filter arguments and ensure if there are any testable targets in current directory"
        );
        std::process::exit(1);
    }

    // Additional arguments passed to the test executor.
    // Only applied when `--no-run` is not set.
    if !args.no_run && !args.args.is_empty() {
        buck2_cmd = buck2_cmd.arg("--");
        for arg in &args.args {
            buck2_cmd = buck2_cmd.arg(arg);
        }
    }

    match buck2_cmd.status() {
        Ok(status) if status.success() => {}
        _ => std::process::exit(1),
    }
}
