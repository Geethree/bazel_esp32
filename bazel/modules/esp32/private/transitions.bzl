"""Configuration transitions used by ESP32 device rules."""

def _firmware_mode_transition_impl(_settings, attr):
    return {"//command_line_option:compilation_mode": attr.compilation_mode}

firmware_mode_transition = transition(
    implementation = _firmware_mode_transition_impl,
    inputs = [],
    outputs = ["//command_line_option:compilation_mode"],
)
