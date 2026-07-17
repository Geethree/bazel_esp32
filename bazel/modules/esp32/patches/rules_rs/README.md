# rules_rs ESP32-C6 patches

This directory carries four ordered, pinned patches for
[`rules_rs`](https://github.com/hermeticbuild/rules_rs) so its hermetic Rust
toolchains can target the ESP32-C6.

## Provenance

- rules_rs release: `v0.0.96`
- Release base commit: `87811fbf13177eff8f118a808610fe3a4fd0eed7`
- Patch 1 commit: `c5ebcf0` (`feat: support bare-metal RV32IMAC Rust targets`)
- Patch 2 commit: `b58037d` (`feat: allow overriding generated crate library targets`)
- Patch 3 commit: `2f110e1` (`fix: normalize RISC-V target arch in Cargo cfgs`)
- Patch 4 commit: `4f2f46c` (`feat: allow dependency modules to annotate crate repositories`)

The patches were authored as separate commits against that release, then
exported with `git format-patch`. The root module includes
`//bazel/modules/esp32:rules_rs_override.MODULE.bazel`, which applies them to
the released BCR module with `single_version_override`.

## What patch 1 adds

The ESP32-C6 uses the stable Rust target
`riscv32imac-unknown-none-elf`. The patch:

1. Enables that target in the rules_rs supported-target matrix.
2. Adds an RV32 ISA constraint so RV32IMAC and the existing RV32IMC target do
   not collapse to the same Bazel `riscv32`/`none` constraint set.
3. Selects the Rust distribution's hermetic `rust-lld` for both bare-metal
   RV32 targets. With no target C toolchain present, rules_rust uses that
   linker directly for pure-Rust firmware.

The module platform `@rules_esp32//platforms:esp32c6` inherits the patched target
platform. `//firmware:hello` exercises the complete target: `no_std`
compilation, esp-hal runtime and linker scripts, boot image metadata, and
hermetic `rust-lld` linking.

The target platform is independent from the execution platform. The local
`rbe` config chooses a Linux remote execution platform but does not set
`--platforms`; passing `--config=esp32c6` chooses the ESP32-C6 target. Use
`--config=linux_x86_64_gnu` when a build should instead target Linux.

## What patch 2 adds

rules_rs v0.0.96 generates one library target per Cargo package. That is not
enough for `esp-config`: host build scripts need its `build` feature and `std`,
while ESP32-C6 target libraries need the same package without that feature and
as `no_std`. `esp-metadata-generated` has the same host/target split around its
`build-script` feature.

Patch 2 adds the generic `crate.annotation(override_target_lib = ...)` hook.
It lets a workspace redirect a generated hub alias to an alternate target in
the crate repository. The ESP-specific additive target and platform `select`
remain in `@rules_esp32//crates`; host dependencies keep the generated targets and
RV32IMAC dependencies receive their no_std variants.

Patch 2 is deliberately a separate patch on top of patch 1. It does not amend
or replace the target/toolchain change.

## What patch 3 fixes

Cargo target predicates use Rust's `cfg(target_arch = "riscv32")`, while a
RISC-V target triple encodes ISA extensions in an architecture component such
as `riscv32imac`. Patch 3 normalizes `riscv32*` and `riscv64*` triple
components to Rust's corresponding `target_arch` values. Without that
normalization, rules_rs drops RISC-V-only dependencies such as `riscv` from
generated targets.

Patch 3 is also a separate commit on top of patches 1 and 2.

## What patch 4 adds

The root module owns `crate.from_cargo` because it owns the workspace Cargo
manifest and lockfile. The ESP32 module owns the ESP-specific crate
adaptations. Patch 4 allows modules that use the same crate extension to
contribute annotation tags without declaring another `from_cargo` repository,
then merges those tags by repository name.

Annotations remain explicitly repository-scoped and duplicate crate/version
annotations still fail. Unit tests cover cross-module merging and repository
filtering.

Patch 4 is a separate commit on top of patches 1 through 3.

## Reproduce and validate

From a rules_rs checkout:

```sh
git switch esp32c6-riscv32imac
bazel test //rs/private:annotations_tests //rs/private:cfg_parser_tests
cd test && bazel test //:target_triple_constraints_test
```

The test passed against `v0.0.96` with Rust `1.96.1`. It checks that every
supported target triple, including RV32IMAC, resolves to a distinct generated
platform/toolchain constraint set.

The workspace firmware target also builds successfully. The downloaded output
identifies as a statically linked, 32-bit little-endian RISC-V ELF with RVC and
soft-float ABI.

To regenerate the vendored patch series from its corresponding commits, run
this from the workspace root and replace `<rules_rs-checkout>` with the source
checkout:

```sh
git -C <rules_rs-checkout> format-patch -4 \
  --output-directory "$PWD/bazel/modules/esp32/patches/rules_rs"
```

Treat the patches and the pinned rules_rs version as one atomic dependency.
When changing rules_rs, regenerate or rebase the patches in order and run the
module validation matrix before updating the version in either MODULE file.
