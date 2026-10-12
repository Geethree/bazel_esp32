# Serial transport

`serial-transport` provides shared host-side serial discovery, connection, and
reconnect behavior. Device selection is configurable; the default selects one
Espressif USB Serial/JTAG device and prefers the macOS callout path over its TTY
alias.
