"""Supported ESP32 board configurations."""

_BOARDS = {
    "esp32c6": struct(
        chip = "esp32c6",
        compilation_mode = "opt",
        platform = Label("//platforms:esp32c6"),
    ),
}

def board_config(name):
    """Returns the immutable configuration for a supported board name."""
    board = _BOARDS.get(name)
    if not board:
        fail("unsupported ESP32 board '{}'; supported boards: {}".format(
            name,
            ", ".join(sorted(_BOARDS.keys())),
        ))
    return board
