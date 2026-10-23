# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/), versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.3.1] - 2026-10-23

### Added
- The executable now has an icon and version information (Explorer, taskbar, file properties); the installer uses it too.
- Screenshots in the README.
- Requests AirBridge does not know yet (for example AirPlay video by link) are written to the log with their content.

### Changed
- All code comments are in English.

## [0.3.0] - 2026-10-23

### Added
- **Music:** AirPlay audio (ALAC) is played, no extra codec licence needed, so the downloads now have sound for music.
- **"Now playing" view:** cover, title, artist, album, progress, volume and a live spectrum.
- **Remote control** (play/pause, next, previous) through DACP, where the iPhone allows it.
- **iPhone volume** buttons now control AirBridge's volume.
- **"Trusted devices only"** mode: unknown devices are rejected silently.
- The status tab shows music connections and the current track.

### Changed
- Cargo features: `audio` (default, ALAC and playback) and `nonfree-audio` (adds FDK-AAC for mirroring sound).
  Releases are built with `audio` only.

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
