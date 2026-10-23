# Drittanbieter-Komponenten und Lizenzen

AirBridge enthält oder verwendet die folgenden Komponenten. Die Lizenzlage ist für eine **Weitergabe der fertigen
`.exe`** wichtig, siehe Abschnitt „Wichtiger Lizenzkonflikt“.

## In den Quelltext eingebunden (`vendor/`)

| Komponente | Herkunft | Lizenz | Verwendung |
|---|---|---|---|
| **playfair** (`vendor/playfair/`) | aus [UxPlay](https://github.com/FDH2/UxPlay), `lib/playfair` | **GPL-3.0** (Text: [`LICENSE`](LICENSE)) | FairPlay-Entschlüsselung des Stream-Schlüssels (`fairplay.rs`) |
| **FairPlay-Antworttabellen** (`vendor/fp_tables.c`) | unverändert extrahiert aus UxPlay `lib/fairplay_playfair.c` (Copyright 2018 Juho Vähä-Herttua, ursprünglich shairplay) | **LGPL-2.1-or-later** | Feste Antworten für `POST /fp-setup` |

## Als Abhängigkeit mitkompiliert

| Komponente | Lizenz | Verwendung |
|---|---|---|
| **FDK-AAC** (über Crate `fdk-aac-sys` 0.5.0, Quellcode von Fraunhofer) | **Fraunhofer FDK AAC Codec Library License** (Volltext: [`licenses/FDK-AAC-NOTICE.txt`](licenses/FDK-AAC-NOTICE.txt)) | Dekodierung des iPhone-Tons (AAC-ELD) |

Der Rust-Wrapper `fdk-aac` selbst steht unter MIT, der Codec dahinter unter der Fraunhofer-Lizenz.

## Rust-Crates (direkte Abhängigkeiten)

| Crate | Version | Lizenz |
|---|---|---|
| aes | 0.9.3 | MIT OR Apache-2.0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| cbc | 0.2.1 | MIT OR Apache-2.0 |
| cc (nur Build) | 1.9.0 | MIT OR Apache-2.0 |
| cpal | 0.18.2 | Apache-2.0 |
| ctr | 0.10.1 | MIT OR Apache-2.0 |
| ed25519-dalek | 3.0.0 | BSD-3-Clause |
| fdk-aac | 0.8.0 | MIT |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| mdns-sd | 0.21.6 | Apache-2.0 OR MIT |
| plist | 1.10.3 | MIT |
| sha2 | 0.11.1 | MIT OR Apache-2.0 |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 |
| windows | 0.62.2 | MIT OR Apache-2.0 |
| winit | 0.30.13 | Apache-2.0 |
| x25519-dalek | 3.0.0 | BSD-3-Clause |

Dazu kommen die transitiven Abhängigkeiten dieser Crates (fast ausschließlich MIT und/oder Apache-2.0). Eine vollständige
Liste mit Lizenzen erzeugt:

```powershell
cargo metadata --format-version 1
```

(oder bequemer mit dem Werkzeug `cargo-about` bzw. `cargo-license`, falls installiert).

## Protokoll-Vorbilder

Das Verhalten des AirPlay-Protokolls (Pairing, FairPlay-Handshake, Schlüsselableitung, Paketformate) wurde anhand des
Quellcodes von **UxPlay** (GPL-3.0) und der Dokumentation der Open-Source-AirPlay-Community nachvollzogen und in Rust neu
geschrieben. AirBridge ist daher als abgeleitetes Werk im Sinne der GPL zu behandeln.

## Wichtiger Lizenzkonflikt bei Weitergabe der `.exe`

Zwei der eingebundenen Komponenten sind **nicht miteinander vereinbar**, sobald man das Programm **als fertige Datei
weitergibt**:

1. **playfair steht unter GPL-3.0.** Wer das Programm weitergibt, muss es als Ganzes unter die GPL stellen und den
   vollständigen Quellcode anbieten.
2. **Die FDK-AAC-Lizenz** verlangt u. a., dass der vollständige Lizenztext erhalten bleibt und der Quellcode kostenlos
   verfügbar ist. Sie schließt Patentlizenzen ausdrücklich aus („NO PATENT LICENSE“): Das Programm darf nur für Zwecke
   genutzt werden, für die entsprechende Patentlizenzen vorliegen. Die Free Software Foundation stuft diese Lizenz als
   **mit der GPL unvereinbar** ein (deshalb dürfen z. B. ffmpeg-Binaries mit FDK-AAC nicht weitergegeben werden).

Praktische Folgen:

- **Private Nutzung auf eigenen Geräten** ist davon nicht betroffen.
- **Weitergabe der fertigen `.exe`** an andere kann gegen die Lizenzbedingungen verstoßen. Das ist keine Rechtsberatung.
- **Lösung in diesem Projekt:** FDK-AAC steckt hinter dem Cargo-Feature `audio`. Die veröffentlichten Programme
  (Setup und portable ZIP) werden mit `--no-default-features` gebaut, enthalten also **kein FDK-AAC** und keinen Ton.
  Wer Ton will, baut selbst (`cargo build --release`, siehe [docs/BUILDING.md](docs/BUILDING.md)) und gibt dieses
  Programm nicht weiter.
- Weitere Wege, wenn andere AirBridge mit Ton nutzen sollen:
  - **Nur den Quellcode weitergeben** (mit `LICENSE` und diesen Hinweisen). Jeder baut sich die `.exe` selbst.
  - Bei kommerzieller Nutzung oder größerer Verbreitung: Lizenz- und Patentlage (AAC-ELD, Apple FairPlay) **von einem
    Rechtsanwalt prüfen lassen**.

## Marken und Abgrenzung

AirPlay, FairPlay, iPhone, Apple TV und iOS sind Marken der Apple Inc. AirBridge ist **kein** Apple-Produkt und steht in
keiner Verbindung zu Apple. Der Receiver meldet sich im Netzwerk als `AppleTV3,2`, damit iOS ihn als Spiegelungsziel
akzeptiert.
