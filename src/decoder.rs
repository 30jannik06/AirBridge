// H.264 decoder based on Windows Media Foundation (built into Windows, no DLL needed).
// Input: Annex B data (start codes) per picture. Output: finished pictures as 0x00RRGGBB pixels.
use anyhow::{Result, anyhow, bail};
use std::mem::ManuallyDrop;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

/// What the decoder thread tells the window: a new picture or "stream ended".
pub enum Update {
    Frame(Frame),
    Cleared,
}

/// Mailbox for the window. It always shows only the LATEST picture (older ones are overwritten).
pub type Latest = Arc<Mutex<Option<Update>>>;

/// Starts the decoder thread. Empty data (`Vec::new()`) means "end of stream".
pub fn spawn(rx: Receiver<(Instant, Vec<u8>)>, latest: Latest, wake: impl Fn() + Send + 'static) {
    thread::spawn(move || {
        let mut decoder = match H264Decoder::new() {
            Ok(d) => d,
            Err(e) => return log!("[Decoder] Start fehlgeschlagen: {e:#}"),
        };
        let mut errors = 0u32;
        // Measurements: how long from "packet arrived" to "picture fully decoded"?
        let (mut n_in, mut n_out) = (0u32, 0u32);
        let (mut sum_ms, mut max_ms) = (0.0f64, 0.0f64);
        for (arrived, data) in rx {
            if data.is_empty() {
                *latest.lock().unwrap() = Some(Update::Cleared);
                wake();
                // Start with a fresh state for the next stream.
                match H264Decoder::new() {
                    Ok(d) => decoder = d,
                    Err(e) => return log!("[Decoder] Neustart fehlgeschlagen: {e:#}"),
                }
                continue;
            }
            n_in += 1;
            match decoder.decode(&data) {
                Ok(frames) => {
                    n_out += frames.len() as u32;
                    if let Some(frame) = frames.into_iter().last() {
                        *latest.lock().unwrap() = Some(Update::Frame(frame));
                        wake();
                        let ms = arrived.elapsed().as_secs_f64() * 1000.0;
                        sum_ms += ms;
                        crate::shared::with(|s| {
                            s.latency_ms = s.latency_ms * 0.95 + ms as f32 * 0.05
                        });
                        max_ms = max_ms.max(ms);
                    }
                    if n_in % 300 == 0 {
                        log!(
                            "[Decoder] Latenz Ankunft->Bild: Ø {:.1} ms, max {:.1} ms; im Decoder zurückgehalten: {}",
                            sum_ms / 300.0,
                            max_ms,
                            n_in as i64 - n_out as i64
                        );
                        (sum_ms, max_ms) = (0.0, 0.0);
                    }
                }
                Err(e) => {
                    errors += 1;
                    if errors <= 5 {
                        log!("[Decoder] Fehler: {e:#}");
                    }
                }
            }
        }
    });
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>, // 0x00RRGGBB
}

pub struct H264Decoder {
    transform: IMFTransform,
    coded_height: usize, // height of the buffers (possibly rounded up to 16)
    stride: usize,       // bytes per row in the NV12 buffer
    disp_w: usize,       // visible size (without padding)
    disp_h: usize,
    out_size: u32,
    provides_samples: bool,
    time: i64,
}

// SAFETY: The decoder is only used in exactly one thread (it is created there).
// `IMFTransform` is not automatically `Send`; we never move it between threads.

impl H264Decoder {
    /// Must be called in the thread that also uses the decoder (COM is bound to a thread).
    pub fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;

            let transform: IMFTransform =
                CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;

            // Prefer low latency (ignore errors, not every version knows it).
            match transform
                .GetAttributes()
                .and_then(|a| a.SetUINT32(&MF_LOW_LATENCY, 1))
            {
                Ok(()) => log!("[Decoder] Low-Latency-Modus aktiv"),
                Err(e) => log!("[Decoder] Low-Latency-Modus nicht verfügbar: {e}"),
            }

            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            transform.SetInputType(0, &input, 0)?;

            let mut dec = Self {
                transform,
                coded_height: 0,
                stride: 0,
                disp_w: 0,
                disp_h: 0,
                out_size: 0,
                provides_samples: false,
                time: 0,
            };
            dec.select_output_type()?;
            dec.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            dec.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            Ok(dec)
        }
    }

    /// Selects NV12 as the output format and reads size/stride. Called again on every format change.
    fn select_output_type(&mut self) -> Result<()> {
        unsafe {
            let mut i = 0;
            loop {
                let t = self
                    .transform
                    .GetOutputAvailableType(0, i)
                    .map_err(|_| anyhow!("Decoder bietet kein NV12 an"))?;
                if t.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                    self.transform.SetOutputType(0, &t, 0)?;
                    break;
                }
                i += 1;
            }

            let t = self.transform.GetOutputCurrentType(0)?;
            let size = t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
            let (w, h) = ((size >> 32) as usize, (size & 0xffff_ffff) as usize);

            // Visible area (the decoder often delivers sizes rounded up to 16).
            let mut area: MFVideoArea = std::mem::zeroed();
            let area_bytes = std::slice::from_raw_parts_mut(
                &mut area as *mut MFVideoArea as *mut u8,
                std::mem::size_of::<MFVideoArea>(),
            );
            let have_area = t
                .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, area_bytes, None)
                .is_ok()
                && area.Area.cx > 0
                && area.Area.cy > 0;
            let (dw, dh) = if have_area {
                (area.Area.cx as usize, area.Area.cy as usize)
            } else {
                (w, h)
            };

            let stride = match t.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
                Ok(s) => (s as i32).unsigned_abs() as usize,
                Err(_) => {
                    MFGetStrideForBitmapInfoHeader(0x3231_564E, w as u32)?.unsigned_abs() as usize
                } // 'NV12'
            };

            let info = self.transform.GetOutputStreamInfo(0)?;
            self.provides_samples =
                info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            self.out_size = info.cbSize;
            self.coded_height = h;
            self.stride = stride;
            self.disp_w = dw;
            self.disp_h = dh;
            log!("[Decoder] Ausgabeformat NV12 {w}x{h} (sichtbar {dw}x{dh}), Stride {stride}");
        }
        Ok(())
    }

    /// Decodes one picture (Annex B). Returns 0..n finished frames.
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        if let Err(e) = self.feed(data) {
            if e.code() == MF_E_NOTACCEPTING {
                // Decoder is full: collect the output first, then try again.
                self.drain(&mut frames)?;
                self.feed(data)?;
            } else {
                return Err(e.into());
            }
        }
        self.drain(&mut frames)?;
        Ok(frames)
    }

    /// Forces the decoder to output all held-back pictures immediately.
    pub fn flush(&mut self) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?
        };
        self.drain(&mut frames)?;
        // After a drain the data stream has to be "restarted", otherwise nothing comes out any more.
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?
        };
        Ok(frames)
    }

    fn feed(&mut self, data: &[u8]) -> windows::core::Result<()> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(data.len() as u32)?;
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(data.len() as u32)?;

            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(self.time)?;
            sample.SetSampleDuration(333_333)?; // 1/30 s in 100 ns units
            self.time += 333_333;
            self.transform.ProcessInput(0, &sample, 0)
        }
    }

    fn drain(&mut self, frames: &mut Vec<Frame>) -> Result<()> {
        loop {
            let provided = if self.provides_samples {
                None
            } else {
                unsafe {
                    let size = self
                        .out_size
                        .max((self.stride * self.coded_height * 3 / 2) as u32);
                    let buffer = MFCreateMemoryBuffer(size)?;
                    let sample = MFCreateSample()?;
                    sample.AddBuffer(&buffer)?;
                    Some(sample)
                }
            };
            let mut out = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(provided),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            let result = unsafe { self.transform.ProcessOutput(0, &mut out, &mut status) };
            // Take ownership back so COM objects are released correctly.
            let sample = unsafe { ManuallyDrop::take(&mut out[0].pSample) };
            unsafe { ManuallyDrop::drop(&mut out[0].pEvents) };

            match result {
                Ok(()) => {
                    if let Some(s) = sample {
                        frames.push(self.convert(&s)?);
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.select_output_type()?,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// NV12 (Y plane + interleaved UV plane) -> 0x00RRGGBB (BT.709, "limited range").
    fn convert(&self, sample: &IMFSample) -> Result<Frame> {
        unsafe {
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut ptr, None, Some(&mut len))?;
            let data = std::slice::from_raw_parts(ptr, len as usize);
            let result = nv12_to_rgb(
                data,
                self.stride,
                self.coded_height,
                self.disp_w,
                self.disp_h,
            );
            buffer.Unlock()?;
            result
        }
    }
}

fn nv12_to_rgb(data: &[u8], stride: usize, coded_h: usize, w: usize, h: usize) -> Result<Frame> {
    let uv_offset = stride * coded_h;
    if w == 0 || h == 0 || data.len() < uv_offset + stride * (h / 2) || stride < w {
        bail!(
            "NV12-Puffer passt nicht zur Größe ({} Byte, {w}x{h}, Stride {stride})",
            data.len()
        );
    }
    let mut pixels = vec![0u32; w * h];
    for y in 0..h {
        let y_row = &data[y * stride..y * stride + w];
        let uv_row = &data[uv_offset + (y / 2) * stride..];
        let out = &mut pixels[y * w..(y + 1) * w];
        for x in 0..w {
            let luma = (y_row[x] as i32 - 16).max(0) * 1192; // 1.164 * 1024
            let u = uv_row[x & !1] as i32 - 128;
            let v = uv_row[(x & !1) + 1] as i32 - 128;
            let r = (luma + 1836 * v) >> 10; // 1.793
            let g = (luma - 218 * u - 546 * v) >> 10; // 0.213 / 0.533
            let b = (luma + 2163 * u) >> 10; // 2.112
            out[x] = ((r.clamp(0, 255) as u32) << 16)
                | ((g.clamp(0, 255) as u32) << 8)
                | b.clamp(0, 255) as u32;
        }
    }
    Ok(Frame {
        width: w,
        height: h,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_black_and_white() {
        // 2x2 picture: Y plane (4 bytes) + one UV row (2 bytes), stride 2
        let black = nv12_to_rgb(&[16, 16, 16, 16, 128, 128], 2, 2, 2, 2).unwrap();
        assert!(black.pixels.iter().all(|&p| p == 0x000000));

        let white = nv12_to_rgb(&[235, 235, 235, 235, 128, 128], 2, 2, 2, 2).unwrap();
        for p in white.pixels {
            let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
            assert!(r > 250 && g > 250 && b > 250, "got {r},{g},{b}");
        }
    }

    #[test]
    fn nv12_respects_stride_and_cropping() {
        // visible 2x2, but the buffer is 4 bytes wide and 4 rows high (padding like the Windows decoder)
        let mut data = vec![16u8; 4 * 4];
        data.extend_from_slice(&[128u8; 4 * 2]);
        let f = nv12_to_rgb(&data, 4, 4, 2, 2).unwrap();
        assert_eq!((f.width, f.height, f.pixels.len()), (2, 2, 4));
    }

    #[test]
    fn nv12_rejects_buffers_that_are_too_small() {
        assert!(nv12_to_rgb(&[0; 3], 2, 2, 2, 2).is_err());
        assert!(nv12_to_rgb(&[0; 100], 2, 2, 0, 0).is_err());
    }
}
