// Audio-Wiedergabe über die Standard-Lautsprecher (cpal = WASAPI unter Windows).
// Das Gerät läuft meist mit 48 kHz, das iPhone liefert 44,1 kHz: darum rechnen wir die Abtastrate um.
// Zwischen Netzwerk-Thread (liefert Töne) und Audio-Thread (holt Töne) liegt eine kleine Warteschlange.
use anyhow::{Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Error, FromSample, SampleFormat, SizedSample, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

/// Lautstärke vom iPhone in dB (0 = voll, -144 = stumm), als Bits in einem Atomic, damit jeder Thread sie setzen kann.
static VOLUME_DB_BITS: AtomicU32 = AtomicU32::new(0);

/// Setzt die Lautstärke (AirPlay-Skala: 0 dB = voll, etwa -30 dB = sehr leise, -144 = stumm).
pub fn set_volume_db(db: f32) {
    VOLUME_DB_BITS.store(db.to_bits(), Ordering::Relaxed);
}

/// Faktor, mit dem die Samples multipliziert werden.
pub fn gain() -> f32 {
    let db = f32::from_bits(VOLUME_DB_BITS.load(Ordering::Relaxed));
    if db <= -144.0 {
        0.0
    } else {
        10f32.powf(db.min(0.0) / 20.0)
    }
}

/// Die letzten Samples (Mono) für die Spektrum-Anzeige.
static SCOPE: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// Gibt die letzten `crate::spectrum::N` Samples zurück (oder weniger, wenn noch nicht genug da sind).
pub fn scope() -> Vec<f32> {
    SCOPE.lock().map(|s| s.clone()).unwrap_or_default()
}

const PREFILL_MS: usize = 60; // erst abspielen, wenn so viel Ton im Puffer liegt (verhindert Knacken)
const MAX_MS: usize = 300; // liegt mehr im Puffer, holen wir auf (verwerfen Altes), sonst wächst die Verzögerung
const KEEP_MS: usize = 120;

struct Shared {
    queue: VecDeque<f32>, // Stereo, abwechselnd links/rechts, in der Abtastrate des Geräts
    primed: bool,
}

struct ResampleState {
    prev: [f32; 2], // letzter Frame des vorherigen Blocks
    pos: f64,       // Leseposition relativ zu `prev`
}

pub struct Player {
    shared: Arc<Mutex<Shared>>,
    resample: Mutex<ResampleState>,
    dev_rate: u32,
}

/// Öffnet das Standard-Ausgabegerät. Der Audio-Strom lebt in einem eigenen Thread, bis `stop` gesetzt wird.
pub fn start(stop: Arc<AtomicBool>) -> Result<Player> {
    let shared = Arc::new(Mutex::new(Shared {
        queue: VecDeque::new(),
        primed: false,
    }));
    let (tx, rx) = mpsc::channel::<std::result::Result<u32, String>>();
    let thread_shared = Arc::clone(&shared);
    thread::spawn(move || match open(&thread_shared) {
        Ok((stream, rate)) => {
            let _ = tx.send(Ok(rate));
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
            }
            drop(stream); // beendet die Wiedergabe
        }
        Err(e) => {
            let _ = tx.send(Err(format!("{e:#}")));
        }
    });
    let dev_rate = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow!("Audiogerät antwortet nicht"))?
        .map_err(|e| anyhow!(e))?;
    Ok(Player {
        shared,
        resample: Mutex::new(ResampleState {
            prev: [0.0; 2],
            pos: 0.0,
        }),
        dev_rate,
    })
}

fn open(shared: &Arc<Mutex<Shared>>) -> Result<(cpal::Stream, u32)> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow!("kein Audio-Ausgabegerät gefunden"))?;
    let supported = device.default_output_config().map_err(|e| anyhow!("{e}"))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    log!(
        "[Audio] Ausgabe: {} Kanäle, {rate} Hz, Format {format}",
        config.channels
    );
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, shared)?,
        SampleFormat::I16 => build::<i16>(&device, config, shared)?,
        other => bail!("Sampleformat {other} wird nicht unterstützt"),
    };
    stream.play().map_err(|e| anyhow!("{e}"))?;
    Ok((stream, rate))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: &Arc<Mutex<Shared>>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let prefill = config.sample_rate as usize * 2 * PREFILL_MS / 1000; // in Einzelwerten (2 pro Frame)
    let shared = Arc::clone(shared);

    // Wird vom Audio-System aufgerufen, sobald es neue Töne braucht.
    let callback = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        let mut s = shared.lock().unwrap();
        if !s.primed && s.queue.len() >= prefill {
            s.primed = true;
        }
        for frame in data.chunks_mut(channels) {
            let (l, r) = if s.primed {
                match (s.queue.pop_front(), s.queue.pop_front()) {
                    (Some(l), Some(r)) => (l, r),
                    _ => {
                        s.primed = false; // Puffer leer: neu auffüllen lassen
                        (0.0, 0.0)
                    }
                }
            } else {
                (0.0, 0.0)
            };
            for (i, out) in frame.iter_mut().enumerate() {
                let v = if channels == 1 {
                    (l + r) / 2.0
                } else {
                    [l, r].get(i).copied().unwrap_or(0.0)
                };
                *out = T::from_sample(v);
            }
        }
    };
    let on_error = |e: Error| log!("[Audio] Ausgabefehler: {e}");
    device
        .build_output_stream(config, callback, on_error, None)
        .map_err(|e| anyhow!("{e}"))
}

impl Player {
    /// Nimmt dekodierte 16-Bit-PCM-Daten (abwechselnd links/rechts) an und reiht sie zur Wiedergabe ein.
    pub fn push(&self, pcm: &[i16], channels: usize, src_rate: u32) {
        if channels == 0 || pcm.len() < channels {
            return;
        }
        let frames: Vec<[f32; 2]> = pcm
            .chunks_exact(channels)
            .map(|c| {
                let l = c[0] as f32 / 32768.0;
                [
                    l,
                    if channels > 1 {
                        c[1] as f32 / 32768.0
                    } else {
                        l
                    },
                ]
            })
            .collect();
        let n = frames.len();
        if let Ok(mut scope) = SCOPE.lock() {
            scope.extend(frames.iter().map(|f| (f[0] + f[1]) / 2.0));
            let keep = crate::spectrum::N;
            if scope.len() > keep {
                let excess = scope.len() - keep;
                scope.drain(..excess);
            }
        }

        // Lineare Interpolation von src_rate auf die Geräte-Abtastrate.
        let mut rs = self.resample.lock().unwrap();
        let prev = rs.prev;
        let at = |i: usize| if i == 0 { prev } else { frames[i - 1] };
        let step = src_rate as f64 / self.dev_rate as f64;
        let mut out = Vec::with_capacity((n as f64 / step) as usize * 2 + 4);
        let mut p = rs.pos;
        while p < n as f64 {
            let i = p as usize;
            let f = (p - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push(a[0] + (b[0] - a[0]) * f);
            out.push(a[1] + (b[1] - a[1]) * f);
            p += step;
        }
        rs.pos = p - n as f64;
        rs.prev = frames[n - 1];
        drop(rs);

        let g = gain();
        let mut s = self.shared.lock().unwrap();
        s.queue.extend(out.into_iter().map(|v| v * g));
        let per_ms = self.dev_rate as usize * 2 / 1000;
        if s.queue.len() > MAX_MS * per_ms {
            let excess = s.queue.len() - KEEP_MS * per_ms;
            s.queue.drain(..excess & !1); // gerade Anzahl, damit links/rechts nicht vertauscht werden
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_scale() {
        set_volume_db(0.0);
        assert!((gain() - 1.0).abs() < 1e-6);
        set_volume_db(-20.0);
        assert!((gain() - 0.1).abs() < 1e-4);
        set_volume_db(-144.0);
        assert_eq!(gain(), 0.0);
        set_volume_db(5.0); // über 0 dB wird nicht verstärkt
        assert!((gain() - 1.0).abs() < 1e-6);
        set_volume_db(0.0);
    }
}
