# Hardware-in-the-loop tests

`hil_test` runs a caller-provided host executable locally while allowing its
host tools and device firmware to build for independent platforms. The rule is
deliberately unaware of chips, flash tools, partitions, transports, and test
protocols.

Named artifacts are supplied through `host_tools` or `data` and referenced in
`args` with `{name}` placeholders. The transitioned firmware is always
available as `{firmware}`. Every artifact label must produce exactly one file.
The generic Rust launcher resolves these artifacts with Bazel's runfiles
library before invoking the caller-provided host executable.
Callers may provide fixed `env` values or declare host variables through
`env_inherit`; the rule forwards both without knowing their meaning.

The `hil_test` macro adds `local`, `hil`, and an `esp32_slots` resource tag.
Concrete suites add `manual` so they run only when explicitly requested.
Compilation may use RBE, but test execution never runs remotely.

Concrete tests live with their firmware under `//firmware/hil/...`; this package
contains no device, transport, or application-specific test implementation.
