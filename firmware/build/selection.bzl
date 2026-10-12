"""Select firmware dependencies from the chosen chip and physical board."""

_CHIPS = {
    "esp32c6": struct(
        constraint = "@rules_esp32//platforms:chip_esp32c6",
        crates = "firmware_c6_crates",
    ),
    "esp32s31": struct(
        constraint = "@rules_esp32//platforms:chip_esp32s31",
        crates = "firmware_s31_crates",
    ),
}

_BOARDS = {
    "xiao_esp32c6": struct(
        constraint = "//bazel/platforms:board_xiao_esp32c6",
        library = "//firmware/boards/xiao_esp32c6:board",
        partitions = "//firmware/boards/xiao_esp32c6:partitions.csv",
    ),
    "s31_function_coreboard_1": struct(
        constraint = "//bazel/platforms:board_s31_function_coreboard_1",
        library = "//firmware/boards/s31_function_coreboard_1:board",
        partitions = "//firmware/boards/s31_function_coreboard_1:partitions.csv",
    ),
}

_UNSUPPORTED = "select a supported firmware board platform with --platforms"
_UNSUPPORTED_CHIP = "select a supported ESP32 chip through a firmware board platform with --platforms"

# rules_rs also analyzes Rust library dependencies in an execution/host
# configuration for aspects. Firmware-only selectors use an empty host branch;
# portable dependencies use the root Cargo resolution there. Firmware and
# flash targets enforce the supported-board constraint themselves.

def chip_crates(*names):
    """Return versioned crates from the selected chip's Cargo resolution."""
    conditions = {
        chip.constraint: ["@{}//:{}".format(chip.crates, name) for name in names]
        for chip in _CHIPS.values()
    }
    conditions["//conditions:default"] = []
    return select(conditions)

def portable_crates(*names):
    """Resolve portable Rust dependencies for the chip, or for the host."""
    conditions = {
        chip.constraint: ["@{}//:{}".format(chip.crates, name) for name in names]
        for chip in _CHIPS.values()
    }
    conditions["//conditions:default"] = ["@crates//:{}".format(name) for name in names]
    return select(conditions)

def board():
    """Return the selected physical board adapter."""
    conditions = {
        physical.constraint: [physical.library]
        for physical in _BOARDS.values()
    }
    conditions["//conditions:default"] = []
    return select(conditions)

def selected_chip():
    return select({
        chip.constraint: name
        for name, chip in _CHIPS.items()
    }, no_match_error = _UNSUPPORTED_CHIP)

def selected_partitions():
    return select({
        physical.constraint: physical.partitions
        for physical in _BOARDS.values()
    }, no_match_error = _UNSUPPORTED)

def board_compatibility(*names):
    """Mark a firmware target compatible with its supported physical boards."""
    unknown = [name for name in names if name not in _BOARDS]
    if unknown:
        fail("unknown firmware board(s): {}; supported boards: {}".format(
            ", ".join(unknown),
            ", ".join(sorted(_BOARDS.keys())),
        ))
    conditions = {
        _BOARDS[name].constraint: []
        for name in names
    }
    conditions["//conditions:default"] = ["@platforms//:incompatible"]
    return select(conditions)
