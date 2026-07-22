# Serial console

`serial-console` is a byte-transparent interactive terminal for an attached
Espressif USB Serial/JTAG device. Run it through the stable firmware alias:

```sh
bazel run //firmware:console
```

The raw console opens the selected device at 115200 baud, forwards
newline-delimited stdin to the device, writes device bytes directly to stdout,
and exits when stdin reaches EOF (Ctrl-D). Firmware owns the command language
and output format.

An explicitly HIL-enabled build can inspect generic framed traffic:

```sh
bazel run //firmware:hil_console -- --mode hil
```

HIL mode accepts generic `request` and `control` payloads with `\r`, `\n`,
`\t`, and `\\` escapes. It deliberately contains no Wi-Fi-specific commands.
