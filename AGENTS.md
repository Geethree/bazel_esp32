# Repository Guide for Coding Agents

## Scope

This repository builds bare-metal Rust firmware for ESP32 devices with Bazel. Bazel is the authoritative build interface; use Cargo manifests to declare Rust packages and dependencies, but do not treat a successful host `cargo build` as firmware validation.

The currently supported target is ESP32-C6 (`riscv32imac-unknown-none-elf`). Firmware is `no_std` and uses Rust 2024.

## Repository Layout

- `firmware/`: application manifests, Bazel targets, and firmware source.
- `bazel/modules/esp32/`: the in-repository `rules_esp32` module. It owns board platforms, toolchain policy, flashing, pinned host tools, crate adaptations, and `rules_rs` patches.
- `bazel/rules/rust/`: workspace-level Rust maintenance rules.
- `bazel/platforms/`: host platform registration.
- `bazel/rules/hil/`: generic multi-platform HIL test orchestration.
- `crates/hil-testing/`: reusable `no_std` framing and opt-in host HIL sessions.
- `crates/serial-transport/`: shared host serial discovery and reconnect logic.
- `firmware/hil/`: concrete manual hardware test suites.
- `MODULE.bazel`: root Bzlmod dependencies, Cargo resolution, extensions, and toolchain registration.
- `.bazelrc`: shared Bazel configuration. Put workstation-specific settings in the ignored `.bazelrc.user`.

Read `bazel/modules/esp32/README.md` before changing the embedded toolchain, board support, dependency patches, or flash rules.

## Build and Validation

Run commands from the repository root.

```sh
# Build every target compatible with the host configuration.
bazel build //...

# Compile the firmware for the ESP32-C6 platform.
bazel build --config=esp32c6 //firmware:firmware

# Build the host-side flash launcher and transitioned firmware without touching hardware.
bazel build //firmware:flash

# Flash a connected board and start its monitor.
bazel run //firmware:flash

# Select a serial device without committing a machine-specific path.
ESPFLASH_PORT=/dev/... bazel run //firmware:flash
```

Prefer the smallest relevant build while iterating, then run the broader checks appropriate to the files changed. `bazel build //firmware:flash` is safe in automated validation; `bazel run //firmware:flash` is a hardware operation and must not be run unless the user explicitly asks to flash a connected device.

For module, toolchain, patch, crate, Rust-version, or board-configuration changes, run the full validation matrix documented in `bazel/modules/esp32/README.md`:

```sh
bazel mod tidy
bazel query '@rules_esp32//... + @espflash//:espflash'
bazel build //...
bazel build --config=esp32c6 //firmware:firmware
bazel build //firmware:flash
bash -n bazel-bin/firmware/flash
file bazel-bin/firmware/firmware
```

Report any check that could not run, including whether the reason was missing hardware, unavailable USB access, host incompatibility, or dependency/network access.

## Bazel Conventions

- Keep builds hermetic. Do not introduce dependencies on a developer's system Rust, Cargo, compiler, SDK, serial-port path, or locally installed `espflash`.
- Use Bzlmod (`MODULE.bazel`) rather than adding a legacy `WORKSPACE` dependency path.
- Firmware remains a normal `rust_binary`; board selection belongs in Bazel platforms, constraints, transitions, and configurable attributes.
- Keep firmware targets incompatible with the default host platform. Follow the existing `target_compatible_with` pattern using `@rules_esp32//platforms:is_esp32c6`.
- Load public ESP32 rules through `@rules_esp32//:defs.bzl`. Keep rule implementations and board tables under the module's `private/` boundary.
- Do not duplicate target-specific linker or optimization flags on individual firmware targets; toolchain policy belongs in `bazel/modules/esp32/MODULE.bazel`.
- Keep BUILD and `.bzl` files formatted in standard Buildifier style. Preserve explicit visibility and labels.
- Do not edit generated Bazel output trees (`bazel-*`) or Cargo output (`target/`).

## Rust and Dependency Changes

- Preserve `#![no_std]` and `#![no_main]` for firmware binaries unless a target's runtime model is intentionally being redesigned.
- Avoid allocation, filesystem, networking, threads, wall-clock assumptions, and other host facilities unless the selected embedded stack explicitly provides them.
- Keep panic and logging paths suitable for early bring-up; do not assume peripherals are fully initialized in failure paths.
- Format Rust through the repository's hermetic Bazel tool, not a system `rustfmt` or `cargo fmt`:

  ```sh
  # Format all Rust source in the workspace.
  bazel run //tools/rustfmt

  # Format one or more files or paths.
  bazel run //tools/rustfmt -- ./path/to/file.rs
  ```

- Run Rustfmt after changing Rust source. Keep formatting-only changes limited to files in scope for the task.
- Keep Clippy clean for changed Rust targets. Treat Clippy warnings as issues to fix; use a narrowly scoped `#[allow(...)]` only when the lint is intentionally inapplicable, and include a short reason.
- Use the repository's Bazel-provided Rustfmt and Clippy components. Do not bypass the pinned toolchain with locally installed Cargo tools. If no Bazel Clippy target or documented invocation is available, report that validation gap rather than substituting a host `cargo clippy` result.
- Declare application dependencies in the appropriate `Cargo.toml`, then regenerate the lockfile through the hermetic Bazel target:

  ```sh
  bazel run //:update_lock
  bazel mod tidy
  ```

- Commit `Cargo.lock` and `MODULE.bazel.lock` changes when dependency resolution changes.
- Keep ESP crate features chip-specific and `no_std`-compatible. Check that build-script features and target-library features remain separated by the existing crate annotations.
- Treat the ordered `rules_rs` patches as part of the pinned toolchain contract. If changing `rules_rs`, Rust, an adapted crate, or a patch, review all four patches and their README together.

## Embedded and Hardware Safety

- Compilation and launcher generation are non-hardware operations; flashing, erasing, resetting, and monitoring interact with a physical device.
- Never flash, erase, change fuses, modify boot configuration, or select an attached serial device without explicit user authorization.
- Never add workstation-specific ports, USB identifiers, credentials, Wi-Fi secrets, MAC addresses, or other device-specific values to tracked files. Use environment variables or ignored local configuration.
- Run physical HIL tests only when explicitly requested. Individual tests use `//firmware/hil/<suite>:test`; `//firmware/hil:tests` runs the full explicitly enumerated hardware suite.
- Treat eFuse writes and security settings as irreversible. Do not add or execute such operations unless the user clearly requests the exact change and the target device has been confirmed.
- Prefer bounded waits and timeouts in firmware. Busy loops are acceptable only when deliberate; explain timing, watchdog, power, and CPU-use implications when introducing one.
- When modifying clocks, interrupts, unsafe register access, linker scripts, memory maps, or boot metadata, document the hardware assumption and validate against the precise chip and revision in scope.
- Keep `unsafe` blocks small and state the invariant that makes each one sound.

## Adding Board Support

Follow the checklist in `bazel/modules/esp32/README.md`: add the chip constraint/platform/config setting, board configuration, named Bazel config, target-triple flags, root Cargo platform triple when needed, and versioned crate adaptations. Extend the existing board model; do not create a separate board-specific Rust rule.

## Change Discipline

- Make focused changes and preserve unrelated working-tree edits.
- Update documentation and examples when public labels, board names, environment variables, or workflows change.
- Prefer tests or analysis-time validation for Starlark behavior. Error messages should identify the invalid value and the supported alternatives.
- In the final handoff, summarize affected firmware/toolchain behavior, list the exact Bazel checks run, and call out whether any operation was tested on physical hardware.
