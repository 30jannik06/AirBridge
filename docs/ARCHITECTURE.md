# Architecture

AirBridge is a single Rust binary with a handful of threads.

```
iPhone ──mDNS──────▶ discovery           announces _airplay._tcp and _raop._tcp
iPhone ──TCP 7000──▶ rtsp ─▶ pairing / fairplay / stream     session setup, keys
iPhone ──TCP 7100──▶ mirror ─▶ sps ─▶ decoder ─▶ window      video
iPhone ──UDP───────▶ audio ─▶ ALAC / FDK-AAC ─▶ playback    music and mirroring sound (feature "audio")
iPhone ──RTSP SET_PARAMETER─▶ music ─▶ ui                    title, cover, progress, volume (now playing)
```

## Threads

| Thread | Job |
|---|---|
| main | eframe/egui event loop: window, drawing, input, tray icon. Must be the main thread on Windows. |
| network | `run_network()`: mDNS announcement, then waits for commands from the UI (rename). |
| rtsp | Accept loop on port 7000, one thread per connection. |
| mirror | Accepts the video TCP connection on port 7100, decrypts, forwards NAL units. |
| decoder | Receives NAL units over a bounded channel, decodes via Media Foundation, publishes the newest frame. |
| ntp | Sends NTP requests to the iPhone's timing port while a session is alive. |
| audio / playback | (feature `audio`) receives RTP, decrypts, decodes (ALAC, or AAC-ELD with `nonfree-audio`), resamples, plays via cpal. |

The decoder publishes into a single-slot `Latest` mailbox and wakes the window through the egui context (`shared::wake`).
If the window is slow, frames are dropped instead of queued: latency stays low.

## Modules

| File | Responsibility |
|---|---|
| `main.rs` | Startup, threads, command line options |
| `discovery.rs` | mDNS service records and TXT fields |
| `rtsp.rs` | RTSP/HTTP server on 7000: `/info`, `/pair-setup`, `/pair-verify`, `/fp-setup`, `SETUP`, `RECORD`, … |
| `pairing.rs` | Legacy pair-verify: X25519 + Ed25519 + AES-128-CTR |
| `identity.rs` | Persistent Ed25519 key, device id, `pi` UUID, key file location |
| `fairplay.rs` | FFI to the C code in `vendor/playfair` (`/fp-setup` and key decryption) |
| `stream.rs` | `SETUP`: key derivation, port allocation, starts NTP and audio |
| `ntp.rs` | NTP client towards the iPhone |
| `mirror.rs` | Video channel: 128-byte header, AES-CTR, AVCC → Annex B |
| `sps.rs` | Rewrites the SPS so the decoder does not buffer frames |
| `decoder.rs` | Media Foundation H.264 decoder, NV12 → RGB |
| `ui.rs` | egui interface: video view, side panel (status, settings, log), approval dialog, fullscreen |
| `tray.rs` | Tray icon and menu (tray-icon) |
| `settings.rs` | `settings.json`: name, window and system options, trusted devices |
| `shared.rs` | Global state shared by network threads and UI: log, settings, pending approval |
| `audio.rs`, `playback.rs` | (feature `audio`) audio pipeline: ALAC decoder, optional AAC-ELD, volume, sample ring for the spectrum |
| `music.rs` | "Now playing" state: DMAP metadata, cover, progress, volume from `SET_PARAMETER` |
| `spectrum.rs` | FFT and 32 logarithmic bands for the visualiser |
| `updater.rs` | Optional update check: WinHTTP GET to the GitHub API, version comparison, notice in the UI |
| `dacp.rs` | Remote control of the iPhone (play/pause/next/previous) via DACP over mDNS + HTTP |
| `platform.rs` | Windows helpers: hide console, message boxes, firewall rule, autostart |
| `icon.rs` | Window icon, drawn at runtime |
| `selftest.rs` | Offline decoder test (`--decode-file`) |

`vendor/` holds the C sources compiled by `build.rs` with the `cc` crate (playfair and the fixed FairPlay reply tables).

## Features

- `audio` (default): playback with cpal and the ALAC decoder (AirPlay music). All permissively licensed.
- `nonfree-audio` (off by default, implies `audio`): adds AAC-ELD decoding with FDK-AAC for the sound that goes with screen
  mirroring. Off by default because FDK-AAC is not GPL-compatible. Without it AirBridge still answers the iPhone's audio
  setup for mirroring (it expects ports) but cannot decode that stream.

## Key file location (`identity::key_path`)

1. `portable.txt` next to the exe → key next to the exe.
2. Otherwise `%APPDATA%\AirBridge\airbridge_key.bin`.
3. A legacy key in the current directory is copied to (2) once.
