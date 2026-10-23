# Building

## Prerequisites (Windows 10/11)

1. **Visual Studio Build Tools** with the "Desktop development with C++" workload (linker, C/C++ compiler):

   ```powershell
   winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

2. **Rust** via rustup (stable, edition 2024 needs Rust 1.85 or newer):

   ```powershell
   winget install --id Rustlang.Rustup
   ```

## Build

```powershell
cargo build --release                         # with audio (default feature)
cargo build --release --no-default-features   # video only
cargo test                                    # unit tests
cargo fmt --check                             # formatting
```

Result: `target\release\airbridge.exe` (about 2 MB without audio). The first build is slower because the C code (playfair)
and, with audio, the C++ code (FDK-AAC) are compiled too.

For development with log output, run from a terminal: `cargo run --release`. When started by double click the
console is hidden and errors show up as a dialog.

## Why two variants?

`playfair` is GPL-3.0; FDK-AAC's licence is considered GPL-incompatible by the FSF. Building both together for **your own
use** is fine, but distributing that binary is not. Therefore releases are built with `--no-default-features`.
See [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md). This is not legal advice.

## Packaging

```powershell
.\scripts\package.ps1 -Version 0.1.0        # creates dist\AirBridge-0.1.0-portable.zip
```

The installer is built with [Inno Setup 6](https://jrsoftware.org/isinfo.php):

```powershell
iscc /DAppVersion=0.1.0 installer\airbridge.iss
```

CI does both when you push a tag like `v0.1.0` (see `.github/workflows/release.yml`).

## Antivirus / EDR

Cargo builds a small `build-script-build.exe` for every `build.rs`. Strict EDR products block unknown hashes. Exclude
`target\` and `%USERPROFILE%\.cargo` or release the files once. A running `airbridge.exe` also locks the output file;
close it before rebuilding.
