// Audio-Kanal (UDP): RTP-Pakete vom iPhone, verschlüsselt mit AES-128-CBC.
// Inhalt: ALAC (AirPlay-Musik, offener Codec) oder AAC-ELD (Ton beim Bildschirmspiegeln, braucht `nonfree-audio`).
// RTP-Header (12 Byte): Byte 1 = Payload-Typ, Byte 2-3 = Sequenznummer, 4-7 = Zeitstempel, 8-11 = SSRC.
// Verschlüsselt sind nur volle 16-Byte-Blöcke des Payloads; ein Rest dahinter bleibt unverschlüsselt.
// Der IV ist für JEDES Paket derselbe (der `eiv` aus SETUP 1).
// Das iPhone schickt Pakete teils mehrfach ("redundantAudio"), darum werden Duplikate verworfen.
use crate::playback;
use aes::Aes128;
use anyhow::{Result, anyhow};
use cbc::cipher::block_padding::NoPadding;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const CODEC_ALAC: u64 = 2;
const CODEC_AAC_ELD: u64 = 8;

/// Was das iPhone beim SETUP über den Audio-Stream mitteilt.
#[derive(Debug, Clone, Copy)]
pub struct AudioParams {
    pub ct: u64, // Codec: 2 = ALAC, 4 = AAC-LC, 8 = AAC-ELD
    pub samples_per_frame: u64,
    pub format: u64, // Bitmaske des Audioformats
}

/// Ein Decoder, egal für welchen Codec. Liefert 16-Bit-PCM, abwechselnd links/rechts.
enum Codec {
    Alac(alac::Decoder),
    #[cfg(feature = "nonfree-audio")]
    Eld(fdk_aac::dec::Decoder),
}

/// Ergebnis eines dekodierten Pakets: wie viele Werte in `pcm` stehen, wie viele Kanäle, welche Abtastrate.
struct Decoded {
    samples: usize,
    channels: usize,
    rate: u32,
}

impl Codec {
    fn new(params: &AudioParams) -> Result<Self> {
        match params.ct {
            CODEC_ALAC => {
                // Gleiche Werte wie in der SDP-Zeile von AirPlay: Rahmenlänge 352, 16 Bit, 2 Kanäle, 44,1 kHz.
                let spf = if params.samples_per_frame == 0 {
                    352
                } else {
                    params.samples_per_frame
                };
                let fmtp = format!("{spf} 0 16 40 10 14 2 255 0 0 44100");
                let info = alac::StreamInfo::from_sdp_format_parameters(&fmtp)
                    .map_err(|e| anyhow!("ALAC-Parameter: {e}"))?;
                Ok(Codec::Alac(alac::Decoder::new(info)))
            }
            #[cfg(feature = "nonfree-audio")]
            CODEC_AAC_ELD => {
                use fdk_aac::dec::{Decoder, Transport};
                // AudioSpecificConfig für AAC-ELD, 44,1 kHz, Stereo (Objekttyp 39).
                const ELD_44100_STEREO: [u8; 4] = [0xF8, 0xE8, 0x50, 0x00];
                let mut decoder = Decoder::new(Transport::Raw);
                decoder
                    .config_raw(&ELD_44100_STEREO)
                    .map_err(|e| anyhow!("AAC-ELD-Decoder: {e}"))?;
                Ok(Codec::Eld(decoder))
            }
            #[cfg(not(feature = "nonfree-audio"))]
            CODEC_AAC_ELD => Err(anyhow!(
                "Ton beim Bildschirmspiegeln (AAC-ELD) ist in diesem Programm nicht enthalten, Musik funktioniert"
            )),
            other => Err(anyhow!("Codec {other} wird nicht unterstützt")),
        }
    }

    fn decode(&mut self, payload: &[u8], pcm: &mut [i16]) -> Result<Decoded> {
        match self {
            Codec::Alac(d) => {
                let out = d
                    .decode_packet::<i16>(payload, pcm)
                    .map_err(|e| anyhow!("ALAC: {e}"))?;
                Ok(Decoded {
                    samples: out.len(),
                    channels: d.stream_info().channels() as usize,
                    rate: d.stream_info().sample_rate(),
                })
            }
            #[cfg(feature = "nonfree-audio")]
            Codec::Eld(d) => {
                d.fill(payload)
                    .and_then(|_| d.decode_frame(pcm))
                    .map_err(|e| anyhow!("{e}"))?;
                let info = d.stream_info();
                Ok(Decoded {
                    samples: d.decoded_frame_size(),
                    channels: info.numChannels as usize,
                    rate: info.sampleRate as u32,
                })
            }
        }
    }
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
    log!(
        "[Audio] Format vom iPhone: Codec {} ({}), {} Samples/Frame, Maske {:#x}",
        params.ct,
        match params.ct {
            CODEC_ALAC => "ALAC",
            CODEC_AAC_ELD => "AAC-ELD",
            _ => "unbekannt",
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

    let mut codec = match Codec::new(&params) {
        Ok(c) => c,
        Err(e) => return log!("[Audio] Kein Ton: {e:#}"),
    };

    thread::spawn(move || {
        let player = match playback::start(Arc::clone(&stop)) {
            Ok(p) => Some(p),
            Err(e) => {
                log!("[Audio] Kein Ton möglich: {e:#}");
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
                if !(0..=64).contains(&diff) {
                    dropped += 1;
                    if diff.abs() > 1000 {
                        next_seq = Some(seq); // große Sprünge: neu synchronisieren
                    }
                    continue;
                }
            }
            next_seq = Some(seq.wrapping_add(1));
            crate::shared::mark_audio();

            let Some(payload) = decrypt(&key, &iv, &buf[..len]) else {
                continue;
            };
            match codec.decode(&payload, &mut pcm) {
                Ok(d) => {
                    if played == 0 {
                        log!(
                            "[Audio] Erstes Audio dekodiert: {} Kanäle, {} Hz, {} Werte/Frame",
                            d.channels,
                            d.rate,
                            d.samples
                        );
                    }
                    if let (Some(p), true) = (&player, d.samples > 0 && d.samples <= pcm.len()) {
                        p.push(&pcm[..d.samples], d.channels, d.rate);
                    }
                    played += 1;
                }
                Err(e) => {
                    errors += 1;
                    if errors <= 5 {
                        log!("[Audio] Dekodierfehler: {e:#}");
                    }
                }
            }
        }
        log!("[Audio] beendet: {played} Pakete gespielt, {dropped} Duplikate, {errors} Fehler");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockModeEncrypt;

    fn params(ct: u64) -> AudioParams {
        AudioParams {
            ct,
            samples_per_frame: 352,
            format: 0,
        }
    }

    #[test]
    fn alac_decoder_can_be_created() {
        assert!(Codec::new(&params(CODEC_ALAC)).is_ok());
    }

    #[test]
    fn unknown_codec_is_rejected() {
        assert!(Codec::new(&params(99)).is_err());
        assert!(Codec::new(&params(4)).is_err()); // AAC-LC wird nicht unterstützt
    }

    #[test]
    fn decrypt_reverses_cbc_and_keeps_the_tail() {
        let (key, iv) = ([7u8; 16], [9u8; 16]);
        let plain: Vec<u8> = (0..37u8).collect(); // 2 volle Blöcke + 5 Byte Rest
        let mut enc = plain.clone();
        cbc::Encryptor::<Aes128>::new((&key).into(), (&iv).into())
            .encrypt_padded::<NoPadding>(&mut enc[..32], 32)
            .unwrap();
        let mut packet = vec![0x80, 0x60, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        packet.extend_from_slice(&enc);
        assert_eq!(decrypt(&key, &iv, &packet).unwrap(), plain);
    }

    #[test]
    fn short_packets_are_rejected() {
        assert!(decrypt(&[0; 16], &[0; 16], &[0u8; 5]).is_none());
    }
}
