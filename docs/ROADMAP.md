# Roadmap

Ideas, roughly by value. Nothing here is promised.

- **Audio/video sync** using the RTP timestamps and the NTP clock.
- **AirPlay video by link** (YouTube and similar casts from the iPhone) and photo sharing.
- **Seek bar:** click the progress bar to jump (needs DACP).
- **Executable icon** (embedded resource) instead of only the window icon.
- **Better scaling / GPU rendering** (today: software bilinear scaling).
- **Multiple devices / reconnect polish.**
- **Real PIN pairing** (the current "allow this device?" prompt is a confirmation, not a PIN).
- **Linux support.** The protocol layer (mDNS, RTSP, pairing, FairPlay, mirror channel) is portable Rust and C. What is
  Windows specific: the H.264 decoder (Media Foundation), a few platform helpers, and the installer. A Linux port needs
  a different decoder (e.g. GStreamer or FFmpeg) and packaging. Note that [UxPlay](https://github.com/FDH2/UxPlay)
  already does this very well on Linux, so this is low priority.
- **macOS: not planned.** macOS has an AirPlay receiver built in (Monterey and later).
