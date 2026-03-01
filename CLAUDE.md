# CLAUDE.md — cargo-buckal

## Project Overview

**cargo-buckal** is a Rust CLI tool (cargo subcommand) that bridges Cargo and Buck2 build systems. It converts Cargo dependency graphs into Buck2 BUCK files, enabling Cargo projects to build with Buck2 seamlessly.

- Repository: https://github.com/buck2hub/cargo-buckal
- Docs: https://buck2hub.com/docs
- License: MIT
- Rust Edition: 2024

## Build & Test Commands

```bash
cargo build                                                  # Debug build
cargo build --release                                        # Release build
cargo test                                                   # Run unit tests
cargo clippy --all-targets --all-features -- -D warnings     # Lint (must pass with zero warnings)
cargo +nightly fmt --check                                   # Check formatting
cargo +nightly fmt --                                        # Auto-format
```

### Pre-commit Hooks

This project uses [prek](https://github.com/j178/prek) (not standard pre-commit):

```bash
prek install              # Set up git hooks (one-time)
prek run --all-files      # Run all checks
```

Hooks enforce: `cargo +nightly fmt`, `cargo clippy -D warnings`, typos, TOML/YAML validation, trailing whitespace, buildifier (BUCK files), actionlint + zizmor (GitHub Actions).

### Validation Checklist (before committing)

1. `cargo build` succeeds
2. `cargo clippy --all-targets --all-features -- -D warnings` — zero warnings
3. `cargo +nightly fmt --check` passes
4. `prek run --all-files` passes

## Project Structure

```
src/
├── main.rs              # Entry point, version/user-agent helpers
├── cli.rs               # Clap CLI definition (Cli → Commands → BuckalSubCommands)
├── commands/            # One module per subcommand
│   ├── add.rs           # Add dependencies
│   ├── remove.rs        # Remove dependencies
│   ├── update.rs        # Update dependencies
│   ├── autoremove.rs    # Auto-remove unused deps
│   ├── migrate.rs       # Migrate Cargo project to Buck2
│   ├── init.rs          # Initialize Buck2 project
│   ├── new.rs           # Create new package
│   ├── build.rs         # Build via Buck2
│   ├── test.rs          # Test via Buck2
│   ├── clean.rs         # Clean buck-out
│   ├── login.rs         # Registry login
│   ├── logout.rs        # Registry logout
│   └── push.rs          # Push BUCK files to registry
├── buckify/             # Core Cargo→Buck2 conversion
│   ├── rules.rs         # BUCK rule generation (http_archive, rust_library, etc.)
│   ├── emit.rs          # Rule emission with platform compatibility
│   ├── deps.rs          # Dependency resolution and label generation
│   ├── windows.rs       # Windows-specific path handling
│   ├── cross.rs         # Cross-platform utilities
│   └── actions.rs       # Action rules
├── buck.rs              # Buck2 rule types and serialization
├── buck2.rs             # Buck2 command wrapper
├── config.rs            # Config loading (~/.config/buckal/config.toml, buckal.toml)
├── context.rs           # BuckalContext (cargo metadata, cache, packages)
├── cache.rs             # BLAKE3 fingerprint-based caching (buckal.snap)
├── platform.rs          # Platform/OS detection, cfg predicate evaluation
├── registry.rs          # Registry API types
├── bundles.rs           # Buckal bundles management
├── assets.rs            # Embedded BUCK template extraction
└── utils.rs             # Helpers, UnwrapOrExit trait, logging macros
assets/                  # Embedded BUCK templates for toolchains/platforms
docs/                    # Documentation (cache.md, multi-platform.md)
```

## Architecture & Key Patterns

### Command Pattern

Each subcommand follows a consistent pattern:

```rust
// src/commands/<command>.rs
pub struct CommandArgs { ... }  // Clap Parser derive
pub fn execute(args: &CommandArgs) { ... }  // Entry point
```

Commands are registered in `src/cli.rs` via the `BuckalSubCommands` enum and dispatched in `Cli::run()`.

### Error Handling

- Use `anyhow::Result<T>` for internal error propagation
- Custom `UnwrapOrExit<T>` trait (in `utils.rs`) for top-level error handling that prints and exits
- Colored output macros:
  - `buckal_log!("Action", "message")` — colored action prefix (green/yellow/cyan/blue)
  - `buckal_error!("message")` — red error prefix to stderr
  - `buckal_warn!("message")` — yellow warning to stderr
  - `buckal_note!("message")` — cyan note to stderr

### Serialization

- TOML for config files (`toml`, `toml_edit`)
- Starlark code generation via `serde_starlark`
- Bincode + BLAKE3 for cache fingerprints
- JSON for registry API

### Platform Handling

- `Os` enum: `Windows`, `Macos`, `Linux`
- Tier-1 triples: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`
- Platform predicates evaluated via `rustc --print=cfg --target <triple>`
- Conditional deps mapped to `os_deps`/`os_named_deps` in BUCK files

## Code Conventions

- Follow standard Rust naming: `snake_case` for functions/variables, `PascalCase` for types
- Keep commands modular — changes to one command should not affect others
- Use existing macros (`buckal_log!`, `buckal_error!`, etc.) for user-facing output
- Prefer `anyhow::Result` over custom error types
- New dependencies must be clearly justified and consistent with the existing ecosystem (prefer established crates: `anyhow`, `clap`, `serde`, `pyo3`)
- Use `OnceLock` for per-process cached values (see `main.rs` for examples)

## CI/CD

Key workflows in `.github/workflows/`:

- **`build-and-test.yml`** — `cargo build` on every PR
- **`lint.yml`** — runs `prek` (cargo +nightly fmt + clippy + typos + buildifier + actionlint + zizmor)
- **`integration-test-*.yml`** — tests against real projects (fd, libra, git-internal, monorepo-demo)
- **`test-init-new-commands.yml`** — tests init/new command workflows

Reusable workflows prefixed with `_buck2-*.yml` handle Buck2 generation, cross-platform verification, and build/test.

## Testing

Unit tests are inline (`#[cfg(test)]` modules) in:
- `src/cli.rs` — CLI arg parsing and validation
- `src/platform.rs` — platform detection
- `src/utils.rs` — target triple validation
- `src/buckify/` modules — rule generation

Most end-to-end validation happens through CI integration tests.

## Configuration Files

- **Global**: `~/.config/buckal/config.toml` — `buck2_binary` path, registries
- **Per-repo**: `buckal.toml` — `inherit_workspace_deps`, `align_cells`, `patch_fields`
- **Pre-commit**: `.pre-commit-config.yaml` — hook definitions for prek
