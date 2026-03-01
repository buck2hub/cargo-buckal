# Copilot instructions for cargo-buckal

These instructions are intended for an automated coding agent (such as GitHub Copilot) working on the cargo-buckal repository. They describe where to look for important information and how to validate changes.

## 1. Understanding cargo-buckal

**What is cargo-buckal?**

cargo-buckal is a Rust CLI tool that bridges Cargo and Buck2, enabling seamless builds and dependency management. Key aspects:
- Provides `cargo buckal` subcommands: `init`, `new`, `migrate`, `add`, `remove`, `autoremove`, `update`, `build`, `test`, `clean`
- Automatically converts Cargo dependency graphs to Buck2 BUCK files
- Supports platform-aware dependency mapping for Linux, Windows, and macOS
- Requires Buck2 and Python3 to be installed on the system

**Start with `README.md`** for:
- Feature overview and common commands
- Installation instructions
- Platform support details
- Configuration guide (`~/.config/buckal/config.toml`)
- Links to comprehensive documentation at https://buck2hub.com/docs

**Project structure:**
- This is a **Rust project** with `Cargo.toml` as the primary manifest
- Source code in `src/`: Core modules (`buck.rs`, `buck2.rs`, `buckify.rs`, etc.)
- Commands in `src/commands/`: Each command is a separate module (e.g., `migrate.rs`, `add.rs`)
- Configuration: `.pre-commit-config.yaml`, `.github/workflows/`
- Assets in `assets/`: BUCK templates and demo files for toolchains

## 2. Building and running

**Requirements:**
- Rust toolchain (stable) with `rustfmt` and `clippy` components
- Buck2 (for full integration testing; cargo build works without it)
- Python 3.11+ (for pre-commit hooks and Buck2 integration)

**Build commands:**
```bash
cargo build              # Debug build
cargo build --release   # Optimized release build
cargo run -- --help     # Run with arguments
```

**Running specific commands:**
Since cargo-buckal is a CLI tool, test it with:
```bash
cargo run -- init --help    # View command help
cargo run -- migrate --help # Test migration logic
cargo run -- build --help   # Test build command
```

The tool can be installed locally with `cargo install --path .` for testing installation workflows.

## 3. Testing and validation

**Core testing commands:**
```bash
cargo build              # Verify compilation
cargo clippy --all-targets --all-features -- -D warnings  # Linter (strict mode)
cargo fmt --check       # Check formatting
cargo fmt --            # Auto-format code
```

**Pre-commit hooks:**
This project uses `prek` for automated checks:
```bash
prek install            # Set up git hooks (run once)
prek run --all-files    # Run all checks on entire codebase
```

Pre-commit hooks enforce:
- `cargo fmt` (Rust formatting)
- `cargo clippy --all-targets --all-features -- -D warnings` (strict linting)
- Typos detection, TOML/YAML validation, trailing whitespace
- Buildifier for BUCK file formatting

**Before submitting changes:**
1. Ensure `cargo build` succeeds
2. Run `cargo clippy --all-targets --all-features -- -D warnings` (must have zero warnings)
3. Run `cargo fmt --check` (or `cargo fmt --` to auto-fix)
4. Run `prek run --all-files` to check all pre-commit rules
5. Test the CLI with `cargo run -- <command> --help`

**Note:** The CI workflow (`.github/workflows/build-and-test.yml`) currently has a placeholder test step. Most validation happens via linting and pre-commit hooks.

## 4. Code layout and configuration

**Key directories:**
- `src/main.rs`: CLI entry point using `clap` for argument parsing
- `src/cli.rs`: Command-line interface definition
- `src/commands/`: Implementation of all subcommands (`init.rs`, `migrate.rs`, `add.rs`, etc.)
- `src/buckify/`: Core logic for Cargo→Buck2 conversion
  - `actions.rs`, `deps.rs`, `emit.rs`: Dependency handling and BUCK file generation
  - `cross.rs`: Cross-platform dependency logic
  - `windows.rs`: Windows-specific handling
- `src/config.rs`: Configuration file handling
- `src/buck2.rs`, `src/buck.rs`: Buck2 integration
- `assets/`: BUCK file templates for platforms and toolchains

**Important modules:**
- `platform.rs`: Platform detection (Linux, macOS, Windows)
- `cache.rs`: Caching layer
- `bundles.rs`: Buck2 toolchain bundle management
- `utils.rs`: Utility functions

**Configuration files:**
- `.pre-commit-config.yaml`: Pre-commit hooks (cargo fmt, cargo clippy, typos, buildifier)
- `Cargo.toml`: Rust dependencies and project metadata (edition 2024)

**When making changes:**
- Modify code in `src/` directories following existing patterns
- Use `anyhow::Result<T>` for error handling (consistent with codebase)
- Commands are modular: changes to one command should not affect others
- Starlark template strings use `serde_starlark` for serialization

## 5. CI and GitHub Actions

**Main CI workflows:**
- `.github/workflows/build-and-test.yml`: Runs `cargo build` on every PR
- `.github/workflows/lint.yml`: Runs `prek` (pre-commit hooks) for formatting and linting
- `.github/workflows/test-init-new-commands.yml`: Tests `cargo buckal init` and `new` commands
- `.github/workflows/integration-test-*.yml`: Tests against real projects (monorepo-demo, fd, libra, git-internal)
- `.github/workflows/_buck2-*.yml`: Buck2-related build and verification tests

**Custom GitHub Actions:**
- `.github/actions/setup-rust-toolchains/`: Sets up stable Rust with clippy and rustfmt
- `.github/actions/install-buck2/`: Installs Buck2 for integration tests
- `.github/actions/setup_test_env/`: Sets up test environment with Python 3.11+

**Key points for CI:**
1. The build workflow checks `cargo build` succeeds on Ubuntu
2. The lint workflow runs `prek` which enforces `cargo fmt` and `cargo clippy`
3. Integration tests validate the tool against real-world projects
4. Placeholders exist for future test enhancements

**Before finalizing changes:**
1. Run `cargo build` locally (primary CI check)
2. Run `prek run --all-files` locally (enforces lint/fmt, same as CI)
3. Verify specific commands with `cargo run -- <subcommand> --help`
4. If modifying commands, test with `cargo buckal <command>` after `cargo install --path .`

## 6. Working strategy for Copilot

- **Start with README.md** for high-level understanding of features and commands
- **Follow the existing patterns** for error handling (use `anyhow::Result<T>`), logging, and CLI design
- When adding new code:
  - Add commands to `src/commands/` as separate modules
  - Use `clap`'s `Parser` derive macro for CLI arguments
  - Ensure platform-awareness via `src/platform.rs` and `src/buckify/cross.rs`
  - Update BUCK templates in `assets/` if needed
- **Only introduce new dependencies** if clearly justified and consistent with the existing ecosystem (prefer established Rust crates like `anyhow`, `clap`, `serde`, `pyo3`)
- **Test locally** with `cargo run --` before relying on CI
- Before committing:
  1. Run `cargo fmt --`
  2. Run `cargo clippy --all-targets --all-features -- -D warnings`
  3. Run `prek run --all-files`
  4. Manually test the affected `cargo buckal` subcommand
