"""Implementation of the generic HIL test launcher."""

load(":transitions.bzl", "firmware_platform_transition", "runner_platform_transition")

def _rlocation_path(file, workspace_name):
    if file.short_path.startswith("../"):
        return file.short_path[3:]
    return workspace_name + "/" + file.short_path

def _single_file(target, name):
    files = target[DefaultInfo].files.to_list()
    if len(files) != 1:
        fail("HIL artifact '{}' must provide exactly one file, got {}".format(name, len(files)))
    return files[0]

def _configured_target(value, name):
    if type(value) == "list":
        if len(value) != 1:
            fail("HIL dependency '{}' resolved to {} configurations; expected one".format(name, len(value)))
        return value[0]
    return value

def _validate_name(name):
    if not name:
        fail("HIL artifact names must not be empty")
    for character in name.elems():
        if not (character.isalnum() or character == "_"):
            fail("invalid HIL artifact name '{}'; use letters, digits, and underscores".format(name))

def _merge_runfiles(runfiles, target):
    info = target[DefaultInfo]
    return runfiles.merge(info.default_runfiles).merge(info.data_runfiles)

def _hil_test_impl(ctx):
    launcher = ctx.actions.declare_file(ctx.label.name)
    launcher_binary = ctx.executable._launcher
    host = ctx.executable.host
    firmware = ctx.executable.firmware
    host_target = _configured_target(ctx.attr.host, "host")
    firmware_target = _configured_target(ctx.attr.firmware, "firmware")
    artifacts = {"firmware": firmware}
    artifact_targets = [firmware_target]

    for target, name in ctx.attr.host_tools.items():
        target = _configured_target(target, name)
        _validate_name(name)
        if name in artifacts:
            fail("duplicate HIL artifact name '{}'".format(name))
        artifacts[name] = _single_file(target, name)
        artifact_targets.append(target)

    for target, name in ctx.attr.runtime_data.items():
        _validate_name(name)
        if name in artifacts:
            fail("duplicate HIL artifact name '{}'".format(name))
        artifacts[name] = _single_file(target, name)
        artifact_targets.append(target)

    artifact_paths = {
        name: _rlocation_path(file, ctx.workspace_name)
        for name, file in artifacts.items()
    }
    for arg in ctx.attr.args:
        checked = arg
        for name in artifact_paths.keys():
            checked = checked.replace("{{{}}}".format(name), "")
        if "{" in checked or "}" in checked:
            fail("unknown or malformed HIL artifact placeholder in argument '{}'".format(arg))

    ctx.actions.symlink(
        output = launcher,
        target_file = launcher_binary,
        is_executable = True,
    )

    runfiles = ctx.runfiles(files = [launcher_binary, host] + artifacts.values())
    runfiles = _merge_runfiles(runfiles, host_target)
    for target in artifact_targets:
        runfiles = _merge_runfiles(runfiles, target)

    return [
        DefaultInfo(
            executable = launcher,
            runfiles = runfiles,
        ),
        RunEnvironmentInfo(
            environment = dict(ctx.attr.test_env, **{
                "HIL_RUNFILES_ARTIFACTS": "\n".join([
                "{}\t{}".format(name, path)
                for name, path in artifact_paths.items()
                ]),
                "HIL_RUNFILES_HOST": _rlocation_path(host, ctx.workspace_name),
            }),
            inherited_environment = ctx.attr.test_env_inherit,
        ),
    ]

_hil_test = rule(
    implementation = _hil_test_impl,
    attrs = {
        "firmware": attr.label(
            cfg = firmware_platform_transition,
            executable = True,
            mandatory = True,
        ),
        "firmware_compilation_mode": attr.string(default = "opt"),
        "firmware_platform": attr.label(mandatory = True),
        "host": attr.label(
            cfg = runner_platform_transition,
            executable = True,
            mandatory = True,
        ),
        "host_tools": attr.label_keyed_string_dict(
            allow_files = True,
            cfg = runner_platform_transition,
        ),
        "runner_platform": attr.label(mandatory = True),
        "runtime_data": attr.label_keyed_string_dict(allow_files = True),
        "test_env": attr.string_dict(),
        "test_env_inherit": attr.string_list(),
        "_launcher": attr.label(
            default = "//bazel/rules/hil:launcher",
            cfg = runner_platform_transition,
            executable = True,
        ),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    },
    doc = "Runs a local HIL host with explicitly transitioned firmware and host artifacts.",
    executable = True,
    test = True,
)

def hil_test(
        name,
        data = {},
        env = {},
        env_inherit = [],
        tags = [],
        size = "large",
        timeout = "long",
        **kwargs):
    """Creates an explicit local HIL test while allowing compilation through RBE."""
    _hil_test(
        name = name,
        runtime_data = data,
        test_env = env,
        test_env_inherit = env_inherit,
        size = size,
        tags = ["local", "hil", "resources:esp32_slots:1"] + tags,
        timeout = timeout,
        **kwargs
    )
