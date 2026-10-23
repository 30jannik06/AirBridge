// Spectrum for the music view: a few bars are computed from the latest samples with an FFT.
use std::f32::consts::PI;

pub const N: usize = 1024; // FFT window size (power of 2)
pub const BANDS: usize = 32;
const RATE: f32 = 44100.0;
const F_MIN: f32 = 60.0;
const F_MAX: f32 = 14000.0;

/// FFT, in place, the length must be a power of 2 (radix-2, iterative).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    // bit-reversal order
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

/// Computes `BANDS` values (0..1) from exactly `N` samples. The bands are spread logarithmically.
pub fn bands(samples: &[f32]) -> [f32; BANDS] {
    let mut out = [0.0; BANDS];
    if samples.len() < N {
        return out;
    }
    let mut re = vec![0.0f32; N];
    let mut im = vec![0.0f32; N];
    for (i, v) in re.iter_mut().enumerate() {
        let hann = 0.5 - 0.5 * (2.0 * PI * i as f32 / (N - 1) as f32).cos();
        *v = samples[samples.len() - N + i] * hann;
    }
    fft(&mut re, &mut im);

    let bin_hz = RATE / N as f32;
    for (b, slot) in out.iter_mut().enumerate() {
        let lo = F_MIN * (F_MAX / F_MIN).powf(b as f32 / BANDS as f32);
        let hi = F_MIN * (F_MAX / F_MIN).powf((b + 1) as f32 / BANDS as f32);
        let first = ((lo / bin_hz) as usize).max(1);
        let last = ((hi / bin_hz).ceil() as usize).clamp(first + 1, N / 2);
        let peak = (first..last)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() / (N as f32 / 4.0))
            .fold(0.0f32, f32::max);
        let db = 20.0 * peak.max(1e-6).log10(); // 0 dB = full scale
        *slot = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32) -> Vec<f32> {
        (0..N)
            .map(|i| 0.8 * (2.0 * PI * freq * i as f32 / RATE).sin())
            .collect()
    }

    #[test]
    fn silence_is_zero() {
        assert!(bands(&vec![0.0; N]).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn too_few_samples_give_zero() {
        assert!(bands(&[0.5; 10]).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sine_lights_up_the_matching_band() {
        let loudest = |freq| {
            let b = bands(&sine(freq));
            (0..BANDS).max_by(|&x, &y| b[x].total_cmp(&b[y])).unwrap()
        };
        let (low, mid, high) = (loudest(150.0), loudest(1000.0), loudest(8000.0));
        assert!(low < mid && mid < high, "{low} {mid} {high}");
        // 1 kHz is roughly in the middle on the logarithmic scale
        assert!((12..=22).contains(&mid), "{mid}");
    }

    #[test]
    fn fft_matches_a_known_signal() {
        // impulse -> flat spectrum with magnitude 1
        let mut re = vec![0.0; 8];
        let mut im = vec![0.0; 8];
        re[0] = 1.0;
        fft(&mut re, &mut im);
        assert!(re.iter().all(|v| (v - 1.0).abs() < 1e-5));
        assert!(im.iter().all(|v| v.abs() < 1e-5));
    }
}
