# Wi-Fi provisioning

`wifi-provisioning` is the transport-independent, `no_std` provisioning core.
Production firmware serves its bounded HTTP service over the captive TCP
portal. The non-default `hil` feature adds provisioning-specific test controls
on top of the generic `hil-testing` protocol; production firmware has a
compile-time guard against enabling that feature.

While the portal is active, the radio uses access-point and station mode so
the station can rescan without stopping the portal. A manual serial `rescan`
reports completion or failure and the raw/listed access-point counts. The
displayed list keeps networks from successful scans for up to two minutes;
one empty scan does not immediately erase earlier sightings.

The manual `rescan` command temporarily uses station-only mode for three scan
passes with a 50–120 ms active dwell per channel, then restores the setup
access point. Clients connected to it may need to reconnect after a manual
rescan. Periodic scans stay
in access-point and station mode so they do not repeatedly interrupt the portal.

Run the host unit suite without hardware:

```sh
bazel test //firmware/services/provisioning:unit_tests
```

Preview the exact minified portal served by the firmware, backed by local mock
network and connection endpoints:

```sh
bazel run //firmware/services/provisioning/portal:preview
```

Open `http://127.0.0.1:4173/`. The readable HTML, CSS, and JavaScript sources
live under `portal/`; Bazel minifies and inlines them into a generated `no_std`
Rust asset. Production streams the page from flash instead of copying the full
document into the bounded dynamic-response buffer.

Build production firmware and the provisioning HIL test without flashing:

```sh
bazel build --platforms=//bazel/platforms:xiao_esp32c6 //firmware/apps/provisioning:flash
bazel build //firmware/hil/provisioning:test_xiao_esp32c6
bazel build --platforms=//bazel/platforms:s31_function_coreboard_1 //firmware/apps/provisioning:flash
bazel build //firmware/hil/provisioning:test_s31_function_coreboard_1
```

The hardware test is intentionally explicit:

```sh
export HIL_WIFI_SSID='your 2.4 GHz test SSID'
export HIL_WIFI_PASSWORD='your test password'
bazel test //firmware/hil/provisioning:test_xiao_esp32c6 --test_output=streamed
```

The target inherits those two variables without placing their values on
Bazel's command line. It is tagged `manual` and `local`, and reserves the
`esp32_slots` test resource. Its host
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

The S31 provisioning image and HIL launcher compile. Physical S31 Wi-Fi,
portal, and credential-persistence behavior has not been verified by these
build commands. The current HIL device uses its own scan loop and USB
Serial/JTAG transport. Its host test records whether the requested SSID was
observed but still attempts a direct connection when it was not. It does not
exercise the production app's serial `rescan` path or its two-minute scan
cache. The physical test erases `wifi_cfg` and flashes a separate HIL image.
