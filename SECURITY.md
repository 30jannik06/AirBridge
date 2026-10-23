# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems. Use GitHub's
[private vulnerability reporting](https://github.com/30jannik06/AirBridge/security/advisories/new) for this repository.
Include steps to reproduce and the affected version. You can expect a first reply within about a week.

## Scope and known design limits

- AirBridge accepts connections from **any device on the local network**; there is no PIN or password. This is by design
  (like many AirPlay receivers). Run it only on networks you trust.
- The private key file `airbridge_key.bin` identifies the receiver. Anyone with it can impersonate your receiver.
- The network parsers (RTSP, plist, video/audio framing) handle untrusted input. Reports of crashes, hangs or memory
  safety problems (`unsafe` code, the C code in `vendor/`) are especially welcome.
- AirBridge does not connect to the internet.
