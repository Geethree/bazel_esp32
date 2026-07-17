"""Implementation of the ESP32 flash rule."""

load(":boards.bzl", "board_config")
load(":transitions.bzl", "esp32_platform_transition")

def _runfiles_path(file, workspace_name):
    if file.short_path.startswith("../"):
        return file.short_path[3:]
    return workspace_name + "/" + file.short_path

def _shell_quote(value):
    return "'{}'".format(value.replace("'", "'\"'\"'"))

def _esp32_flash_impl(ctx):
    if ctx.attr.baud <= 0:
        fail("baud must be positive")

    launcher = ctx.actions.declare_file(ctx.label.name)
    firmware = ctx.executable.firmware
    espflash = ctx.file.espflash
    monitor_arg = "args+=(--monitor)" if ctx.attr.monitor else ""

    ctx.actions.write(
        output = launcher,
        content = """#!/usr/bin/env bash
set -euo pipefail

runfiles_dir="${{RUNFILES_DIR:-${{BASH_SOURCE[0]}}.runfiles}}"
espflash="${{runfiles_dir}}/{espflash}"
firmware="${{runfiles_dir}}/{firmware}"
default_port={port}
port="${{ESPFLASH_PORT:-${{default_port}}}}"

args=(flash --chip {chip} --baud {baud})
if [[ -n "${{port}}" ]]; then
  args+=(--port "${{port}}")
fi
{monitor_arg}

exec "${{espflash}}" "${{args[@]}}" "${{firmware}}"
""".format(
            baud = ctx.attr.baud,
            chip = ctx.attr.chip,
            espflash = _runfiles_path(espflash, ctx.workspace_name),
            firmware = _runfiles_path(firmware, ctx.workspace_name),
            monitor_arg = monitor_arg,
            port = _shell_quote(ctx.attr.port),
        ),
        is_executable = True,
    )

    return [DefaultInfo(
        executable = launcher,
        runfiles = ctx.runfiles(files = [espflash, firmware]),
    )]

_esp32_flash = rule(
    implementation = _esp32_flash_impl,
    attrs = {
        "baud": attr.int(default = 115200),
        "chip": attr.string(mandatory = True),
        "compilation_mode": attr.string(mandatory = True),
        "espflash": attr.label(
            allow_single_file = True,
            cfg = "exec",
            mandatory = True,
        ),
        "firmware": attr.label(
            cfg = esp32_platform_transition,
            executable = True,
            mandatory = True,
        ),
        "monitor": attr.bool(default = True),
        "platform": attr.label(mandatory = True),
        "port": attr.string(),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    },
    doc = "Flashes an ESP32 ELF locally with espflash and optionally monitors it.",
    executable = True,
)

def esp32_flash(name, board, espflash, firmware, **kwargs):
    """Creates a host flash target for firmware built for a supported board."""
    config = board_config(board)
    _esp32_flash(
        name = name,
        chip = config.chip,
        compilation_mode = config.compilation_mode,
        espflash = espflash,
        firmware = firmware,
        platform = config.platform,
        **kwargs
    )
