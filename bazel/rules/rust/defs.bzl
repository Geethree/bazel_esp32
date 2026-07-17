"""Workspace-level Rust dependency maintenance rules."""

_RUST_TOOLCHAIN_TYPE = Label("@rules_rust//rust:toolchain_type")

def _runfiles_path(file, workspace_name):
    if file.short_path.startswith("../"):
        return file.short_path[3:]
    return workspace_name + "/" + file.short_path

def _rust_lockfile_impl(ctx):
    launcher = ctx.actions.declare_file(ctx.label.name)
    toolchain = ctx.toolchains[_RUST_TOOLCHAIN_TYPE]
    cargo = toolchain.cargo
    if not cargo:
        fail("the selected Rust toolchain does not provide Cargo")

    ctx.actions.write(
        output = launcher,
        content = """#!/usr/bin/env bash
set -euo pipefail

workspace="${{BUILD_WORKSPACE_DIRECTORY:?run this target with bazel run}}"
runfiles_dir="${{RUNFILES_DIR:-${{BASH_SOURCE[0]}}.runfiles}}"
cargo="${{runfiles_dir}}/{cargo}"

exec "${{cargo}}" generate-lockfile --manifest-path "${{workspace}}/{manifest}"
""".format(
            cargo = _runfiles_path(cargo, ctx.workspace_name),
            manifest = ctx.file.manifest.short_path,
        ),
        is_executable = True,
    )

    return [DefaultInfo(
        executable = launcher,
        runfiles = ctx.runfiles(
            files = [cargo],
            transitive_files = toolchain.all_files,
        ),
    )]

rust_lockfile = rule(
    implementation = _rust_lockfile_impl,
    attrs = {
        "manifest": attr.label(
            allow_single_file = ["Cargo.toml"],
            mandatory = True,
        ),
    },
    doc = "Updates the root Cargo dependency lock through bazel run.",
    executable = True,
    toolchains = [_RUST_TOOLCHAIN_TYPE],
)
