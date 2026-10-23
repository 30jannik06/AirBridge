// Mirror data channel (TCP, port 7100): the iPhone sends the H.264 video stream here.
// Every packet: 128-byte header + payload.
//   Header[0..4]  payload size (u32, little endian)
//   Header[4]     type: 0 = video (encrypted), 1 = SPS/PPS (unencrypted), otherwise heartbeat/report
//   Header[8..16] timestamp (NTP)
use crate::config;
use aes::Aes128;
use anyhow::Result;
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

type AesCtr = Ctr128BE<Aes128>;

#[derive(Clone)]
pub struct StreamKeys {
    pub key: [u8; 16],
    pub iv: [u8; 16],
}

/// Mailbox between the RTSP thread (puts the keys) and the mirror thread (picks them up).
pub type Slot = Arc<Mutex<Option<StreamKeys>>>;

const START_CODE: [u8; 4] = [0, 0, 0, 1];
/// `video` is the way to the decoder: every element is one picture (Annex B). An empty element = end of stream.
pub fn spawn(slot: Slot, video: SyncSender<(Instant, Vec<u8>)>) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", config::MIRROR_PORT))?;
    log!("Mirror-Kanal lauscht auf Port {}", config::MIRROR_PORT);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let keys = slot.lock().unwrap().clone();
            let video = video.clone();
            thread::spawn(move || match keys {
                Some(keys) => {
                    if let Err(e) = handle(stream, keys, &video) {
                        log!("[Mirror] Fehler: {e:#}");
                    }
                    let _ = video.send((Instant::now(), Vec::new())); // reset the window
                }
                None => log!("[Mirror] Verbindung ohne Schlüssel, verworfen"),
            });
        }
    });
    Ok(())
}

fn handle(
    mut stream: TcpStream,
    keys: StreamKeys,
    video: &SyncSender<(Instant, Vec<u8>)>,
) -> Result<()> {
    log!("[Mirror] iPhone verbunden");
    // ONE cipher for the whole connection: the keystream continues across all packets.
    let mut cipher = AesCtr::new(&keys.key.into(), &keys.iv.into());
    let mut sps_pps: Vec<u8> = Vec::new();
    let (mut frames, mut bad) = (0u32, 0u32);

    loop {
        let mut header = [0u8; 128];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let size = u32::from_le_bytes(header[0..4].try_into()?) as usize;
        if size > 16 * 1024 * 1024 {
            anyhow::bail!("unplausible Paketgröße {size}");
        }
        let mut payload = vec![0u8; size];
        stream.read_exact(&mut payload)?;

        match header[4] {
            0 => {
                cipher.apply_keystream(&mut payload);
                if !to_annex_b(&mut payload) {
                    bad += 1;
                    if bad <= 3 {
                        log!(
                            "[Mirror] Paket {frames}: ungültige NAL-Struktur (Entschlüsselung falsch?)"
                        );
                    }
                    continue;
                }
                if frames < 5 || frames % 120 == 0 {
                    log!(
                        "[Mirror] Frame {frames}: {size} Byte, NAL-Typ {}",
                        payload[4] & 0x1f
                    );
                }
                frames += 1;
                // If SPS/PPS came earlier, they are prepended to this picture.
                let mut picture = std::mem::take(&mut sps_pps);
                picture.extend_from_slice(&payload);
                if video.send((Instant::now(), picture)).is_err() {
                    break; // the decoder thread no longer exists
                }
            }
            1 => {
                if let Some(p) = parse_avcc(&payload) {
                    log!("[Mirror] SPS/PPS erhalten ({} Byte)", p.len());
                    sps_pps = p;
                } else {
                    log!("[Mirror] SPS/PPS-Paket nicht lesbar (HEVC?)");
                }
            }
            _ => {} // heartbeat / streaming report
        }
    }
    log!("[Mirror] getrennt ({frames} Frames, {bad} fehlerhaft)");
    Ok(())
}

/// Replaces the 4-byte length prefixes of the NAL units with start codes (Annex B).
/// Returns false if the structure is wrong (e.g. because the decryption was wrong).
fn to_annex_b(buf: &mut [u8]) -> bool {
    let mut pos = 0;
    while pos < buf.len() {
        if pos + 4 > buf.len() {
            return false;
        }
        let len = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        if len == 0 || len > buf.len() - pos - 4 {
            return false;
        }
        buf[pos..pos + 4].copy_from_slice(&START_CODE);
        pos += 4;
        if buf[pos] & 0x80 != 0 {
            return false; // forbidden_zero_bit must be 0
        }
        pos += len;
    }
    !buf.is_empty()
}

/// Reads SPS and PPS from the avcC block and builds Annex B data from them.
fn parse_avcc(p: &[u8]) -> Option<Vec<u8>> {
    let sps_len = u16::from_be_bytes(p.get(6..8)?.try_into().ok()?) as usize;
    let sps = p.get(8..8 + sps_len)?;
    let pps_len = u16::from_be_bytes(p.get(9 + sps_len..11 + sps_len)?.try_into().ok()?) as usize;
    let pps = p.get(11 + sps_len..11 + sps_len + pps_len)?;
    // Rewrite the SPS so the decoder does not hold back pictures for reordering (less delay).
    let sps = crate::sps::low_latency(sps).unwrap_or_else(|| sps.to_vec());
    let mut out = Vec::new();
    out.extend_from_slice(&START_CODE);
    out.extend_from_slice(&sps);
    out.extend_from_slice(&START_CODE);
    out.extend_from_slice(pps);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefixes_become_start_codes() {
        let mut buf = vec![0, 0, 0, 2, 0x65, 0xAA, 0, 0, 0, 1, 0x41];
        assert!(to_annex_b(&mut buf));
        assert_eq!(buf, [0, 0, 0, 1, 0x65, 0xAA, 0, 0, 0, 1, 0x41]);
    }

    #[test]
    fn broken_nal_structure_is_detected() {
        assert!(
            !to_annex_b(&mut vec![0, 0, 0, 9, 0x65]),
            "length exceeds the data"
        );
        assert!(!to_annex_b(&mut vec![0, 0, 0, 0]), "empty NAL");
        assert!(
            !to_annex_b(&mut vec![0, 0, 0, 1, 0xE5]),
            "forbidden_zero_bit set (wrong key?)"
        );
        assert!(!to_annex_b(&mut vec![0, 0]), "truncated length");
        assert!(!to_annex_b(&mut []), "empty buffer");
    }

    #[test]
    fn avcc_block_is_converted_to_annex_b() {
        let sps = [
            0x27, 0x64, 0x00, 0x1f, 0xac, 0x13, 0x14, 0x50, 0x20, 0x02, 0x27, 0x88, 0x96, 0x6e,
            0x02, 0x1a, 0x02, 0x04,
        ];
        let pps = [0x28, 0xee, 0x3c, 0xb0];
        // avcC: version, profile, compatibility, level, 0xFF, 0xE1 (1 SPS), SPS length, SPS, 1 PPS, PPS length, PPS
        let mut avcc = vec![1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, sps.len() as u8];
        avcc.extend_from_slice(&sps);
        avcc.extend_from_slice(&[1, 0, pps.len() as u8]);
        avcc.extend_from_slice(&pps);

        let out = parse_avcc(&avcc).expect("valid avcC");
        assert_eq!(out[..5], [0, 0, 0, 1, 0x27]);
        assert!(
            out.ends_with(&[0, 0, 0, 1, 0x28, 0xee, 0x3c, 0xb0]),
            "PPS must follow with a start code"
        );
    }

    #[test]
    fn truncated_avcc_is_rejected() {
        assert!(parse_avcc(&[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 50, 1, 2]).is_none());
        assert!(parse_avcc(&[]).is_none());
    }
}
