# rules_esp32

`rules_esp32` is the in-repository Bazel module for hermetic Rust firmware on
ESP32 devices. It owns the board platforms, board configuration, flash rule,
pinned host flashing tool, Rust target policy, ESP-specific crate adaptations,
and the permanent rules_rs compatibility patches.

The public firmware rule remains rules_rs's ordinary `rust_binary`. Board
selection happens through Bazel platforms and configurable attributes rather
than a board-specific Rust rule.

## Supported stack

| Component | Supported version |
| --- | --- |
| Bazel | 9.x |
| rules_rs | 0.0.98 plus the module's four ordered patches |
| Rust | 1.96.1, edition 2024 |
| Target | `riscv32imac-unknown-none-elf` |
| Board/chip | ESP32-C6 |
| esp-hal | 1.1.1 |
| esp-config | 0.7.0 |
| esp-metadata-generated | 0.4.0 |
| espflash | 4.5.0 |

The espflash repository supports Linux and macOS on `x86_64` and `aarch64`.
Unsupported hosts fail during repository setup with the exact OS and CPU.

Versions are intentionally explicit. Updating one requires updating any
versioned crate adaptation or patch that depends on it, followed by the full
validation matrix below.

## Module boundary

The root workspace has three small integration points:

```bzl
bazel_dep(name = "rules_esp32", version = "0.1.0")

local_path_override(
    module_name = "rules_esp32",
    path = "bazel/modules/esp32",
)

include("//bazel/modules/esp32:rules_rs_override.MODULE.bazel")

esp32 = use_extension("@rules_esp32//:extensions.bzl", "esp32")
use_repo(esp32, "espflash")
```

The root `.bazelrc` imports the module-owned board configs:

```bazelrc
import %workspace%/bazel/modules/esp32/esp32.bazelrc
```

The module extension creates `@espflash` from a pinned, SHA-256-verified
release archive. Host OS and CPU are explicit repository attributes, so the
module lock cannot reuse a binary chosen for another host.

The included rules_rs override is the only dependency policy that cannot live
inside the dependency module itself. Bzlmod honors version and patch overrides
only when the root module declares them. Keeping the root-only fragment and
patch files here makes that unavoidable boundary visible and atomic.

## Rust toolchain and crate ownership

`rules_esp32/MODULE.bazel` contributes the Rust toolchain configuration. The
RV32IMAC target receives the linker script, size optimization, and frame-pointer
flags through rules_rs's target-triple-specific toolchain flags. Consequently,
direct platform builds and transitioned flash builds use the same compiler
policy without repeating `rustc_flags` on every firmware target.

The root still declares `crate.from_cargo` because the root owns `Cargo.toml`
and `Cargo.lock`. The two ESP-specific `crate.annotation` tags live in
`rules_esp32/MODULE.bazel`, beside their versioned additive BUILD files. Patch 4
allows a dependency module to contribute annotations to the named root Cargo
repository. This keeps host build-script features separate from ESP target
`no_std` features without leaking ESP package internals into the root module.

## Firmware targets

Use the normal rules_rs rule and a module-provided `config_setting`:

```bzl
load("@crates//:defs.bzl", "aliases", "all_crate_deps")
load("@rules_esp32//:defs.bzl", "esp32_flash")
load("@rules_rs//rs:rust_binary.bzl", "rust_binary")

rust_binary(
    name = "hello",
    srcs = ["src/main.rs"],
    aliases = aliases(),
    deps = all_crate_deps(normal = True),
    target_compatible_with = select({
        "@rules_esp32//platforms:is_esp32c6": [],
        "//conditions:default": ["@platforms//:incompatible"],
    }),
)

esp32_flash(
    name = "flash",
    board = "esp32c6",
    espflash = "@espflash//:espflash",
    firmware = ":hello",
)
```

The same board `config_setting` can select board-specific sources,
dependencies, crate features, data, or other configurable rule attributes as
more boards are added. The custom chip constraint also prevents firmware from
being accidentally analyzed as a host binary.

`esp32_flash` is a small board-aware macro over a private executable rule. It
transitions only the firmware dependency to the selected board platform and
optimized compilation mode. The launcher and USB access remain local while
firmware compilation may execute remotely.

The rule uses espflash's serial-port auto-detection by default. Set
`ESPFLASH_PORT=/dev/...` to choose a device without committing a
workstation-specific path. The default baud is the conservative 115,200 rate;
targets may opt into a higher `baud` after validating their USB path and board.

```sh
bazel build --config=esp32c6 //firmware:hello
bazel run //firmware:flash
```

## Adding a board

Adding a board is an explicit module change:

1. Add its chip constraint, platform, and `is_<board>` `config_setting` under
   `platforms/BUILD.bazel`.
2. Add its flash chip, compilation mode, and platform label to
   `private/boards.bzl`.
3. Add a named config to `esp32.bazelrc`.
4. Add any target-triple compiler flags to the toolchain tag in
   `MODULE.bazel`.
5. Extend `platform_triples` in the root Cargo resolver when the board uses a
   new Rust triple.
6. Add or version any crate adaptations and run the validation matrix.

No new Rust rule is required. Firmware targets opt into the new board through
`select()` and the named Bazel config.

## rules_rs compatibility layer

The four ordered patches under `patches/rules_rs` are part of this module's
pinned dependency contract:

1. Add `crate.annotation(override_target_lib = ...)`.
2. Normalize RISC-V ISA triple names to Rust `cfg(target_arch)` values.
3. Merge crate annotations contributed by dependency modules into named Cargo
   repositories.
4. Avoid an `esp-phy` build-script data cycle.

RV32IMAC target support and hermetic `rust-lld` selection are provided directly
by rules_rs 0.0.98.

Removing or reordering a patch, changing rules_rs, or changing an adapted crate
version must fail validation before it is accepted.

## Validation

Run after changing the module, patches, tools, crates, Rust version, or board
configuration:

```sh
bazel mod tidy
bazel query '@rules_esp32//... + @espflash//:espflash'
bazel build //...
bazel build --config=esp32c6 //firmware:hello
bazel build //firmware:flash
bash -n bazel-bin/firmware/flash
file bazel-bin/firmware/hello
```

`bazel build //firmware:flash` proves the board transition without accessing
USB. Actual flashing is intentionally a separate local operation.
