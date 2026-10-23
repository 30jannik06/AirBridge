use crate::config::*;
use crate::identity::{Identity, hex};
use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo};

// `&str` = geliehener Text (nur lesen), `String` = besitzt seinen Text.
pub fn announce(id: &Identity) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?; // `?` = bei Fehler sofort zurückgeben
    let host = "airbridge.local.";
    let pk = hex(&id.public());
    let device_id = id.device_id();
    let pi = id.pi();

    // --- _airplay._tcp ---
    let airplay_txt = [
        ("deviceid", device_id.as_str()),
        ("features", FEATURES_TXT),
        ("model", MODEL),
        ("srcvers", SRCVERS),
        ("flags", "0x4"),
        ("pk", pk.as_str()),
        ("pi", pi.as_str()),
        ("vv", "2"),
    ];
    // IP leer + enable_addr_auto(): die Library trägt alle lokalen IPs selbst ein.
    let airplay = ServiceInfo::new(
        "_airplay._tcp.local.",
        NAME,
        host,
        "",
        RTSP_PORT,
        &airplay_txt[..],
    )?
    .enable_addr_auto();
    daemon.register(airplay)?;

    // --- _raop._tcp --- Instanzname MUSS "<MAC ohne Doppelpunkte>@<Name>" sein.
    let raop_name = format!("{}@{}", device_id.replace(':', ""), NAME);
    let raop_txt = [
        ("txtvers", "1"),
        ("ch", "2"),
        ("cn", AUDIO_CN),
        ("da", "true"),
        ("et", "0,3,5"),
        ("md", "0,1,2"),
        ("pw", "false"),
        ("sv", "false"),
        ("sr", "44100"),
        ("ss", "16"),
        ("tp", "UDP"),
        ("vn", "65537"),
        ("vs", SRCVERS),
        ("am", MODEL),
        ("sf", "0x4"),
        ("ft", FEATURES_TXT),
        ("pk", pk.as_str()),
    ];
    let raop = ServiceInfo::new(
        "_raop._tcp.local.",
        &raop_name,
        host,
        "",
        RTSP_PORT,
        &raop_txt[..],
    )?
    .enable_addr_auto();
    daemon.register(raop)?;

    Ok(daemon)
}
