"""Configuration transitions used by ESP32 device rules."""

def _esp32_platform_transition_impl(_settings, attr):
    return {
        "//command_line_option:compilation_mode": attr.compilation_mode,
        "//command_line_option:platforms": str(attr.platform),
    }

esp32_platform_transition = transition(
    implementation = _esp32_platform_transition_impl,
    inputs = [],
    outputs = [
        "//command_line_option:compilation_mode",
        "//command_line_option:platforms",
    ],
)
