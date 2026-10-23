// Audio playback on the default speakers (cpal = WASAPI on Windows).
// The device usually runs at 48 kHz while the iPhone delivers 44.1 kHz, so we convert the sample rate.
// A small queue sits between the network thread (delivers sound) and the audio thread (fetches sound).
use anyhow::{Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Error, FromSample, SampleFormat, SizedSample, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

/// Volume from the iPhone in dB (0 = full, -144 = mute), stored as bits in an atomic so any thread can set it.
static VOLUME_DB_BITS: AtomicU32 = AtomicU32::new(0);

/// Sets the volume (AirPlay scale: 0 dB = full, about -30 dB = very quiet, -144 = mute).
pub fn set_volume_db(db: f32) {
    VOLUME_DB_BITS.store(db.to_bits(), Ordering::Relaxed);
}

/// Factor the samples are multiplied by.
pub fn gain() -> f32 {
    let db = f32::from_bits(VOLUME_DB_BITS.load(Ordering::Relaxed));
    if db <= -144.0 {
        0.0
    } else {
        10f32.powf(db.min(0.0) / 20.0)
    }
}

/// The latest samples (mono) for the spectrum display.
static SCOPE: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// Returns the last `crate::spectrum::N` samples (or fewer if there are not enough yet).
pub fn scope() -> Vec<f32> {
    SCOPE.lock().map(|s| s.clone()).unwrap_or_default()
}

const PREFILL_MS: usize = 60; // only start playing once this much sound is buffered (prevents crackling)
const MAX_MS: usize = 300; // if more is buffered we catch up (drop old data), otherwise the delay keeps growing
const KEEP_MS: usize = 120;

struct Shared {
    queue: VecDeque<f32>, // stereo, alternating left/right, at the device sample rate
    primed: bool,
}

struct ResampleState {
    prev: [f32; 2], // last frame of the previous block
    pos: f64,       // read position relative to `prev`
}

pub struct Player {
    shared: Arc<Mutex<Shared>>,
    resample: Mutex<ResampleState>,
    dev_rate: u32,
}

/// Opens the default output device. The audio stream lives in its own thread until `stop` is set.
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
            drop(stream); // ends playback
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
    let prefill = config.sample_rate as usize * 2 * PREFILL_MS / 1000; // in single values (2 per frame)
    let shared = Arc::clone(shared);

    // Called by the audio system as soon as it needs more sound.
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
                        s.primed = false; // buffer empty: let it fill up again
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
    /// Takes decoded 16-bit PCM data (alternating left/right) and queues it for playback.
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

        // Linear interpolation from src_rate to the device sample rate.
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
            s.queue.drain(..excess & !1); // even count so left/right do not get swapped
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
        set_volume_db(5.0); // above 0 dB there is no amplification
        assert!((gain() - 1.0).abs() < 1e-6);
        set_volume_db(0.0);
    }
}
