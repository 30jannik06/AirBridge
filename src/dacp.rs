// Remote control of the iPhone (play/pause, next, previous) via DACP, the old iTunes remote protocol.
// The iPhone sends two headers in every RTSP request: `DACP-ID` and `Active-Remote`. Under the name
// `iTunes_Ctrl_<DACP-ID>` it announces a small HTTP service via mDNS (`_dacp._tcp`). We send it
// simple GET requests like `/ctrl-int/1/playpause`.
use crate::music::Dacp;
use crate::shared;
use anyhow::{Result, anyhow, bail};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// Commands the interface can send.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

impl Command {
    fn path(self) -> &'static str {
        match self {
            Command::PlayPause => "playpause",
            Command::Next => "nextitem",
            Command::Previous => "previtem",
        }
    }
}

/// Remembers the remote control data from the RTSP headers and looks for the matching service via mDNS (once per device).
pub fn learn(id: &str, remote: &str, peer: Option<IpAddr>) {
    let start_search = shared::with(|s| {
        let same = s
            .music
            .dacp
            .as_ref()
            .is_some_and(|d| d.id == id && d.remote == remote);
        if !same {
            s.music.dacp = Some(Dacp {
                id: id.to_string(),
                remote: remote.to_string(),
                target: None,
            });
        }
        !same
    });
    if start_search {
        let (id, remote) = (id.to_string(), remote.to_string());
        thread::spawn(move || match find(&id, peer) {
            Ok(target) => {
                log!("[Fernsteuerung] iPhone gefunden: {}:{}", target.0, target.1);
                shared::with(|s| {
                    if let Some(d) = s
                        .music
                        .dacp
                        .as_mut()
                        .filter(|d| d.id == id && d.remote == remote)
                    {
                        d.target = Some(target);
                    }
                });
                shared::wake();
            }
            Err(e) => log!("[Fernsteuerung] nicht verfügbar: {e:#}"),
        });
    }
}

/// Looks for the service `iTunes_Ctrl_<id>` and returns address and port.
fn find(id: &str, peer: Option<IpAddr>) -> Result<(IpAddr, u16)> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse("_dacp._tcp.local.")?;
    let wanted = format!("itunes_ctrl_{}", id.to_ascii_lowercase());
    let deadline = Instant::now() + Duration::from_secs(8);
    let result = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break Err(anyhow!("kein Dienst für {id} im Netzwerk gefunden"));
        }
        match events.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info))
                if info.get_fullname().to_ascii_lowercase().contains(&wanted) =>
            {
                let ips: Vec<IpAddr> = info
                    .get_addresses()
                    .iter()
                    .map(|a| a.to_ip_addr())
                    .collect();
                let ip = peer
                    .filter(|p| ips.contains(p))
                    .or_else(|| ips.iter().copied().find(|a| a.is_ipv4()))
                    .or(peer);
                match ip {
                    Some(ip) => break Ok((ip, info.get_port())),
                    None => break Err(anyhow!("Dienst ohne Adresse")),
                }
            }
            Ok(_) => {}
            Err(_) => break Err(anyhow!("kein Dienst für {id} im Netzwerk gefunden")),
        }
    };
    let _ = daemon.shutdown();
    result
}

/// Sends a command to the iPhone. Blocks briefly, so call it from its own thread.
pub fn send(command: Command) -> Result<()> {
    let dacp = shared::with(|s| s.music.dacp.clone())
        .ok_or_else(|| anyhow!("keine Fernsteuerung bekannt"))?;
    let (ip, port) = dacp
        .target
        .ok_or_else(|| anyhow!("iPhone-Dienst noch nicht gefunden"))?;
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let request = format!(
        "GET /ctrl-int/1/{} HTTP/1.1\r\nHost: {ip}:{port}\r\nActive-Remote: {}\r\nConnection: close\r\n\r\n",
        command.path(),
        dacp.remote
    );
    stream.write_all(request.as_bytes())?;
    let mut head = [0u8; 64];
    let n = stream.read(&mut head)?;
    let line = String::from_utf8_lossy(&head[..n]);
    let status = line.split_whitespace().nth(1).unwrap_or("");
    if status.starts_with('2') {
        Ok(())
    } else {
        bail!("das iPhone antwortete mit {status:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_paths() {
        assert_eq!(Command::PlayPause.path(), "playpause");
        assert_eq!(Command::Next.path(), "nextitem");
        assert_eq!(Command::Previous.path(), "previtem");
    }

    #[test]
    fn send_without_target_fails_cleanly() {
        shared::with(|s| s.music.dacp = None);
        assert!(send(Command::PlayPause).is_err());
    }

    #[test]
    fn send_talks_http_to_the_target() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let n = conn.read(&mut buf).unwrap();
            conn.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        shared::with(|s| {
            s.music.dacp = Some(Dacp {
                id: "ABC".into(),
                remote: "12345".into(),
                target: Some(("127.0.0.1".parse().unwrap(), port)),
            })
        });
        send(Command::Next).unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /ctrl-int/1/nextitem HTTP/1.1"));
        assert!(request.contains("Active-Remote: 12345"));
    }
}
