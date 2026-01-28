# Multi-platform builds

`cargo buckal migrate` can generate BUCK files that work across Linux/macOS/Windows without regenerating on each host. The key idea is to preserve Cargo’s platform-conditional dependencies in the generated rules.

## What gets generated

- `os_deps`: OS-scoped dependencies (e.g., a Windows-only dep lands under `os_deps["windows"]`).
- `os_named_deps`: same as `os_deps`, but for renamed dependencies.
- `compatible_with`: applied to a small allowlist of known OS-only crates to prevent Buck2 from building them on the wrong OS.

The generated rules use canonical Buck prelude OS constraint labels: `prelude//os/constraints:{linux,macos,windows}`.

## Supported platforms

Platform-aware dependency mapping and bundled sample platforms currently target these Rust tier-1
host triples:

- Linux: `x86_64-unknown-linux-gnu`
- Windows: `x86_64-pc-windows-msvc`
- macOS: `aarch64-apple-darwin`

## How platform matching works

Cargo encodes target-specific dependencies in `cargo metadata` as platform predicates (for example, `cfg(target_os = "windows")`). During `migrate`, cargo-buckal maps those predicates to a set of OS keys (`linux`/`macos`/`windows`) by evaluating them against cached `rustc --print=cfg --target <triple>` snapshots for Rust Tier-1 host targets.

If a predicate can’t be mapped to `linux`/`macos`/`windows`, cargo-buckal treats the dependency as unconditional by default (to preserve build success).

## Using it

1. Generate BUCK (and initialize Buck2 config on first run):

   ```bash
   cargo buckal migrate --buck2
   ```

   To update the pinned Buckal bundles revision (the `buckal` cell), rerun with:

   ```bash
   cargo buckal migrate --fetch
   ```

2. Build with `cargo buckal build`:

   Without `--target-platforms`, builds for the host platform:

   ```bash
   cargo buckal build //...
   ```

   With `--target-platforms`, builds for a specific target platform:

   ```bash
   cargo buckal build //... --target-platforms //platforms:x86_64-pc-windows-msvc
   ```

   You can also use `buck2 build` directly:

   ```bash
   buck2 build //... --target-platforms //platforms:x86_64-pc-windows-msvc
   ```

   `cargo buckal migrate --buck2` configures a `buckal` cell (Buckal bundles). The bundles provide sample platforms under `//platforms:*`. You can also use your own platform definitions; any platform you use must include the appropriate OS constraint value (`prelude//os/constraints:windows` in the example above) so `select()` picks up the right `os_deps` branch.

   If you want to use the bundled toolchain config too, point the `toolchains` cell at it in `.buckconfig`:

   ```ini
   [cells]
     toolchains = buckal/toolchains
   ```

3. Validate multi-platform builds by building against multiple target platforms:

   Linux:

   ```bash
   cargo buckal build //... --target-platforms //platforms:x86_64-unknown-linux-gnu
   ```

   Windows:

   ```bash
   cargo buckal build //... --target-platforms //platforms:x86_64-pc-windows-msvc
   ```

   macOS (bundled sample platforms):

   ```bash
   cargo buckal build //... --target-platforms //platforms:aarch64-apple-darwin
   ```

### Skipping tests for cross-compilation

When cross-compiling or when the target binaries cannot run on the host, you can
skip `rust_test` targets by passing `-c cross.skip_test=true`. cargo-buckal
marks generated `rust_test` targets with a `target_compatible_with` constraint
that matches the `//platforms:cross` config setting when this config is set.

Examples:

```bash
cargo buckal test //... --target-platforms //platforms:x86_64-unknown-linux-gnu -c cross.skip_test=true
cargo buckal test //... --target-platforms //platforms:x86_64-pc-windows-msvc -c cross.skip_test=true
```

## Troubleshooting

- If you see warnings about `rustc --print=cfg --target ...` failing, install the missing Rust targets (or expect fewer platform predicates to be mapped).
- If OS-specific deps appear in the default `deps` list, the corresponding predicate likely couldn’t be mapped; rerun with more Rust targets installed.
- If Buck2 fails to parse generated BUCK files due to missing support for `os_deps`/`os_named_deps` (or missing symbols like `rust_test` in `wrapper.bzl`), update the Buckal bundles (try `cargo buckal migrate --fetch`) or pin a bundles revision that supports these attributes.
