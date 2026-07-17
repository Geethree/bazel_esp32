"""Public build and device rules for ESP32 firmware."""

load("//private:flash.bzl", _esp32_flash = "esp32_flash")

esp32_flash = _esp32_flash
