"""Dependency transitions for multi-platform HIL tests."""

def _firmware_platform_transition_impl(_settings, attr):
    return {
        "//command_line_option:compilation_mode": attr.firmware_compilation_mode,
        "//command_line_option:platforms": str(attr.firmware_platform),
    }

firmware_platform_transition = transition(
    implementation = _firmware_platform_transition_impl,
    inputs = [],
    outputs = [
        "//command_line_option:compilation_mode",
        "//command_line_option:platforms",
    ],
)

def _runner_platform_transition_impl(_settings, attr):
    return {
        "//command_line_option:platforms": str(attr.runner_platform),
    }

runner_platform_transition = transition(
    implementation = _runner_platform_transition_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms"],
)
