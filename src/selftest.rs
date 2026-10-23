// Offline-Test des Decoders: `airbridge --decode-file mirror_dump.h264 300 out.ppm`
// dekodiert eine Annex-B-Datei und speichert das n-te Bild als PPM. Hilft beim Debuggen ohne iPhone.
use crate::decoder::H264Decoder;
use anyhow::{Result, bail};
use std::fs;

pub fn decode_file(path: &str, wanted: usize, out: &str) -> Result<()> {
    let data = fs::read(path)?;
    let mut dec = H264Decoder::new()?;

    // Startcodes (00 00 00 01) finden, damit wir die Datei in NAL-Einheiten zerlegen können.
    let starts: Vec<usize> = (0..data.len().saturating_sub(3))
        .filter(|&i| data[i..i + 4] == [0, 0, 0, 1])
        .collect();

    let mut pending: Vec<u8> = Vec::new(); // SPS/PPS/SEI sammeln, bis die Bild-NAL kommt
    let mut count = 0usize;
    let mut pictures = 0usize;
    let mut fixed_stream: Vec<u8> = Vec::new(); // Stream mit umgeschriebenem SPS, für ffmpeg-Kontrolle
    let mut saved = false;
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(data.len());
        let nal = &data[s..end];
        let nal_type = nal[4] & 0x1f;
        if nal_type == 7 && std::env::var("AB_RAW_SPS").is_err() {
            // SPS umschreiben wie im Live-Betrieb (mit AB_RAW_SPS=1 abschaltbar, zum Vergleichen).
            let fixed = crate::sps::low_latency(&nal[4..])
                .ok_or_else(|| anyhow::anyhow!("SPS nicht lesbar"))?;
            println!(
                "SPS umgeschrieben: {} -> {} Byte",
                nal.len() - 4,
                fixed.len()
            );
            fixed_stream.extend_from_slice(&[0, 0, 0, 1]);
            fixed_stream.extend_from_slice(&fixed);
            pending.extend_from_slice(&[0, 0, 0, 1]);
            pending.extend_from_slice(&fixed);
            continue;
        }
        fixed_stream.extend_from_slice(nal);
        pending.extend_from_slice(nal);
        if nal_type == 1 || nal_type == 5 {
            pictures += 1;
            for f in dec.decode(&pending)? {
                if count == wanted {
                    let mut ppm = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
                    for p in &f.pixels {
                        ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
                    }
                    fs::write(out, ppm)?;
                    println!(
                        "Frame {count} gespeichert: {}x{} -> {out}",
                        f.width, f.height
                    );
                    saved = true;
                }
                count += 1;
            }
            pending.clear();
        }
    }
    if std::env::var("AB_WRITE_FIXED").is_ok() {
        fs::write("fixed_sps.h264", &fixed_stream)?; // zum Gegenprüfen mit ffmpeg/ffprobe
    }
    println!(
        "{count} Frames dekodiert (ohne Flush), {} Pakete insgesamt",
        pictures
    );
    let extra = dec.flush()?.len();
    println!("Nach Flush kamen noch {extra} Frames: so viele hielt der Decoder zurück");
    if !saved {
        bail!("Frame {wanted} wurde nicht erzeugt");
    }
    Ok(())
}
