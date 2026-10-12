# HIL testing

`hil-testing` is the reusable hardware-in-the-loop protocol framework. Its
default build is `no_std` and provides the versioned `HIL1` frame codec. The
non-default `host` feature adds streaming decode and reconnectable serial test
sessions through `serial-transport`.

Frame kinds are deliberately application-neutral. Concrete tests define the
meaning of request, response, event, and control payloads.
