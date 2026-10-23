# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/), versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-10-23

### Added
- New interface (egui): status page, settings, live log, hint screen while waiting.
- Settings saved in `settings.json`: device name (re-announced live), always on top, automatic fullscreen,
  start with Windows, run in the tray when closed.
- Tray icon (left click opens, right click menu) and `--minimized` start.
- Optional confirmation before a new device may connect, with a list of trusted devices.
- Live connection stats (resolution, latency) and a built-in log view.

### Changed
- The window layer moved from winit/softbuffer to eframe/egui (executable is about 5 MB larger).
- Uninstalling also removes the autostart entry.

## [0.1.0] - 2026-10-23

First public version.

### Added
- AirPlay screen mirroring receiver: mDNS discovery, legacy pairing, FairPlay, mirror stream.
- H.264 decoding with Windows Media Foundation, low-latency SPS rewrite, window with fullscreen (F11, double click).
- AAC-ELD audio with FDK-AAC, behind the opt-in `nonfree-audio` cargo feature (off by default and in prebuilt releases).
- Unique device id and persistent key (`%APPDATA%\AirBridge`, or next to the exe in portable mode).
- Firewall helper (`--install-firewall`, `--remove-firewall`).
- Windows installer and portable ZIP.
