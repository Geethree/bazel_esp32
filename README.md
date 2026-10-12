# ESP32

Rust firmware for the XIAO ESP32-C6 and ESP32-S31 Function CoreBoard-1,
built with Bazel.

The diagnostics app blinks the LED and reports BOOT button presses. The
provisioning app starts a Wi-Fi setup page, saves the credentials, and uses
them to reconnect after a restart.

A place to try embedded Rust and use the same apps across different boards.
See [firmware](firmware/README.md) for build and flash commands.

Licensed under [MIT](LICENSE).
