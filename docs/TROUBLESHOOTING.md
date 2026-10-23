# Troubleshooting

Start AirBridge from a terminal (`.\airbridge.exe`) to see the log; double-click start hides the console.

| Problem | Cause / fix |
|---|---|
| AirBridge does not show up on the iPhone | Same network? Firewall rule (`--install-firewall`)? VPN off? Guest Wi-Fi with client isolation? Virtual adapters (WSL, Hyper-V) can confuse mDNS. |
| "Connection could not be established" on the iPhone | Run from a terminal and read the last log lines; they show which step failed. Please include the log in a bug report. |
| Error dialog "Netzwerkfehler" at startup | Another AirBridge is running, or another program uses port 7000/7100. |
| Picture but no sound | Releases are built without audio. Build with default features yourself. Otherwise check the Windows output device and look for `[Audio]` in the log. |
| Picture slightly delayed | The log prints `[Decoder] Latenz …` lines. A few milliseconds is normal; the rest is iPhone/Wi-Fi. |
| Pairing again after update | The key lives in `%APPDATA%\AirBridge` (installed) or next to the exe with `portable.txt`. Deleting `airbridge_key.bin` creates a new identity. |
| Build fails with "Zugriff verweigert" / access denied | EDR blocks `build-script-build.exe`, or a running `airbridge.exe` locks the output. See [BUILDING.md](BUILDING.md). |

Netflix, Disney+ and similar apps show a black screen: that is DRM and cannot be mirrored.
