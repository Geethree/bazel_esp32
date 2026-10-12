# Firmware layout

`firmware` builds each application once for the physical board selected by
Bazel's `--platforms` flag. Bazel is the firmware build interface; the Cargo
workspaces supply chip-specific dependency resolution to `rules_rs`.

| Directory | Ownership |
| --- | --- |
| `apps/<app>/` | One shared Rust app library, one firmware binary, and one flash launcher. |
| `boards/api/` | Small interfaces used at real hardware differences. |
| `boards/<physical-board>/` | Pins, peripheral adapters, provisioning settings, and the flash partition table. |
| `chips/<chip>/` | Cargo workspace, lockfile, and chip feature sets shared by boards using that chip. |
| `services/provisioning/` | Portable provisioning logic and portal assets shared by the app and HIL. |
| `build/selection.bzl` | Platform selection of board adapters, chip crates, and flash metadata. |
| `hil/support/` | Generic `no_std` frame codec and opt-in host sessions. |
| `hil/provisioning/` | Host assertions and device images for manual provisioning tests. |

Host serial tools live in `tools/serial/`. Portable libraries have one
platform-configured Bazel label; only their external crate resolution changes
with the selected chip. No app or HIL target has a chip-suffixed copy.

The current physical boards are XIAO ESP32-C6 and ESP32-S31 Function
CoreBoard-1. Their public platforms are `//bazel/platforms:xiao_esp32c6` and
`//bazel/platforms:s31_function_coreboard_1`. Chip constraints and Rust target
triples are separate from physical board constraints, so another C6 board can
reuse the C6 Cargo resolution. Board identity selects adapters and partitions;
chip identity selects the Cargo resolution and Rust toolchain.

## Build and flash

```sh
bazel build --config=xiao_esp32c6 //firmware/apps/diagnostics:firmware
bazel build --platforms=//bazel/platforms:s31_function_coreboard_1 //firmware/apps/diagnostics:flash
bazel build --platforms=//bazel/platforms:xiao_esp32c6 //firmware/apps/provisioning:flash
bazel build --platforms=//bazel/platforms:s31_function_coreboard_1 //firmware/apps/provisioning:flash
```

The board-named `--config` also selects optimized compilation.
`--platforms` selects the target board directly.

The `:flash` target builds a launcher and its firmware without contacting a
device. The following command flashes a connected board:

```sh
bazel run --platforms=//bazel/platforms:s31_function_coreboard_1 //firmware/apps/diagnostics:flash
```

Set
`ESPFLASH_PORT` to select a serial device without putting its path in the
repository.

The app's ordinary `rust_binary` builds under the selected target platform.
`esp32_flash` keeps that platform for its firmware dependency, chooses
the chip and partition table from `build/selection.bzl`, and builds `espflash`
for the execution host.

The provisioning console is a separate host target. On the S31 Function
CoreBoard-1 it uses UART0 through the board's USB-to-UART bridge; select that
port explicitly after flashing:

```sh
bazel run //tools/serial/console:console -- --port /dev/cu.YOUR_S31_UART_PORT
```

The C6 provisioning console uses USB Serial/JTAG and is discovered
automatically by the same host target.

## Add a board or app

For another physical board on a supported chip:

1. Add a platform with the chip platform as parent and a new physical-board
   constraint under `bazel/platforms/`.
2. Add `boards/<physical-board>/` with its adapter library and partition table.
3. Add its board constraint, library label, and partition label to
   `build/selection.bzl`. The inherited chip constraint selects the chip's
   Cargo resolution.
4. Add an optional board-named `.bazelrc` shortcut for platform and optimized
   builds.
5. Add the board to each app's `board_compatibility(...)` list only after that
   app's hardware paths have been compiled and validated for it.
6. Extend the build and manual HIL matrix. Reuse the chip Cargo workspace.

A new chip also needs its own Cargo resolution in `chips/`, a chip platform,
toolchain support, and any crate adaptations described in
`bazel/modules/esp32/README.md`. Update the chip entries in
`build/selection.bzl` and `MODULE.bazel`. Existing portable library targets
require no changes. Keep distinct chip features out of the other chip's Cargo
workspace.

A new app adds one package under `apps/`, with one binary entrypoint. Its
chip-specific Cargo dependency declarations belong in the member manifests
of the chips that support it. These manifests resolve dependencies; they do
not create board-specific firmware source copies. Extract app modules into
shared Rust libraries when another app actually uses them.

Provisioning builds for both boards with the same `esp-radio` version. The
build checks compilation and launcher generation; association, persistence,
and portal behavior on S31 still require a physical HIL run.
