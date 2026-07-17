"""Bzlmod extensions for ESP32 host tooling."""

load("//private:repositories.bzl", "espflash_repository")

def _esp32_impl(module_ctx):
    espflash_repository(
        name = "espflash",
        host_arch = module_ctx.os.arch,
        host_os = module_ctx.os.name,
    )

    return module_ctx.extension_metadata(
        root_module_direct_deps = ["espflash"],
        root_module_direct_dev_deps = [],
    )

esp32 = module_extension(
    implementation = _esp32_impl,
    doc = "Provides the pinned espflash binary for the current host.",
)
