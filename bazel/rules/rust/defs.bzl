"""Workspace-level Rust dependency maintenance rules."""

_RUST_TOOLCHAIN_TYPE = Label("@rules_rust//rust:toolchain_type")

_RUNNER_TOOLCHAINS = {
    "linux_aarch64": [
        "@platforms//os:linux",
        "@platforms//cpu:aarch64",
    ],
    "linux_x86_64": [
        "@platforms//os:linux",
        "@platforms//cpu:x86_64",
    ],
    "macos_aarch64": [
        "@platforms//os:macos",
        "@platforms//cpu:aarch64",
    ],
}

_RUNNER_CONSTRAINT_ATTRS = {
    "linux_aarch64": ["_linux_constraint", "_aarch64_constraint"],
    "linux_x86_64": ["_linux_constraint", "_x86_64_constraint"],
    "macos_aarch64": ["_macos_constraint", "_aarch64_constraint"],
}

def _runfiles_path(file, workspace_name):
    if file.short_path.startswith("../"):
        return file.short_path[3:]
    return workspace_name + "/" + file.short_path

def _rust_lockfile_impl(ctx):
    launcher = ctx.actions.declare_file(ctx.label.name)
    runner = None
    for name, constraint_attrs in _RUNNER_CONSTRAINT_ATTRS.items():
        if all([
            ctx.target_platform_has_constraint(getattr(ctx.attr, constraint_attr)[platform_common.ConstraintValueInfo])
            for constraint_attr in constraint_attrs
        ]):
            runner = name
            break
    if not runner:
        fail("rust_lockfile does not support the selected runner platform")

    toolchain = ctx.exec_groups[runner].toolchains[_RUST_TOOLCHAIN_TYPE]
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
        "_aarch64_constraint": attr.label(default = "@platforms//cpu:aarch64"),
        "_linux_constraint": attr.label(default = "@platforms//os:linux"),
        "_macos_constraint": attr.label(default = "@platforms//os:macos"),
        "_x86_64_constraint": attr.label(default = "@platforms//cpu:x86_64"),
        "manifest": attr.label(
            allow_single_file = ["Cargo.toml"],
            mandatory = True,
        ),
    },
    doc = "Updates the root Cargo dependency lock through bazel run.",
    exec_groups = {
        name: exec_group(
            exec_compatible_with = constraints,
            toolchains = [_RUST_TOOLCHAIN_TYPE],
        )
        for name, constraints in _RUNNER_TOOLCHAINS.items()
    },
    executable = True,
)
