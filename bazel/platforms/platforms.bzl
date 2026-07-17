"""Target platforms for the OpenShell release build matrix."""

PLATFORMS = {
    "linux_x86_64_gnu": {
        "exec_properties": {
            "Arch": "amd64",
            "OSFamily": "Linux",
        },
        "parent": "@llvm//platforms:linux_x86_64",
    },
    "linux_aarch64_gnu": {
        "parent": "@llvm//platforms:linux_aarch64",
    },
    "macos_aarch64": {
        "parent": "@llvm//platforms:macos_aarch64",
    },
}

def register_platforms():
    """Registers all supported target platforms."""
    for name, config in PLATFORMS.items():
        native.platform(
            name = name,
            constraint_values = config.get("constraint_values", []),
            exec_properties = config.get("exec_properties", {}),
            parents = [config["parent"]],
            visibility = ["//visibility:public"],
        )
