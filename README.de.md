# AirBridge

[![CI](https://github.com/30jannik06/AirBridge/actions/workflows/ci.yml/badge.svg)](https://github.com/30jannik06/AirBridge/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

**Ein schlanker, nativer AirPlay-Screen-Mirroring-Receiver für Windows, geschrieben in Rust.**
Du spiegelst dein iPhone über Kontrollzentrum → Bildschirmsynchronisierung auf den PC. Keine App auf dem iPhone, keine Apple-Software auf dem PC.

🇬🇧 [English version](README.md)

- **Eine kleine Datei** (ca. 2 MB), keine Laufzeit-Abhängigkeiten.
- **Wenig Verzögerung:** H.264 über den in Windows eingebauten Decoder (Media Foundation), Fenster mit Vollbild.
- **Ton** (optional, beim Bauen wählbar): AAC-ELD über FDK-AAC, Ausgabe auf den Standard-Lautsprechern.
- **Setup oder portabel:** beides gibt es auf der Releases-Seite.

## Status

Getestet mit einem iPhone unter Windows 11. Funktioniert: Erkennung, Pairing, Bild, Ton (selbst gebaut), Drehen, Vollbild.

Bekannte Einschränkungen:

- Nur Windows, nur ein iPhone gleichzeitig.
- Kein DRM-Inhalt (Netflix, Disney+, …): das blockiert iOS selbst beim Spiegeln.
- Ton und Bild sind nicht exakt synchronisiert.
- Kein Lautstärke-Abgleich mit dem iPhone; der Ton folgt der Windows-Lautstärke.
- Nicht geprüft: iPad, Mac, mehrere gleichzeitige Verbindungen, sehr alte iOS-Versionen.

## Installation

Download auf der [Releases](https://github.com/30jannik06/AirBridge/releases)-Seite:

| Variante | Für wen | So geht's |
|---|---|---|
| **Setup** (`AirBridge-Setup-x.y.z.exe`) | Normale Nutzung | Ausführen: installiert nach Programme, legt Startmenü-Eintrag und Firewall-Regel an. |
| **Portabel** (`AirBridge-x.y.z-portable.zip`) | USB-Stick, keine Adminrechte | Irgendwohin entpacken, `airbridge.exe` starten. Einstellungen bleiben neben der Exe (`portable.txt`). |

> **Zum Ton in den Downloads.** Die Lizenz des AAC-Codecs (FDK-AAC) ist nicht mit der GPL vereinbar. Darum sind die
> fertigen Programme **ohne Ton** gebaut (nur Bild). Für Ton baust du selbst mit `--features nonfree-audio`, siehe
> [docs/BUILDING.md](docs/BUILDING.md). Details: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Benutzung

1. PC und iPhone müssen im **selben Netzwerk** sein (kein Gast-WLAN mit Geräteisolation, kein VPN dazwischen).
2. AirBridge starten. Es öffnet sich ein Fenster „AirBridge – warte auf iPhone …“.
3. Auf dem iPhone: **Kontrollzentrum → Bildschirmsynchronisierung → AirBridge**.

| Eingabe | Wirkung |
|---|---|
| `F11` oder Doppelklick | Vollbild an/aus |
| `Esc` | Vollbild verlassen |
| Fenster schließen | Beenden |

### Firewall-Regel

Das Setup legt sie an. Bei der portablen Version (oder nach dem Verschieben der Exe):

```powershell
.\airbridge.exe --install-firewall   # fragt per UAC nach Administratorrechten
.\airbridge.exe --remove-firewall
```

Die Regel erlaubt **eingehende** Verbindungen nur in privaten und Domänen-Netzwerken.

### Ports

| Port | Protokoll | Zweck |
|---|---|---|
| 5353 | UDP | mDNS/Bonjour: macht AirBridge sichtbar |
| 7000 | TCP | RTSP/HTTP: Pairing, FairPlay, Verbindungsaufbau |
| 7100 | TCP | Video-Datenstrom |
| dynamisch | UDP | Zeitabgleich (NTP) und Ton |

## Selbst bauen

```powershell
cargo build --release                           # nur Bild (so bauen die Releases)
cargo build --release --features nonfree-audio  # mit Ton, nur für den eigenen Gebrauch (nicht weitergeben)
```

Voraussetzungen und Hinweise zu Virenscanner/EDR: [docs/BUILDING.md](docs/BUILDING.md).

## Dokumentation

- [Architektur](docs/ARCHITECTURE.md): Module und Datenfluss
- [Protokoll-Notizen](docs/PROTOCOL.md): wie der AirPlay-Mirroring-Handshake funktioniert
- [Bauen](docs/BUILDING.md)
- [Fehlersuche](docs/TROUBLESHOOTING.md)
- [Roadmap](docs/ROADMAP.md) (u. a. Linux)
- [Mitmachen](CONTRIBUTING.md), [Sicherheit](SECURITY.md), [Änderungen](CHANGELOG.md)

## Sicherheit und Datenschutz

- AirBridge hört im lokalen Netzwerk und hat **keine PIN/kein Passwort**: Jedes Gerät im selben Netzwerk kann sich verbinden. Nur in vertrauenswürdigen Netzwerken betreiben.
- Das Programm sendet nichts ins Internet.
- Der private Schlüssel (`airbridge_key.bin`) liegt in `%APPDATA%\AirBridge` (installiert) bzw. neben der Exe (portabel). Nicht weitergeben.

## Lizenz

GPL-3.0-only, siehe [LICENSE](LICENSE). Enthalten ist der GPL-Code `playfair` aus UxPlay, darum steht das ganze Programm unter der GPL.
Drittanbieter: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

AirPlay, FairPlay und iPhone sind Marken von Apple Inc. AirBridge ist ein unabhängiges Projekt ohne Verbindung zu Apple.

## Danksagung

Dank an die Open-Source-AirPlay-Community: **UxPlay**, **shairplay** und alle, die das Protokoll dokumentiert haben.
