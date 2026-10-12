# rules_rs ESP32-C6 patches

This directory carries five ordered, pinned patches for
[`rules_rs`](https://github.com/hermeticbuild/rules_rs) so its hermetic Rust
toolchains can target the ESP32-C6.

## Provenance

- rules_rs release: `v0.0.98`
- Release base commit: `68d13bf398efb349092e86b714646f8fcdd12f9c`
- Patch 2 commit: `b58037d` (`feat: allow overriding generated crate library targets`)
- Patch 3 commit: `476eafe` (`fix: normalize RISC-V target arch in Cargo cfgs`)
- Patch 4 commit: `4f2f46c` (`feat: allow dependency modules to annotate crate repositories`)
- Patch 5: `fix: avoid esp-phy build-script data cycle`
- Patch 6: add the RV32IMAFC target and linker for ESP32-S31

Patches 2 and 4 retain their commit provenance from the series originally
authored against v0.0.96. Patch 3 was regenerated from its standalone upstream
commit, and patches 5 and 6 were added locally. All five apply cleanly to v0.0.98. The
root module includes `//bazel/modules/esp32:rules_rs_override.MODULE.bazel`,
which applies them to the released BCR module with `single_version_override`.

## What patch 2 adds

rules_rs v0.0.98 generates one library target per Cargo package. That is not
enough for `esp-config`: host build scripts need its `build` feature and `std`,
while ESP32-C6 target libraries need the same package without that feature and
as `no_std`. `esp-metadata-generated` has the same host/target split around its
`build-script` feature.

Patch 2 adds the generic `crate.annotation(override_target_lib = ...)` hook.
It lets a workspace redirect a generated hub alias to an alternate target in
the crate repository. The ESP-specific additive target and platform `select`
remain in `@rules_esp32//crates`; host dependencies keep the generated targets and
RV32IMAC dependencies receive their no_std variants.

RV32IMAC target and toolchain support are provided directly by rules_rs.

## What patch 3 fixes

Cargo target predicates use Rust's `cfg(target_arch = "riscv32")`, while a
RISC-V target triple encodes ISA extensions in an architecture component such
as `riscv32imac`. Patch 3 normalizes `riscv32*` and `riscv64*` triple
components to Rust's corresponding `target_arch` values. Without that
normalization, rules_rs drops RISC-V-only dependencies such as `riscv` from
generated targets.

The cfg-parser tests cover both an RV32IMAC bare-metal triple and an RV64GC
Linux musl triple, including negative assertions for the ISA-bearing names.

Patch 3 is applied after patch 2 but comes from an independent standalone
upstream commit.

## What patch 4 adds

The root module owns `crate.from_cargo` because it owns the workspace Cargo
manifest and lockfile. The ESP32 module owns the ESP-specific crate
adaptations. Patch 4 allows modules that use the same crate extension to
contribute annotation tags without declaring another `from_cargo` repository,
then merges those tags by repository name.

Annotations remain explicitly repository-scoped and duplicate crate/version
annotations still fail. Unit tests cover cross-module merging and repository
filtering.

Patch 4 is a separate commit on top of patches 2 and 3.

## What patch 5 fixes

The published `esp-phy` 0.2.0 package contains an empty file named `esp-phy`.
The generated Bazel library target has that same name. `rules_rs` otherwise
includes the package file in the build script's compile-data glob, where the
label resolves to the library target and creates a dependency cycle.

Patch 5 excludes only that colliding empty file for `esp-phy`. The build
script's actual inputs, including `esp_config.yml` and `phy_provides.x`, remain
in its compile data.

## Reproduce and validate

From a rules_rs checkout:

```sh
git switch esp32c6-riscv32imac
bazel test //rs/private:annotations_tests //rs/private:cfg_parser_tests
```

The annotation and cfg-parser tests pass against the patched v0.0.98 source.
The workspace validation matrix passes with Rust `1.96.1`, including direct
ESP32-C6 firmware compilation and transitioned flash-launcher generation.

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
