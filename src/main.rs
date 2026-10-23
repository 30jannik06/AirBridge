// AirBridge: AirPlay-Mirroring-Receiver für Windows.
// Phase 1: mDNS-Discovery   Phase 2: RTSP-Server + Pairing   Phase 3: FairPlay, SETUP, Mirror-Kanal
// Phase 4: H.264-Decoder (Media Foundation) + Live-Fenster
#[cfg(feature = "nonfree-audio")]
mod audio;
mod config;
mod decoder;
mod discovery;
mod fairplay;
mod icon;
mod identity;
mod mirror;
mod ntp;
mod pairing;
mod platform;
#[cfg(feature = "nonfree-audio")]
mod playback;
mod rtsp;
mod selftest;
mod sps;
mod stream;
mod window;

use anyhow::Result; // `Result<T>` mit beliebigem Fehler; `?` reicht Fehler nach oben weiter
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

fn main() -> Result<()> {
    // Bei Doppelklick-Start gibt es keine Konsole mehr; bei Start aus dem Terminal bleibt sie (für das Log).
    platform::hide_console_if_alone();
    // Ohne Konsole würde ein Absturz unsichtbar bleiben: darum zeigen wir ihn in einem Dialog.
    std::panic::set_hook(Box::new(|info| {
        platform::message("AirBridge – Absturz", &info.to_string(), true)
    }));

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Test-Modus: airbridge --decode-file <datei.h264> <frame-nr> <out.ppm>
        Some("--decode-file") if args.len() == 5 => {
            return selftest::decode_file(&args[2], args[3].parse()?, &args[4]);
        }
        // Test-Modus: airbridge --save-icon <out.ppm>  (zeigt das gezeichnete Icon als Bild)
        Some("--save-icon") if args.len() == 3 => {
            let rgba = icon::rgba(256);
            let mut ppm = b"P6\n256 256\n255\n".to_vec();
            for px in rgba.chunks(4) {
                let a = px[3] as u32; // auf hellgrauen Hintergrund mischen, damit man die Transparenz sieht
                ppm.extend(
                    px[..3]
                        .iter()
                        .map(|&c| ((c as u32 * a + 200 * (255 - a)) / 255) as u8),
                );
            }
            std::fs::write(&args[2], ppm)?;
            return Ok(());
        }
        // Firewall-Regel anlegen/entfernen (fragt per UAC nach Administratorrechten)
        Some(flag @ ("--install-firewall" | "--remove-firewall")) => {
            if let Err(e) = platform::firewall(flag == "--install-firewall") {
                platform::message("AirBridge – Firewall", &format!("{e:#}"), true);
            }
            return Ok(());
        }
        _ => {}
    }

    // Das Fenster MUSS im Haupt-Thread laufen. Netzwerk und Decoder laufen in eigenen Threads.
    let (event_loop, proxy) = window::create()?;
    let latest: decoder::Latest = Arc::new(Mutex::new(None));

    // Kanal Netzwerk -> Decoder: bis zu 120 Bilder dürfen warten.
    let (video_tx, video_rx) = sync_channel::<(Instant, Vec<u8>)>(120);
    decoder::spawn(video_rx, Arc::clone(&latest), move || {
        let _ = proxy.send_event(()); // weckt das Fenster auf
    });

    thread::spawn(move || {
        if let Err(e) = run_network(video_tx) {
            eprintln!("Netzwerk-Fehler: {e:#}");
            platform::message(
                "AirBridge – Netzwerkfehler",
                &format!(
                    "{e:#}\n\nLäuft AirBridge vielleicht schon, oder belegt ein anderes Programm Port 7000/7100?"
                ),
                true,
            );
            std::process::exit(1);
        }
    });

    window::run(event_loop, latest)
}

fn run_network(video: SyncSender<(Instant, Vec<u8>)>) -> Result<()> {
    let identity = Arc::new(identity::Identity::load_or_create(&identity::key_path())?);

    // Muss in einer Variable leben bleiben: wird sie gedroppt (Ownership!), endet die Ankündigung.
    let _daemon = discovery::announce(&identity)?;
    println!(
        "AirBridge sichtbar im Netzwerk (Geräte-ID {}).",
        identity.device_id()
    );

    // Briefkasten: der RTSP-Thread legt hier die Video-Schlüssel ab, der Mirror-Thread holt sie.
    let slot: mirror::Slot = Arc::new(Mutex::new(None));
    mirror::spawn(Arc::clone(&slot), video)?;

    rtsp::serve(identity, slot) // blockiert für immer
}
