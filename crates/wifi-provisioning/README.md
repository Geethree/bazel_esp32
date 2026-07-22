# Wi-Fi provisioning

`wifi-provisioning` is the transport-independent, `no_std` provisioning core.
Production firmware serves its bounded HTTP service over the captive TCP
portal. The non-default `hil` feature adds provisioning-specific test controls
on top of the generic `hil-testing` protocol; production firmware has a
compile-time guard against enabling that feature.

Run the host unit suite without hardware:

```sh
bazel test //crates/wifi-provisioning:unit_tests
```

Preview the exact minified portal served by the firmware, backed by local mock
network and connection endpoints:

```sh
bazel run //crates/wifi-provisioning/portal:preview
```

Open `http://127.0.0.1:4173/`. The readable HTML, CSS, and JavaScript sources
live under `portal/`; Bazel minifies and inlines them into a generated `no_std`
Rust asset. Production streams the page from flash instead of copying the full
document into the bounded dynamic-response buffer.

Build production firmware and the provisioning HIL test without flashing:

```sh
bazel build //firmware:flash //firmware/hil/provisioning:test
```

The hardware test is intentionally explicit:

```sh
export HIL_WIFI_SSID='your 2.4 GHz test SSID'
export HIL_WIFI_PASSWORD='your test password'
bazel test //firmware/hil/provisioning:test --test_output=streamed
```

The target inherits those two variables without placing their values on
Bazel's command line. It is tagged `manual`, `local`, and `exclusive`. Its host
harness discovers the single attached Espressif USB Serial/JTAG device and
never prints the password. The test erases only the `wifi_cfg` region, flashes
the HIL firmware from runfiles, exercises raw HTTP over serial, verifies
association and DHCP, resets to check persisted reconnection, and clears the
test credentials. The tokenized LAN health assertion remains available but
disabled while the test network isolates Wi-Fi clients.

Run every registered physical test explicitly with:

```sh
bazel test //firmware/hil:tests --test_output=streamed
```
