buildscript_run(
    name = "demo-build-script-run",
    package_name = "demo",
    buildscript_rule = ":demo-build-script-build",
    env = {"RUST_LOG": "debug"},
    env_srcs = [
        "//path/to/example:example-build-script-main-run[metadata]",
        ":manifest[env_dict]",
    ],
    features = [
        "alloc",
        "default",
    ],
    version = "1.2.3",
    manifest_dir = ":vendor",
    visibility = ["PUBLIC"],
)
