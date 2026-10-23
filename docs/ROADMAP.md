# Roadmap

Ideas, roughly by value. Nothing here is promised.

- **Audio/video sync** using the RTP timestamps and the NTP clock.
- **Volume sync** with the iPhone (`SET_PARAMETER volume`).
- **Executable icon** (embedded resource) instead of only the window icon.
- **Better scaling / GPU rendering** (today: software bilinear scaling).
- **Multiple devices / reconnect polish.**
- **Optional PIN** to restrict who may connect.
- **Linux support.** The protocol layer (mDNS, RTSP, pairing, FairPlay, mirror channel) is portable Rust and C. What is
  Windows specific: the H.264 decoder (Media Foundation), a few platform helpers, and the installer. A Linux port needs
  a different decoder (e.g. GStreamer or FFmpeg) and packaging. Note that [UxPlay](https://github.com/FDH2/UxPlay)
  already does this very well on Linux, so this is low priority.
- **macOS: not planned.** macOS has an AirPlay receiver built in (Monterey and later).
