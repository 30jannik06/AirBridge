// RTSP SETUP: Schlüssel ableiten und Ports für Video/Audio bereitstellen.
// iOS schickt SETUP zweimal:
//   1. mit `ekey`/`eiv` (verschlüsselter Schlüssel) -> wir antworten mit timingPort/eventPort
//   2. mit `streams` (Typ 110 = Mirroring-Video, 96 = Audio) -> wir antworten mit dataPort
#[cfg(feature = "audio")]
use crate::audio::{self, AudioParams};
use crate::config;
use crate::fairplay::FairPlay;
use crate::mirror::{Slot, StreamKeys};
use crate::ntp;
use anyhow::{Result, anyhow, bail};
use plist::{Dictionary, Value};
use sha2::{Digest, Sha512};
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Pro Verbindung: der entschlüsselte Basisschlüssel und offen gehaltene UDP-Sockets.
#[derive(Default)]
pub struct StreamState {
    aes_key: Option<[u8; 16]>,
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    aes_iv: Option<[u8; 16]>, // `eiv` aus SETUP 1, wird für die Audio-Entschlüsselung gebraucht
    // Solange die Sockets leben, bleiben ihre Ports reserviert (Drop = Port frei).
    _sockets: Vec<UdpSocket>,
    stop: Arc<AtomicBool>, // beendet den NTP-Thread, wenn die Verbindung endet
}

impl Drop for StreamState {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    let digest = h.finalize();
    let mut out = [0u8; 64];
    out.copy_from_slice(&digest);
    out
}

fn num(v: u64) -> Value {
    Value::Integer(v.into())
}

pub fn setup(
    st: &mut StreamState,
    fp: &FairPlay,
    ecdh_secret: Option<[u8; 32]>,
    peer_ip: Option<IpAddr>,
    mirror: &Slot,
    body: &[u8],
) -> Result<Vec<u8>> {
    let req = Value::from_reader(Cursor::new(body))?;
    let dict = req
        .as_dictionary()
        .ok_or_else(|| anyhow!("SETUP-Body ist kein Dictionary"))?;
    let mut res = Dictionary::new();

    // --- SETUP 1: Schlüssel ---
    if let (Some(ekey), Some(eiv)) = (
        dict.get("ekey").and_then(|v| v.as_data()),
        dict.get("eiv").and_then(|v| v.as_data()),
    ) {
        // Sicherheit: erst fragen (falls eingestellt), dann Schlüssel entschlüsseln.
        let text = |key: &str| {
            dict.get(key)
                .and_then(|v| v.as_string())
                .map(str::to_string)
        };
        let ip = peer_ip.map(|i| i.to_string()).unwrap_or_default();
        let device_name = text("name").unwrap_or_else(|| format!("iPhone ({ip})"));
        let device_id = text("deviceID")
            .or_else(|| text("macAddress"))
            .unwrap_or_else(|| ip.clone());
        if !crate::shared::approve(&device_id, &device_name, &ip) {
            bail!("Verbindung von {device_name} wurde abgelehnt");
        }
        let fp_key = fp.decrypt_key(ekey)?;
        // Mit Legacy-Pairing wird der FairPlay-Schlüssel noch mit dem ECDH-Geheimnis gehasht.
        let key: [u8; 16] = match ecdh_secret {
            Some(secret) => sha512(&[&fp_key, &secret])[..16].try_into()?,
            None => fp_key,
        };
        st.aes_key = Some(key);
        st.aes_iv = Some(
            eiv.try_into()
                .map_err(|_| anyhow!("eiv hat nicht 16 Byte"))?,
        );

        // Timing: wir fragen das iPhone per NTP nach der Zeit (siehe ntp.rs).
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        let timing = socket.local_addr()?.port();
        let remote_port = dict.get("timingPort").and_then(|v| v.as_unsigned_integer());
        if let (Some(ip), Some(rport)) = (peer_ip, remote_port) {
            ntp::start(
                socket.try_clone()?,
                SocketAddr::new(ip, rport as u16),
                Arc::clone(&st.stop),
            );
            log!("   NTP-Anfragen an {ip}:{rport}");
        }
        st._sockets.push(socket);
        res.insert("timingPort".into(), num(timing as u64));
        res.insert("eventPort".into(), num(0));
        log!("   SETUP 1: Schlüssel entschlüsselt, timingPort {timing}");
    }

    // --- SETUP 2: Streams ---
    if let Some(streams) = dict.get("streams").and_then(|v| v.as_array()) {
        let mut out = Vec::new();
        for s in streams {
            let d = s
                .as_dictionary()
                .ok_or_else(|| anyhow!("stream ist kein Dictionary"))?;
            let ty = d
                .get("type")
                .and_then(|v| v.as_unsigned_integer())
                .unwrap_or(0);
            let mut entry = Dictionary::new();
            match ty {
                110 => {
                    let id = d
                        .get("streamConnectionID")
                        .and_then(|v| {
                            v.as_unsigned_integer()
                                .or_else(|| v.as_signed_integer().map(|i| i as u64))
                        })
                        .ok_or_else(|| anyhow!("streamConnectionID fehlt"))?;
                    let base = st
                        .aes_key
                        .ok_or_else(|| anyhow!("Stream-Setup vor Schlüssel-Setup"))?;
                    let key = sha512(&[format!("AirPlayStreamKey{id}").as_bytes(), &base]);
                    let iv = sha512(&[format!("AirPlayStreamIV{id}").as_bytes(), &base]);
                    *mirror.lock().unwrap() = Some(StreamKeys {
                        key: key[..16].try_into()?,
                        iv: iv[..16].try_into()?,
                    });
                    entry.insert("dataPort".into(), num(config::MIRROR_PORT as u64));
                    entry.insert("type".into(), num(110));
                    log!(
                        "   SETUP 2: Video-Stream (ID {id}) -> Port {}",
                        config::MIRROR_PORT
                    );
                }
                96 => {
                    let data = UdpSocket::bind(("0.0.0.0", 0))?;
                    let control = UdpSocket::bind(("0.0.0.0", 0))?;
                    let (data_port, control_port) =
                        (data.local_addr()?.port(), control.local_addr()?.port());

                    #[cfg(feature = "audio")]
                    {
                        log!("   Audio-Anfrage vom iPhone: {d:?}");
                        let get = |name: &str| {
                            d.get(name)
                                .and_then(|v| v.as_unsigned_integer())
                                .unwrap_or(0)
                        };
                        let params = AudioParams {
                            ct: get("ct"),
                            samples_per_frame: get("spf"),
                            format: get("audioFormat"),
                        };
                        let key = st
                            .aes_key
                            .ok_or_else(|| anyhow!("Audio-Setup vor Schlüssel-Setup"))?;
                        let iv = st.aes_iv.ok_or_else(|| anyhow!("Audio-Setup ohne eiv"))?;
                        audio::start(data, control, key, iv, params, Arc::clone(&st.stop));
                    }
                    // Ohne das Feature `audio` beantworten wir die Anfrage trotzdem (das iPhone erwartet Ports),
                    // halten die Sockets offen und werfen die Audiopakete einfach weg: Bild ohne Ton.
                    #[cfg(not(feature = "audio"))]
                    {
                        log!(
                            "   Audio-Stream angefragt, aber dieses Programm wurde ohne Ton gebaut."
                        );
                        st._sockets.push(data);
                        st._sockets.push(control);
                    }
                    entry.insert("dataPort".into(), num(data_port as u64));
                    entry.insert("controlPort".into(), num(control_port as u64));
                    entry.insert("type".into(), num(96));
                    log!("   SETUP 2: Audio-Stream, Ports {data_port}/{control_port}");
                }
                other => bail!("unbekannter Stream-Typ {other}"),
            }
            out.push(Value::Dictionary(entry));
        }
        res.insert("streams".into(), Value::Array(out));
    }

    let mut bytes = Vec::new();
    Value::Dictionary(res).to_writer_binary(&mut bytes)?;
    Ok(bytes)
}
