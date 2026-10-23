// Audio-Kanal (UDP): RTP-Pakete vom iPhone, verschlüsselt mit AES-128-CBC, Inhalt = AAC-ELD.
// RTP-Header (12 Byte): Byte 1 = Payload-Typ, Byte 2-3 = Sequenznummer, 4-7 = Zeitstempel, 8-11 = SSRC.
// Verschlüsselt sind nur volle 16-Byte-Blöcke des Payloads; ein Rest dahinter bleibt unverschlüsselt.
// Der IV ist für JEDES Paket derselbe (der `eiv` aus SETUP 1).
// Das iPhone schickt jedes Paket mehrfach ("redundantAudio"), darum werden Duplikate verworfen.
use crate::playback;
use aes::Aes128;
use cbc::cipher::block_padding::NoPadding;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use fdk_aac::dec::{Decoder, Transport};
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// AudioSpecificConfig für AAC-ELD, 44,1 kHz, Stereo (Objekttyp 39). So konfiguriert man den FDK-Decoder.
const ELD_44100_STEREO: [u8; 4] = [0xF8, 0xE8, 0x50, 0x00];
const CODEC_AAC_ELD: u64 = 8;

/// Was das iPhone beim SETUP über den Audio-Stream mitteilt.
#[derive(Debug, Clone, Copy)]
pub struct AudioParams {
    pub ct: u64, // Codec: 2 = ALAC, 4 = AAC-LC, 8 = AAC-ELD
    pub samples_per_frame: u64,
    pub format: u64, // Bitmaske des Audioformats
}

/// Entschlüsselt ein RTP-Audio-Paket und gibt den Payload (ohne RTP-Header) zurück.
fn decrypt(key: &[u8; 16], iv: &[u8; 16], packet: &[u8]) -> Option<Vec<u8>> {
    let payload = packet.get(12..)?;
    let whole = payload.len() / 16 * 16;
    let mut out = payload.to_vec();
    Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded::<NoPadding>(&mut out[..whole])
        .ok()?;
    Some(out)
}

pub fn start(
    data: UdpSocket,
    control: UdpSocket,
    key: [u8; 16],
    iv: [u8; 16],
    params: AudioParams,
    stop: Arc<AtomicBool>,
) {
    println!(
        "[Audio] Format vom iPhone: Codec {} ({}), {} Samples/Frame, Maske {:#x}",
        params.ct,
        if params.ct == CODEC_AAC_ELD {
            "AAC-ELD"
        } else {
            "nicht unterstützt"
        },
        params.samples_per_frame,
        params.format
    );

    // Kontrollkanal: Synchronisationspakete. Wir brauchen sie (noch) nicht, lesen sie aber leer.
    let stop_c = Arc::clone(&stop);
    thread::spawn(move || {
        let _ = control.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = [0u8; 2048];
        while !stop_c.load(Ordering::Relaxed) {
            let _ = control.recv(&mut buf);
        }
    });

    if params.ct != CODEC_AAC_ELD {
        println!("[Audio] Dieser Codec wird nicht unterstützt, es gibt keinen Ton.");
        return;
    }

    thread::spawn(move || {
        let mut decoder = Decoder::new(Transport::Raw);
        if let Err(e) = decoder.config_raw(&ELD_44100_STEREO) {
            return eprintln!("[Audio] AAC-ELD-Decoder konnte nicht gestartet werden: {e}");
        }
        let player = match playback::start(Arc::clone(&stop)) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("[Audio] Kein Ton möglich: {e:#}");
                None
            }
        };

        let _ = data.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = [0u8; 4096];
        let mut pcm = vec![0i16; 8192];
        let mut next_seq: Option<u16> = None;
        let (mut played, mut dropped, mut errors) = (0u64, 0u64, 0u32);

        while !stop.load(Ordering::Relaxed) {
            let Ok(len) = data.recv(&mut buf) else {
                continue;
            };
            if len < 13 {
                continue;
            }
            // Duplikate/Altes verwerfen: nur Pakete verarbeiten, die "neuer" sind als das zuletzt gespielte.
            let seq = u16::from_be_bytes([buf[2], buf[3]]);
            if let Some(next) = next_seq {
                let diff = seq.wrapping_sub(next) as i16;
                if diff < 0 || diff > 64 {
                    dropped += 1;
                    if diff.abs() > 1000 {
                        next_seq = Some(seq); // große Sprünge: neu synchronisieren
                    }
                    continue;
                }
            }
            next_seq = Some(seq.wrapping_add(1));

            let Some(payload) = decrypt(&key, &iv, &buf[..len]) else {
                continue;
            };
            let result = decoder
                .fill(&payload)
                .and_then(|_| decoder.decode_frame(&mut pcm));
            match result {
                Ok(()) => {
                    let info = decoder.stream_info();
                    let (channels, rate) = (info.numChannels as usize, info.sampleRate as u32);
                    let samples = decoder.decoded_frame_size();
                    if played == 0 {
                        println!(
                            "[Audio] Erstes Audio dekodiert: {channels} Kanäle, {rate} Hz, {samples} Werte/Frame"
                        );
                    }
                    if let (Some(p), true) = (&player, samples > 0 && samples <= pcm.len()) {
                        p.push(&pcm[..samples], channels, rate);
                    }
                    played += 1;
                }
                Err(e) => {
                    errors += 1;
                    if errors <= 5 {
                        eprintln!("[Audio] Dekodierfehler: {e}");
                    }
                }
            }
        }
        println!("[Audio] beendet: {played} Pakete gespielt, {dropped} Duplikate, {errors} Fehler");
    });
}
