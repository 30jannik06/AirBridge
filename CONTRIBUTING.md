# Contributing

Thanks for your interest! AirBridge is a small project; bug reports, protocol findings and patches are welcome.

## Reporting bugs

Use the bug report template. Please include: Windows version, iPhone model and iOS version, how you started AirBridge
(installed/portable/built), and the **log** from a terminal start (`.\airbridge.exe`). Remove anything private first.

## Development

```powershell
cargo fmt
cargo test
cargo build --release                           # default (video + music)
cargo build --release --no-default-features     # no audio
cargo build --release --features nonfree-audio  # plus FDK-AAC
```

Please make sure all feature variants compile without warnings (default, `--no-default-features`, `nonfree-audio`). Do not make `nonfree-audio` a default feature.

Guidelines:

- Keep it small and readable; the project is also a learning project, comments that explain *why* are welcome.
- Code comments are in English. The interface and log messages are in German for now.
- Add a unit test when you touch parsing, crypto or protocol code (see the existing tests in `pairing.rs`, `sps.rs`, `mirror.rs`).
- Do not commit key files, captured video or screenshots of real devices.

## Licensing

By contributing you agree that your contribution is licensed under **GPL-3.0-only**, like the rest of the project.
Do not copy code from sources with incompatible licences. Do not add FDK-AAC (or other non-GPL-compatible code) to the
default build in a way that makes the released binaries non-distributable.

## Pull requests

Small, focused PRs are easiest to review. Describe what changed and how you tested it (ideally with a real iPhone).
By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
