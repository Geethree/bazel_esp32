# Serial console

`serial-console` is a byte-transparent interactive terminal for an attached
board. Run the host tool directly:

```sh
bazel run //tools/serial/console:console
```

The raw console opens the selected device at 115200 baud, forwards
newline-delimited stdin to the device, writes device bytes directly to stdout,
and exits when stdin reaches EOF (Ctrl-D). Firmware owns the command language
and output format.

The XIAO ESP32-C6 provisioning console uses the chip's USB Serial/JTAG port,
which the host tool discovers automatically. The ESP32-S31 Function CoreBoard-1
provisioning console uses UART0 on the board's USB-to-UART port. Select its
serial device explicitly:

```sh
bazel run //tools/serial/console:console -- --port /dev/cu.YOUR_S31_UART_PORT
```

Replace the example path with the port currently assigned to the board.

An explicitly HIL-enabled build can inspect generic framed traffic:

```sh
bazel run //tools/serial/console:hil -- --mode hil
```

HIL mode accepts generic `request` and `control` payloads with `\r`, `\n`,
`\t`, and `\\` escapes. It deliberately contains no Wi-Fi-specific commands.
