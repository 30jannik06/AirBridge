// Minimaler RTSP/HTTP-Server auf Port 7000. Ein Thread pro Verbindung.
use crate::config;
use crate::fairplay::FairPlay;
use crate::identity::Identity;
use crate::mirror::Slot;
use crate::pairing::{self, VerifySession};
use crate::stream::{self, StreamState};
use anyhow::{Result, anyhow};
use plist::{Dictionary, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

struct Request {
    method: String,
    path: String,
    proto: String, // "RTSP/1.0" oder "HTTP/1.1" - Antwort muss dasselbe sprechen
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    code: u16,
    text: &'static str,
    content_type: Option<&'static str>,
    extra: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
}

impl Response {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            code: 200,
            text: "OK",
            content_type: Some(content_type),
            extra: Vec::new(),
            body,
        }
    }
    fn empty() -> Self {
        Self::status(200, "OK")
    }
    fn status(code: u16, text: &'static str) -> Self {
        Self {
            code,
            text,
            content_type: None,
            extra: Vec::new(),
            body: Vec::new(),
        }
    }
    fn header(mut self, name: &'static str, value: &'static str) -> Self {
        self.extra.push((name, value));
        self
    }
}

pub fn serve(identity: Arc<Identity>, mirror: Slot) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", config::RTSP_PORT))?;
    log!("RTSP lauscht auf Port {}", config::RTSP_PORT);
    for stream in listener.incoming() {
        let stream = stream?;
        // Arc = geteilter Besitz: jeder Thread bekommt einen eigenen "Zeiger" auf dieselbe Identity.
        let identity = Arc::clone(&identity);
        let mirror = Arc::clone(&mirror);
        // `move` verschiebt `stream` und `identity` in den neuen Thread (Ownership-Transfer).
        thread::spawn(move || {
            let peer = stream
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default();
            log!("[{peer}] verbunden");
            if let Err(e) = handle(stream, &identity, &mirror) {
                log!("[{peer}] Fehler: {e:#}");
            }
            log!("[{peer}] getrennt");
        });
    }
    Ok(())
}

fn handle(stream: TcpStream, identity: &Identity, mirror: &Slot) -> Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut conn = Conn {
        peer_ip: writer.peer_addr().ok().map(|a| a.ip()),
        ..Default::default()
    };

    while let Some(req) = read_request(&mut reader)? {
        log!(
            "-> {} {} (Body {} Byte)",
            req.method,
            req.path,
            req.body.len()
        );
        let resp = route(&req, identity, &mut conn, mirror).unwrap_or_else(|e| {
            log!("   Fehler in {}: {e:#}", req.path);
            Response::status(470, "Connection Authorization Required")
        });
        write_response(&mut writer, &req, &resp)?;
    }
    Ok(())
}

/// Alles, was zu EINER TCP-Verbindung gehört.
#[derive(Default)]
struct Conn {
    verify: Option<VerifySession>, // Option = "vielleicht noch nicht vorhanden"
    fairplay: FairPlay,
    stream: StreamState,
    peer_ip: Option<IpAddr>,
}

fn route(req: &Request, id: &Identity, conn: &mut Conn, mirror: &Slot) -> Result<Response> {
    let verify = &mut conn.verify;
    match (req.method.as_str(), req.path.as_str()) {
        ("SETUP", _) => {
            let secret = conn.verify.as_ref().map(|v| v.shared);
            let reply = stream::setup(
                &mut conn.stream,
                &conn.fairplay,
                secret,
                conn.peer_ip,
                mirror,
                &req.body,
            )?;
            Ok(Response::ok("application/x-apple-binary-plist", reply))
        }
        ("OPTIONS", _) => Ok(Response::empty().header(
            "Public",
            "SETUP, RECORD, FLUSH, TEARDOWN, OPTIONS, GET_PARAMETER, SET_PARAMETER",
        )),
        ("RECORD", _) => Ok(Response::empty()
            .header("Audio-Latency", "0")
            .header("Audio-Jack-Status", "connected; type=analog")),
        ("GET_PARAMETER", _) if req.body.starts_with(b"volume") => Ok(Response::ok(
            "text/parameters",
            b"volume: 0.000000\r\n".to_vec(),
        )),
        ("SET_PARAMETER", _) if parse_volume(&req.body).is_some() => {
            #[cfg(feature = "audio")]
            if let Some(db) = parse_volume(&req.body) {
                crate::playback::set_volume_db(db);
            }
            Ok(Response::empty())
        }
        ("FLUSH" | "TEARDOWN" | "GET_PARAMETER" | "SET_PARAMETER", _) => Ok(Response::empty()),
        ("POST", "/feedback" | "/command") => Ok(Response::empty()),
        ("POST", "/fp-setup") => {
            let reply = match req.body.len() {
                16 => conn.fairplay.setup(&req.body)?,
                164 => conn.fairplay.handshake(&req.body)?,
                n => return Err(anyhow!("fp-setup mit {n} Byte")),
            };
            Ok(Response::ok("application/octet-stream", reply))
        }
        // Mit Content-Type = erste Anfrage mit "qualifier"-Plist, ohne = die vollständige Geräteinfo.
        ("GET", "/info") if req.headers.contains_key("content-type") => Ok(Response::ok(
            "application/x-apple-binary-plist",
            info_plist(id)?,
        )),
        ("GET", "/info") => Ok(Response::ok(
            "application/x-apple-binary-plist",
            info_plist_full(id)?,
        )),
        ("POST", "/pair-setup") => Ok(Response::ok(
            "application/octet-stream",
            id.public().to_vec(),
        )),
        ("POST", "/pair-verify") => match req.body.first() {
            Some(1) => {
                let (session, reply) = pairing::verify_step1(id, &req.body)?;
                *verify = Some(session);
                Ok(Response::ok("application/octet-stream", reply))
            }
            Some(0) => {
                let session = verify
                    .as_mut()
                    .ok_or_else(|| anyhow!("Schritt 2 ohne Schritt 1"))?;
                pairing::verify_step2(session, &req.body)?;
                log!("   Pairing erfolgreich!");
                Ok(Response::ok("application/octet-stream", Vec::new()))
            }
            _ => Err(anyhow!("leerer pair-verify Body")),
        },
        _ => {
            log!("   (noch nicht implementiert)");
            Ok(Response::status(501, "Not Implemented"))
        }
    }
}

/// Liest die Lautstärke aus einem SET_PARAMETER-Body ("volume: -15.000000", in dB).
fn parse_volume(body: &[u8]) -> Option<f32> {
    let text = std::str::from_utf8(body).ok()?;
    let value = text.trim().strip_prefix("volume:")?;
    value.trim().parse().ok()
}

/// Vollständige Antwort auf GET /info (wie UxPlay), inkl. Audio-Formaten und Display.
fn info_plist_full(id: &Identity) -> Result<Vec<u8>> {
    let s = |v: &str| Value::String(v.to_string());
    let n = |v: u64| Value::Integer(v.into());
    let dict = |items: Vec<(&str, Value)>| {
        let mut d = Dictionary::new();
        for (k, v) in items {
            d.insert(k.to_string(), v);
        }
        Value::Dictionary(d)
    };
    let latency = |ty: u64| {
        dict(vec![
            ("type", n(ty)),
            ("inputLatencyMicros", n(0)),
            ("audioType", s("default")),
            ("outputLatencyMicros", Value::Boolean(false)),
        ])
    };
    let format = |ty: u64| {
        dict(vec![
            ("type", n(ty)),
            ("audioInputFormats", n(config::AUDIO_FORMATS)),
            ("audioOutputFormats", n(config::AUDIO_FORMATS)),
        ])
    };
    let display = dict(vec![
        ("uuid", s("e0ff8a27-6738-3d56-8a16-cc53aacee925")),
        ("widthPhysical", n(0)),
        ("heightPhysical", n(0)),
        ("width", n(1920)),
        ("height", n(1080)),
        ("widthPixels", n(1920)),
        ("heightPixels", n(1080)),
        ("rotation", Value::Boolean(false)),
        ("refreshRate", Value::Real(1.0 / 60.0)),
        ("maxFPS", n(30)),
        ("overscanned", Value::Boolean(false)),
        ("features", n(14)),
    ]);
    let root = dict(vec![
        ("deviceID", s(&id.device_id())),
        ("macAddress", s(&id.device_id())),
        ("pk", Value::Data(id.public().to_vec())),
        ("features", n(config::FEATURES)),
        ("name", s(&crate::shared::settings().name)),
        ("pi", s(&id.pi())),
        ("vv", n(2)),
        ("statusFlags", n(68)),
        ("keepAliveLowPower", Value::Boolean(true)),
        ("sourceVersion", s(config::SRCVERS)),
        ("keepAliveSendStatsAsBody", Value::Boolean(true)),
        ("model", s(config::MODEL)),
        ("initialVolume", Value::Real(0.0)),
        (
            "audioLatencies",
            Value::Array(vec![latency(100), latency(101)]),
        ),
        ("audioFormats", Value::Array(vec![format(100), format(101)])),
        ("displays", Value::Array(vec![display])),
    ]);
    let mut out = Vec::new();
    root.to_writer_binary(&mut out)?;
    Ok(out)
}

fn info_plist(id: &Identity) -> Result<Vec<u8>> {
    let s = |v: &str| Value::String(v.to_string());
    let n = |v: u64| Value::Integer(v.into());

    let mut display = Dictionary::new();
    display.insert("width".into(), n(1920));
    display.insert("height".into(), n(1080));
    display.insert("widthPixels".into(), n(1920));
    display.insert("heightPixels".into(), n(1080));
    display.insert("refreshRate".into(), Value::Real(60.0));
    display.insert("features".into(), n(14));
    display.insert("overscanned".into(), Value::Boolean(false));
    display.insert("rotation".into(), Value::Boolean(false));
    display.insert("uuid".into(), s("e5f7a68d-7b0f-4305-984b-974f677a150b"));

    let mut d = Dictionary::new();
    d.insert("deviceID".into(), s(&id.device_id()));
    d.insert("macAddress".into(), s(&id.device_id()));
    d.insert("name".into(), s(&crate::shared::settings().name));
    d.insert("model".into(), s(config::MODEL));
    d.insert("sourceVersion".into(), s(config::SRCVERS));
    d.insert("features".into(), n(config::FEATURES));
    d.insert("statusFlags".into(), n(4));
    d.insert("pi".into(), s(&id.pi()));
    d.insert("pk".into(), Value::Data(id.public().to_vec()));
    d.insert("vv".into(), n(2));
    d.insert("keepAliveLowPower".into(), n(1));
    d.insert("keepAliveSendStatsAsBody".into(), n(1));
    d.insert(
        "displays".into(),
        Value::Array(vec![Value::Dictionary(display)]),
    );

    let mut out = Vec::new();
    Value::Dictionary(d).to_writer_binary(&mut out)?;
    Ok(out)
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None); // Gegenseite hat die Verbindung geschlossen
    }
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow!("leere Request-Zeile"))?
        .to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let proto = parts.next().unwrap_or("RTSP/1.0").to_string();

    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        path,
        proto,
        headers,
        body,
    }))
}

fn write_response(w: &mut TcpStream, req: &Request, resp: &Response) -> Result<()> {
    let mut head = format!("{} {} {}\r\n", req.proto, resp.code, resp.text);
    if let Some(cseq) = req.headers.get("cseq") {
        head += &format!("CSeq: {cseq}\r\n");
    }
    head += &format!("Server: AirTunes/{}\r\n", config::SRCVERS);
    if let Some(ct) = resp.content_type {
        head += &format!("Content-Type: {ct}\r\n");
    }
    for (name, value) in &resp.extra {
        head += &format!("{name}: {value}\r\n");
    }
    head += &format!("Content-Length: {}\r\n\r\n", resp.body.len());
    w.write_all(head.as_bytes())?;
    w.write_all(&resp.body)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_parameter_is_parsed() {
        assert_eq!(parse_volume(b"volume: -15.500000\r\n"), Some(-15.5));
        assert_eq!(parse_volume(b"volume: 0.000000"), Some(0.0));
        assert_eq!(parse_volume(b"progress: 1/2/3"), None);
        assert_eq!(parse_volume(b"volume: abc"), None);
    }
}
