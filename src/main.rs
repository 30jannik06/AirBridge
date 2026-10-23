// AirBridge: AirPlay mirroring receiver for Windows.
// Phase 1: mDNS discovery   Phase 2: RTSP server + pairing   Phase 3: FairPlay, SETUP, mirror channel
// Phase 4: H.264 decoder (Media Foundation)   Phase 5: interface (egui), settings, tray

// Writes to the console AND to the log view of the interface. Must come before the `mod` lines.
macro_rules! log {
    ($($arg:tt)*) => { $crate::shared::log(format!($($arg)*)) };
}

#[cfg(feature = "audio")]
mod audio;
mod config;
mod dacp;
mod decoder;
mod discovery;
mod fairplay;
mod icon;
mod identity;
mod mirror;
mod music;
mod ntp;
mod pairing;
mod platform;
#[cfg(feature = "audio")]
mod playback;
mod rtsp;
mod selftest;
mod settings;
mod shared;
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
// without audio there is no spectrum, only the constant BANDS is needed
mod spectrum;
mod sps;
mod stream;
mod tray;
mod ui;

use anyhow::Result; // `Result<T>` with any error; `?` passes errors upwards
use shared::NetCmd;
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

fn main() -> Result<()> {
    // Started by double click there is no console any more; started from a terminal it stays (for the log).
    platform::hide_console_if_alone();
    // Without a console a crash would be invisible, so we show it in a dialog.
    std::panic::set_hook(Box::new(|info| {
        platform::message("AirBridge – Absturz", &info.to_string(), true)
    }));

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Test mode: airbridge --decode-file <file.h264> <frame-no> <out.ppm>
        Some("--decode-file") if args.len() == 5 => {
            return selftest::decode_file(&args[2], args[3].parse()?, &args[4]);
        }
        // Test mode: airbridge --save-icon <out.ppm>  (shows the drawn icon as an image)
        Some("--save-icon") if args.len() == 3 => {
            let rgba = icon::rgba(256);
            let mut ppm = b"P6\n256 256\n255\n".to_vec();
            for px in rgba.chunks(4) {
                let a = px[3] as u32; // blend onto light grey so the transparency is visible
                ppm.extend(
                    px[..3]
                        .iter()
                        .map(|&c| ((c as u32 * a + 200 * (255 - a)) / 255) as u8),
                );
            }
            std::fs::write(&args[2], ppm)?;
            return Ok(());
        }
        // Developer tool: airbridge --save-ico <out.ico>  (creates assets/airbridge.ico for the exe)
        Some("--save-ico") if args.len() == 3 => {
            std::fs::write(&args[2], icon::ico()?)?;
            return Ok(());
        }
        // Create/remove the firewall rule (asks for administrator rights via UAC)
        Some(flag @ ("--install-firewall" | "--remove-firewall")) => {
            if let Err(e) = platform::firewall(flag == "--install-firewall") {
                platform::message("AirBridge – Firewall", &format!("{e:#}"), true);
            }
            return Ok(());
        }
        _ => {}
    }
    // `--minimized` (autostart): start in the tray only, but only if the tray behaviour is enabled.
    let start_hidden =
        args.iter().any(|a| a == "--minimized") && shared::settings().minimize_to_tray;

    // The window MUST run on the main thread. Network and decoder run in their own threads.
    let latest: decoder::Latest = Arc::new(Mutex::new(None));

    // Channel network -> decoder: up to 120 frames may wait.
    let (video_tx, video_rx) = sync_channel::<(Instant, Vec<u8>)>(120);
    decoder::spawn(video_rx, Arc::clone(&latest), shared::wake);

    let (cmd_tx, cmd_rx) = channel::<NetCmd>();
    shared::set_net(cmd_tx);
    thread::spawn(move || {
        if let Err(e) = run_network(video_tx, cmd_rx) {
            log!("Netzwerk-Fehler: {e:#}");
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

    if let Err(e) = ui::run(latest, start_hidden) {
        platform::message("AirBridge – Fenster", &format!("{e}"), true);
    }
    Ok(())
}

fn run_network(video: SyncSender<(Instant, Vec<u8>)>, commands: Receiver<NetCmd>) -> Result<()> {
    let identity = Arc::new(identity::Identity::load_or_create(&identity::key_path())?);

    // Must stay in a variable: if it is dropped (ownership!), the announcement ends.
    let mut daemon = discovery::announce(&identity, &shared::settings().name)?;
    shared::with(|s| {
        s.device_id = identity.device_id();
        s.ip = shared::local_ip();
    });
    log!(
        "AirBridge sichtbar im Netzwerk (Geräte-ID {}).",
        identity.device_id()
    );

    // Mailbox: the RTSP thread puts the video keys here, the mirror thread picks them up.
    let slot: mirror::Slot = Arc::new(Mutex::new(None));
    mirror::spawn(Arc::clone(&slot), video)?;

    // The RTSP server blocks forever, so it gets its own thread.
    let rtsp_identity = Arc::clone(&identity);
    thread::spawn(move || {
        if let Err(e) = rtsp::serve(rtsp_identity, slot) {
            platform::message("AirBridge – Netzwerkfehler", &format!("{e:#}"), true);
            std::process::exit(1);
        }
    });

    // This thread waits for commands from the interface (e.g. a new device name).
    for cmd in commands {
        match cmd {
            NetCmd::Rename(name) => {
                let _ = daemon.shutdown();
                daemon = discovery::announce(&identity, &name)?;
                log!("Neuer Name im Netzwerk: {name}");
            }
        }
    }
    Ok(())
}
