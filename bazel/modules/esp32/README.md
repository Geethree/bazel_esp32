# rules_esp32

`rules_esp32` supplies the Rust firmware toolchains, chip platforms, pinned
flashing tool, crate adaptations, and `esp32_flash` rule. Application source,
physical board definitions, and chip-specific Cargo workspaces live under
[`firmware/`](../../../firmware/README.md).

## Supported build stack

| Component | Version |
| --- | --- |
| Bazel | 9.x |
| rules_rs | 0.0.98 plus five ordered patches |
| Rust | 1.96.1, edition 2024 |
| C6 target | `riscv32imac-unknown-none-elf` |
| S31 target | `riscv32imafc-unknown-none-elf` |
| esp-hal | 1.2.2 |
| esp-config | 0.8.0 |
| esp-metadata-generated | 0.5.3 |
| esp-radio | 1.0.0-beta.1 |
| espflash | 4.5.0 |

The C6 and S31 Cargo workspaces in `firmware/chips/` are separate because
chip features must not unify in one Cargo resolution. The root Cargo workspace
resolves host libraries and tools. `MODULE.bazel` creates one `rules_rs` crate
repository for each resolution and registers the toolchains.

`rules_esp32/MODULE.bazel` supplies target-triple linker and optimization
flags. Firmware remains an ordinary `rust_binary`; individual targets do not
repeat compiler or linker policy. Crate annotations in this module keep host
build-script features separate from target `no_std` library features.

The root module includes `rules_rs_override.MODULE.bazel` because Bzlmod
requires version and patch overrides at the root. The ordered patches under
`patches/rules_rs/` are part of the pinned toolchain contract:

1. Override a generated crate's target library.
2. Normalize RISC-V target architecture names for Cargo cfg handling.
3. Let dependency modules annotate root Cargo repositories.
4. Avoid an `esp-phy` build-script data cycle.
5. Add the S31 RV32IMAFC target.

Review the patch files and their README together when changing `rules_rs`,
Rust, or an adapted crate.

## Platforms and flash rule

The module defines **chip** constraints and Rust target platforms in
`platforms/BUILD.bazel`. The root `bazel/platforms/BUILD.bazel` defines
**physical board** constraints and platforms that inherit from those chip
platforms, alongside the host execution platforms registered by `MODULE.bazel`.
This lets a second C6 board use the C6 toolchain and Cargo graph while
selecting different pins and a partition table. Firmware selects directly on
chip and board constraint values; one-value `config_setting` wrappers are not
needed.

`esp32_flash` is a public executable rule loaded from
`@rules_esp32//:defs.bzl`. It preserves the caller's target platform for its
firmware dependency and transitions only compilation mode to `opt`. Its
configurable `chip` and `partition_table` attributes come from
`//firmware/build:selection.bzl`; `espflash` builds for the execution host.
The launcher only touches a device when run, not when built.

```bzl
load("@rules_esp32//:defs.bzl", "esp32_flash")
load("//firmware/build:selection.bzl", "board_compatibility", "selected_chip", "selected_partitions")

esp32_flash(
    name = "flash",
    chip = selected_chip(),
    espflash = "@espflash//:espflash",
    firmware = ":firmware",
    partition_table = selected_partitions(),
    target_compatible_with = board_compatibility("xiao_esp32c6", "s31_function_coreboard_1"),
)
```

`ESPFLASH_PORT=/dev/...` selects a serial device without recording a local
path in a tracked file. A target may set `monitor = False` or an explicit
positive `baud`; the default baud is 115200.

## Add support

For another physical board on an existing chip, add its constraint and
platform under `bazel/platforms/`, its adapter and partitions under
`firmware/boards/`, and one entry in `firmware/build/selection.bzl`. Reuse the
chip workspace in `firmware/chips/`. Add the board to each app's compatibility
list only after validating its hardware paths.

A new chip also needs a chip constraint/platform here, target-triple toolchain
flags in `MODULE.bazel`, a Cargo workspace and `crate.from_cargo` entry, and
any versioned crate adaptations. Add a physical board platform, a board-named
`.bazelrc` config, and build matrix entries. No board-specific Rust rule is
needed.

## Validation

After module, toolchain, crate, patch, or board-configuration changes:

```sh
bazel run //:update_lock
bazel run //firmware/chips/esp32c6:update_lock
bazel run //firmware/chips/esp32s31:update_lock
bazel mod tidy
bazel query '@rules_esp32//... + @espflash//:espflash'
bazel build //...
bazel build --platforms=//bazel/platforms:xiao_esp32c6 //firmware/apps/diagnostics:flash //firmware/apps/provisioning:flash
bazel build --platforms=//bazel/platforms:s31_function_coreboard_1 //firmware/apps/diagnostics:flash //firmware/apps/provisioning:flash
bazel build //firmware/hil/provisioning:test_xiao_esp32c6 //firmware/hil/provisioning:test_s31_function_coreboard_1
```

Both chip resolutions pin `esp-radio` 1.0.0-beta.1. The S31 Wi-Fi capability
metadata comes from `esp-metadata-generated` 0.5.3. Build commands do not
flash or run a physical HIL test.
