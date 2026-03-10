# cargo-buckal

Seamlessly build Cargo projects with Buck2.

![demo](docs/demo.gif)

## Install

You can install the latest stable release from crates.io:

```bash
cargo install cargo-buckal
```

Or, to install the latest development version from the active repository:

```bash
cargo install --git https://github.com/buck2hub/cargo-buckal.git
```

> [!NOTE]
>
> Buckal requires [Buck2](https://buck2.build/). Please ensure it's installed on your system before proceeding.

## Usage

Run `cargo buckal --help` for more information, and visit https://buck2hub.com/docs for comprehensive documentation.

Common commands:

- `cargo buckal init|new`: Create a new package or a Buck2 project in the directory.
- `cargo buckal migrate`: Migrate an existing Cargo project to Buck2 (generate/update BUCK files).
- `cargo buckal add|remove|update`: Manage dependencies, applying the changes to both `Cargo.toml` and `BUCK` files.
- `cargo buckal build`: Build the current package with Buck2.
- `cargo buckal test`: Compile and execute unit and integration tests with Buck2.
- `cargo buckal clean`: Remove `buck-out` directory.

## Migrate existing Cargo projects

For any Cargo project that builds successfully, you can migrate to Buck2 with zero configuration by running the following command in a valid directory (one containing `Cargo.toml`). Buckal will automatically initialize the Buck2 project configuration and convert the Cargo dependency graph into `BUCK` files.

```bash
cargo buckal migrate --init <repo_root>
```

This is equivalent to running `cargo buckal init --repo` at `<repo_root>` followed by `cargo buckal migrate` in the current directory.

## Supported platforms

Platform-aware dependency mapping and bundled sample platforms currently target these Rust tier-1 host triples:

- Linux: `x86_64-unknown-linux-gnu`
- Windows: `x86_64-pc-windows-msvc`
- macOS: `aarch64-apple-darwin`

## Multi-platform builds

Buckal preserves platform-conditional Cargo dependencies by emitting `os_deps`/`os_named_deps` and canonical OS constraints, so the same generated BUCK files can be built for different target platforms without regenerating on each host.

See https://buck2hub.com/docs/multi-platform.

## Configuration

You can configure cargo-buckal by creating a configuration file at `~/.config/buckal/config.toml`.

### Custom Buck2 Binary Path

If you have buck2 installed in a custom location, you can specify the path:

```toml
buck2_binary = "/path/to/your/buck2"
```

If no configuration file exists, cargo-buckal will use `buck2` (searches your PATH).

## Pre-commit Hooks

This project uses [prek](https://github.com/j178/prek) for pre-commit hooks (configured in `.pre-commit-config.yaml`).
Install `prek` following the project instructions, then set up the git hooks:

```
prek install
```

To run hooks on all files at any time:

```
prek run --all-files
```

## Repos using cargo-buckal

- [rk8s-dev/rk8s](https://github.com/rk8s-dev/rk8s): A lightweight Kubernetes-compatible container orchestration system written in Rust.
- [web3infra-foundation/libra](https://github.com/web3infra-foundation/libra): High-performance reimplementation and extension of the core Git engine in Rust, focused on foundational VCS primitives and customizable storage semantics compatible with Git workflows.
- [web3infra-foundation/git-internal](https://github.com/web3infra-foundation/git-internal): Internal Git infrastructure, experiments, and foundational components for Git-compatible monorepo systems.
