rust_library(
    name = "with_select",
    srcs = ["src/lib.rs"],
    crate = "with_select",
    crate_root = "src/lib.rs",
    edition = "2024",
    rustc_flags = ["@$(location :manifest[env_flags])"] + select({
        "prelude//os/constraints:windows": select({
            "prelude//abi/constraints:gnu": [
                "@$(location //third-party/rust/crates/windows_x86_64_gnu/0.48.5:build-script-run[rustc_flags])",
            ],
            "DEFAULT": [
                "@$(location //third-party/rust/crates/windows_x86_64_msvc/0.48.5:build-script-run[rustc_flags])",
            ],
        }),
        "DEFAULT": [],
    }),
    visibility = ["PUBLIC"],
)

rust_library(
    name = "with_select_reversed",
    srcs = ["src/lib.rs"],
    crate = "with_select_reversed",
    crate_root = "src/lib.rs",
    edition = "2024",
    rustc_flags = select({
        "prelude//os/constraints:windows": [
            "@$(location //third-party/rust/crates/windows:build-script-run[rustc_flags])",
        ],
        "DEFAULT": [],
    }) + ["@$(location :manifest[env_flags])"],
    visibility = ["PUBLIC"],
)

rust_library(
    name = "merge_both_sides",
    srcs = ["src/lib.rs"],
    crate = "merge_both_sides",
    crate_root = "src/lib.rs",
    edition = "2024",
    rustc_flags = ["@$(location //third-party/rust/crates/windows:build-script-run[rustc_flags])"] + ["@$(location :manifest[env_flags])"],
    visibility = ["PUBLIC"],
)
