# AirBridge

[![CI](https://github.com/30jannik06/AirBridge/actions/workflows/ci.yml/badge.svg)](https://github.com/30jannik06/AirBridge/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

**A lean, native AirPlay screen-mirroring receiver for Windows, written in Rust.**
Mirror your iPhone from Control Center → Screen Mirroring to a Windows PC. No app on the iPhone, no Apple software on the PC.

🇩🇪 [Deutsche Version](README.de.md)

- **One small executable** (about 2 MB), no runtime dependencies.
- **Low latency video:** H.264 decoded by the decoder built into Windows (Media Foundation), plus a window with fullscreen.
- **Audio** (optional, build-time feature): AAC-ELD via FDK-AAC, played on your default speakers.
- **Installer or portable:** both are provided on the releases page.

## Status

Tested with an iPhone on Windows 11. Working: discovery, pairing, video, audio (self-built), rotation, fullscreen.

Known limitations:

- Windows only, one iPhone at a time.
- No DRM content (Netflix, Disney+, …): iOS itself blocks that while mirroring.
- Audio and video are not precisely synchronised.
- No volume sync with the iPhone; audio follows the Windows volume.
- Not tested: iPad, Mac, several simultaneous connections, very old iOS versions.

## Install

Download from the [Releases](https://github.com/30jannik06/AirBridge/releases) page:

| Variant | For whom | How |
|---|---|---|
| **Setup** (`AirBridge-Setup-x.y.z.exe`) | Normal use | Run it: installs to Program Files, adds a Start menu entry and the firewall rule. |
| **Portable** (`AirBridge-x.y.z-portable.zip`) | USB stick, no admin rights | Unzip anywhere, run `airbridge.exe`. Settings stay next to the exe (marked by `portable.txt`). |

> **About audio in the downloads.** The licence of the AAC codec (FDK-AAC) is not compatible with the GPL, so the
> prebuilt binaries are built **without audio** (video only). To get sound, build it yourself with one command, see
> [docs/BUILDING.md](docs/BUILDING.md). Details: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Usage

1. PC and iPhone must be on the **same network** (no guest Wi-Fi with client isolation, no VPN in between).
2. Start AirBridge. A window "AirBridge – waiting for iPhone …" opens.
3. On the iPhone: **Control Center → Screen Mirroring → AirBridge**.

| Input | Effect |
|---|---|
| `F11` or double click | Toggle fullscreen |
| `Esc` | Leave fullscreen |
| Close window | Quit |

The window follows the picture size (e.g. when you rotate the phone).

### Firewall rule

The installer creates it for you. For the portable version (or after moving the exe):

```powershell
.\airbridge.exe --install-firewall   # asks for administrator rights (UAC)
.\airbridge.exe --remove-firewall
```

The rule allows **inbound** connections for AirBridge on private and domain networks only.

### Ports

| Port | Protocol | Purpose |
|---|---|---|
| 5353 | UDP | mDNS / Bonjour: makes AirBridge visible |
| 7000 | TCP | RTSP/HTTP: pairing, FairPlay, session setup |
| 7100 | TCP | Video stream |
| dynamic | UDP | Time sync (NTP) and audio |

## Build from source

```powershell
cargo build --release                         # with audio (for your own use)
cargo build --release --no-default-features   # video only (what the releases use)
```

Prerequisites and EDR/antivirus hints: [docs/BUILDING.md](docs/BUILDING.md).

## Documentation

- [Architecture](docs/ARCHITECTURE.md): modules and data flow
- [Protocol notes](docs/PROTOCOL.md): how the AirPlay mirroring handshake works (the hard-won knowledge)
- [Building](docs/BUILDING.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Roadmap](docs/ROADMAP.md) (including Linux)
- [Contributing](CONTRIBUTING.md), [Security policy](SECURITY.md), [Changelog](CHANGELOG.md)

## Security and privacy

- AirBridge listens on the local network and has **no PIN or password**: any device on the same network can connect. Use it on networks you trust.
- It never contacts the internet.
- The receiver's private key (`airbridge_key.bin`) lives in `%APPDATA%\AirBridge` (installed) or next to the exe (portable). Do not share it.

## Licence

GPL-3.0-only, see [LICENSE](LICENSE). It includes the GPL-licensed `playfair` code from UxPlay, so the whole program is GPL.
Third-party components: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

AirPlay, FairPlay and iPhone are trademarks of Apple Inc. AirBridge is an independent project and not affiliated with Apple.

## Acknowledgements

Thanks to the open-source AirPlay community: **UxPlay**, **shairplay** and everyone who documented the protocol.
