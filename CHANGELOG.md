# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/), versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-23

First public version.

### Added
- AirPlay screen mirroring receiver: mDNS discovery, legacy pairing, FairPlay, mirror stream.
- H.264 decoding with Windows Media Foundation, low-latency SPS rewrite, window with fullscreen (F11, double click).
- AAC-ELD audio with FDK-AAC, behind the `audio` cargo feature (on by default, off in prebuilt releases).
- Unique device id and persistent key (`%APPDATA%\AirBridge`, or next to the exe in portable mode).
- Firewall helper (`--install-firewall`, `--remove-firewall`).
- Windows installer and portable ZIP.
