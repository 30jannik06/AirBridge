# Architecture

AirBridge is a single Rust binary with a handful of threads.

```
iPhone ──mDNS──────▶ discovery           announces _airplay._tcp and _raop._tcp
iPhone ──TCP 7000──▶ rtsp ─▶ pairing / fairplay / stream     session setup, keys
iPhone ──TCP 7100──▶ mirror ─▶ sps ─▶ decoder ─▶ window      video
iPhone ──UDP───────▶ audio ─▶ FDK-AAC ─▶ playback            audio (feature "audio")
```

## Threads

| Thread | Job |
|---|---|
| main | winit event loop: window, drawing, input. Must be the main thread on Windows. |
| network | `run_network()`: mDNS announcement, then blocks in the RTSP accept loop (one thread per connection). |
| mirror | Accepts the video TCP connection on port 7100, decrypts, forwards NAL units. |
| decoder | Receives NAL units over a bounded channel, decodes via Media Foundation, publishes the newest frame. |
| ntp | Sends NTP requests to the iPhone's timing port while a session is alive. |
| audio / playback | (feature `audio`) receives RTP, decrypts, decodes, resamples, plays via cpal. |

The decoder publishes into a single-slot `Latest` mailbox and wakes the window with an event-loop proxy.
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
| `window.rs` | winit window, softbuffer drawing, bilinear scaling, fullscreen |
| `audio.rs`, `playback.rs` | (feature `audio`) audio pipeline |
| `platform.rs` | Windows helpers: hide console, message boxes, firewall rule |
| `icon.rs` | Window icon, drawn at runtime |
| `selftest.rs` | Offline decoder test (`--decode-file`) |

`vendor/` holds the C sources compiled by `build.rs` with the `cc` crate (playfair and the fixed FairPlay reply tables).

## Features

- `audio` (default): AAC-ELD decoding with FDK-AAC and playback with cpal.
  Without it AirBridge still answers the iPhone's audio setup (it expects ports) but drops the packets.

## Key file location (`identity::key_path`)

1. `portable.txt` next to the exe → key next to the exe.
2. Otherwise `%APPDATA%\AirBridge\airbridge_key.bin`.
3. A legacy key in the current directory is copied to (2) once.
