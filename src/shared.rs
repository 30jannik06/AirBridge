// State shared by the network threads and the interface: log, settings, pending connection request.
// One single global `Mutex` (instead of many channels) keeps this simple; the lock is never held for long.
use crate::settings::{self, Settings, Trusted};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const MAX_LOG: usize = 400;
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(30);

/// The user's answer to "May this device mirror?".
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Decision {
    Once,
    Always,
    Deny,
}

pub struct Pending {
    pub id: String,
    pub name: String,
    pub ip: String,
    reply: Sender<Decision>,
}

/// Commands from the interface to the network thread.
pub enum NetCmd {
    Rename(String),
}

pub struct Shared {
    pub settings: Settings,
    pub log: VecDeque<String>,
    pub device: Option<String>, // name of the iPhone that connected last
    pub pending: Option<Pending>,
    pub device_id: String,
    pub ip: String,
    pub latency_ms: f32,
    pub net_error: Option<String>,
    pub music: crate::music::Music,
    ctx: Option<eframe::egui::Context>,
    net: Option<Sender<NetCmd>>,
}

fn cell() -> &'static Mutex<Shared> {
    static CELL: OnceLock<Mutex<Shared>> = OnceLock::new();
    CELL.get_or_init(|| {
        Mutex::new(Shared {
            settings: settings::load(),
            log: VecDeque::new(),
            device: None,
            pending: None,
            device_id: String::new(),
            ip: String::new(),
            latency_ms: 0.0,
            net_error: None,
            music: Default::default(),
            ctx: None,
            net: None,
        })
    })
}

/// When sound last arrived (milliseconds since program start); the interface uses it to detect "playing".
static AUDIO_TICK_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
        + 10_000
}

/// Called by the audio thread for every packet.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub fn mark_audio() {
    AUDIO_TICK_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// `true` if sound arrived within the last second.
pub fn audio_recent() -> bool {
    let tick = AUDIO_TICK_MS.load(std::sync::atomic::Ordering::Relaxed);
    tick != 0 && now_ms().saturating_sub(tick) < 1200
}

/// Short access to the shared state. Do not nest (deadlock) and do not hold it for long.
pub fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    f(&mut cell().lock().unwrap_or_else(|e| e.into_inner()))
}

pub fn settings() -> Settings {
    with(|s| s.settings.clone())
}

/// Wakes the interface (from any thread).
pub fn wake() {
    let ctx = with(|s| s.ctx.clone());
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

pub fn set_ctx(ctx: eframe::egui::Context) {
    with(|s| s.ctx = Some(ctx));
}

pub fn set_net(tx: Sender<NetCmd>) {
    with(|s| s.net = Some(tx));
}

/// Writes a line to the log (console + log view). Used through the `log!` macro.
pub fn log(line: String) {
    println!("{line}");
    with(|s| {
        if s.log.len() >= MAX_LOG {
            s.log.pop_front();
        }
        s.log.push_back(line);
    });
}

/// Applies new settings: save them and, if necessary, inform the network thread.
pub fn apply_settings(mut new: Settings) {
    new.name = Settings::clean_name(&new.name);
    let (rename, net) = with(|s| {
        let rename = s.settings.name != new.name;
        s.settings = new.clone();
        (rename, s.net.clone())
    });
    if let Err(e) = settings::save(&new) {
        log(format!(
            "Einstellungen konnten nicht gespeichert werden: {e:#}"
        ));
    }
    if rename {
        if let Some(net) = net {
            let _ = net.send(NetCmd::Rename(new.name));
        }
    }
}

/// Called by the network thread when an iPhone wants to mirror. Blocks until a decision is made.
/// Returns `true` if the connection is allowed.
pub fn approve(id: &str, name: &str, ip: &str) -> bool {
    with(|s| s.device = Some(name.to_string()));
    let (rx, ok) = with(|s| {
        if s.settings.is_trusted(id) {
            return (None, true);
        }
        if s.settings.trusted_only || s.pending.is_some() {
            return (None, false); // unknown and only trusted devices allowed, or another request is already waiting
        }
        if !s.settings.require_confirm {
            return (None, true);
        }
        let (tx, rx) = mpsc::channel();
        s.pending = Some(Pending {
            id: id.to_string(),
            name: name.to_string(),
            ip: ip.to_string(),
            reply: tx,
        });
        (Some(rx), true)
    });
    let Some(rx) = rx else {
        if !ok {
            log(format!(
                "Verbindung von {name} ({ip}) abgelehnt: nicht vertraut"
            ));
        }
        return ok;
    };
    log(format!(
        "Verbindungsanfrage von {name} ({ip}): warte auf Bestätigung"
    ));
    wake();
    let decision = rx.recv_timeout(APPROVAL_TIMEOUT).unwrap_or(Decision::Deny);
    with(|s| s.pending = None);
    log(format!("Anfrage von {name}: {decision:?}"));
    decision != Decision::Deny
}

/// The interface's answer to the waiting request.
pub fn answer(decision: Decision) {
    let pending = with(|s| s.pending.take());
    let Some(p) = pending else { return };
    if decision == Decision::Always {
        let new = with(|s| {
            if !s.settings.is_trusted(&p.id) {
                s.settings.trusted.push(Trusted {
                    id: p.id.clone(),
                    name: p.name.clone(),
                });
            }
            s.settings.clone()
        });
        if let Err(e) = settings::save(&new) {
            log(format!(
                "Einstellungen konnten nicht gespeichert werden: {e:#}"
            ));
        }
    }
    let _ = p.reply.send(decision);
}

/// The local IPv4 address preferred by the operating system (nothing is sent in the process).
pub fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "?".into())
}
