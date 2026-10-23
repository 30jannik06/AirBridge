// "Now playing": title, artist, album, cover, progress and volume that the iPhone sends via RTSP SET_PARAMETER.
//   Content-Type application/x-dmap-tagged  -> track info (DAAP/DMAP: 4-byte tag, 4-byte length, data)
//   Content-Type image/jpeg or image/png    -> cover
//   Content-Type text/parameters            -> "progress: start/current/end" (in 44100ths of a second) and "volume: -12.5"
use crate::shared;
use std::sync::Arc;
use std::time::Instant;

const RATE: f64 = 44100.0;

/// Where we reach the iPhone for remote control (play/pause/...).
#[derive(Clone, Debug)]
pub struct Dacp {
    pub id: String,                              // DACP-ID from the RTSP headers
    pub remote: String,                          // Active-Remote from the RTSP headers
    pub target: Option<(std::net::IpAddr, u16)>, // address and port of the remote control service, once found via mDNS
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub position: f64, // seconds at the time `at`
    pub duration: f64,
    pub at: Instant,
}

#[derive(Clone, Default)]
pub struct Music {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<Arc<Vec<u8>>>,
    pub cover_gen: u64, // counts up whenever a new cover arrived
    pub progress: Option<Progress>,
    pub volume_db: f32,
    pub dacp: Option<Dacp>,
}

impl Music {
    pub fn has_track(&self) -> bool {
        !self.title.is_empty() || !self.artist.is_empty()
    }

    /// Current position; while playing, it keeps counting since the last report.
    pub fn position_now(&self, playing: bool) -> Option<(f64, f64)> {
        let p = self.progress.as_ref()?;
        let extra = if playing {
            p.at.elapsed().as_secs_f64()
        } else {
            0.0
        };
        let pos = (p.position + extra).min(p.duration.max(0.0));
        Some((pos, p.duration))
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

/// Reads the track info from a DMAP block. Unknown fields are skipped,
/// broken or truncated data only leads to fewer results (no crash).
pub fn parse_dmap(data: &[u8]) -> Meta {
    let mut meta = Meta::default();
    walk(data, &mut meta, 0);
    meta
}

fn walk(data: &[u8], meta: &mut Meta, depth: u8) {
    let mut i = 0;
    while i + 8 <= data.len() {
        let code = &data[i..i + 4];
        let len = u32::from_be_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]) as usize;
        let Some(body) = data.get(i + 8..i + 8 + len) else {
            return;
        };
        let text = || Some(String::from_utf8_lossy(body).trim().to_string());
        match code {
            b"minm" => meta.title = text(),
            b"asar" => meta.artist = text(),
            b"asal" => meta.album = text(),
            // Containers in which the fields can be nested
            b"mlit" | b"mcon" | b"cmpa" if depth < 4 => walk(body, meta, depth + 1),
            _ => {}
        }
        i += 8 + len;
    }
}

/// "progress: start/current/end" -> (position, length) in seconds.
pub fn parse_progress(text: &str) -> Option<(f64, f64)> {
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("progress:"))?;
    let mut parts = value.trim().split('/').map(|p| p.trim().parse::<f64>());
    let (start, current, end) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if end < start || current < start {
        return None;
    }
    Some(((current - start) / RATE, (end - start) / RATE))
}

/// Reads the volume from "volume: -15.000000" (dB).
pub fn parse_volume(body: &[u8]) -> Option<f32> {
    let text = std::str::from_utf8(body).ok()?;
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("volume:"))?;
    value.trim().parse().ok()
}

/// Handles a SET_PARAMETER from the iPhone. Returns `true` if it was music info.
pub fn set_parameter(content_type: &str, body: &[u8]) -> bool {
    let ct = content_type.to_ascii_lowercase();
    if ct.starts_with("application/x-dmap-tagged") {
        let meta = parse_dmap(body);
        shared::with(|s| {
            let m = &mut s.music;
            m.title = meta.title.unwrap_or_default();
            m.artist = meta.artist.unwrap_or_default();
            m.album = meta.album.unwrap_or_default();
        });
        log!("[Musik] Titel: {}", shared::with(|s| s.music.title.clone()));
        shared::wake();
        true
    } else if ct.starts_with("image/") {
        if !body.is_empty() {
            let cover = Arc::new(body.to_vec());
            shared::with(|s| {
                s.music.cover = Some(cover);
                s.music.cover_gen += 1;
            });
            shared::wake();
        }
        true
    } else if let Some(db) = parse_volume(body) {
        shared::with(|s| s.music.volume_db = db);
        #[cfg(feature = "audio")]
        crate::playback::set_volume_db(db);
        shared::wake();
        true
    } else if let Some((position, duration)) =
        std::str::from_utf8(body).ok().and_then(parse_progress)
    {
        shared::with(|s| {
            s.music.progress = Some(Progress {
                position,
                duration,
                at: Instant::now(),
            })
        });
        shared::wake();
        true
    } else {
        false
    }
}

/// Forget everything (playback ended).
pub fn clear() {
    shared::with(|s| {
        let (dacp, generation) = (s.music.dacp.clone(), s.music.cover_gen + 1);
        s.music = Music::default();
        s.music.dacp = dacp;
        s.music.cover_gen = generation; // a different number than before, so the interface drops the old cover
    });
    shared::wake();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(code: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = code.to_vec();
        v.extend((body.len() as u32).to_be_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn dmap_flat_and_nested() {
        let mut inner = tag(b"minm", "Song Ü".as_bytes());
        inner.extend(tag(b"asar", b"Band"));
        inner.extend(tag(b"asal", b"Album"));
        inner.extend(tag(b"asgn", b"Rock")); // unknown, gets skipped
        assert_eq!(
            parse_dmap(&inner),
            Meta {
                title: Some("Song Ü".into()),
                artist: Some("Band".into()),
                album: Some("Album".into())
            }
        );
        let nested = tag(b"mlit", &inner);
        assert_eq!(parse_dmap(&nested).title.as_deref(), Some("Song Ü"));
    }

    #[test]
    fn dmap_truncated_or_garbage_does_not_panic() {
        let full = tag(b"minm", b"Titel");
        for n in 0..full.len() {
            let _ = parse_dmap(&full[..n]);
        }
        assert_eq!(parse_dmap(&full[..full.len() - 1]).title, None);
        let _ = parse_dmap(&[0xff; 64]); // huge lengths
        let _ = parse_dmap(b"");
    }

    #[test]
    fn progress_is_converted_to_seconds() {
        let (pos, dur) = parse_progress("progress: 1000/89200/45000000\r\n").unwrap();
        assert!((pos - 2.0).abs() < 1e-9);
        assert!((dur - (45_000_000.0 - 1000.0) / 44100.0).abs() < 1e-6);
        assert!(parse_progress("progress: 10/5/20").is_none()); // current before start
        assert!(parse_progress("progress: a/b/c").is_none());
        assert!(parse_progress("volume: -3").is_none());
    }

    #[test]
    fn volume_parameter_is_parsed() {
        assert_eq!(parse_volume(b"volume: -15.500000\r\n"), Some(-15.5));
        assert_eq!(parse_volume(b"volume: 0.000000"), Some(0.0));
        assert_eq!(parse_volume(b"progress: 1/2/3"), None);
        assert_eq!(parse_volume(b"volume: abc"), None);
    }

    #[test]
    fn position_advances_only_while_playing() {
        let m = Music {
            progress: Some(Progress {
                position: 10.0,
                duration: 100.0,
                at: Instant::now() - std::time::Duration::from_secs(5),
            }),
            ..Default::default()
        };
        let (paused, _) = m.position_now(false).unwrap();
        let (playing, dur) = m.position_now(true).unwrap();
        assert_eq!(paused, 10.0);
        assert!((playing - 15.0).abs() < 0.2);
        assert_eq!(dur, 100.0);
    }
}
