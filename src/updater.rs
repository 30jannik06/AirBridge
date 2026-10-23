// Update check: asks GitHub for the latest release and compares its version with ours. It only NOTIFIES (the user
// installs the update themselves); nothing is downloaded or replaced automatically. The check is optional and off by
// default, because AirBridge otherwise never talks to the internet.
// HTTPS goes through WinHTTP, which is built into Windows (system certificates and proxy settings, no extra library).
use crate::shared;
use anyhow::{Result, anyhow, bail};
use std::ffi::c_void;
use std::thread;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{HSTRING, PCWSTR, w};

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/30jannik06/AirBridge/releases/latest";
const RELEASE_URL_PREFIX: &str = "https://github.com/30jannik06/AirBridge/";
const MAX_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String, // without a leading "v"
    pub url: String,     // release page in the browser
}

#[derive(Clone, Default, Debug, PartialEq)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed(String),
}

/// "v1.2.3" or "1.2.3" -> (1, 2, 3). Anything after the third number (e.g. "-beta") is ignored.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut number = || {
        let part = parts.next()?;
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    };
    Some((number()?, number()?, number()?))
}

/// `true` if `latest` is a higher version than `current`. Unreadable versions never count as newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Only links to our own repository are opened in the browser, whatever the server claims.
pub fn is_release_url(url: &str) -> bool {
    url.starts_with(RELEASE_URL_PREFIX) && !url.contains(char::is_whitespace)
}

pub fn parse_release(json: &[u8]) -> Result<Release> {
    let value: serde_json::Value = serde_json::from_slice(json)?;
    let tag = value["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("answer has no version"))?;
    parse_version(tag).ok_or_else(|| anyhow!("unreadable version {tag:?}"))?;
    let url = value["html_url"].as_str().unwrap_or_default();
    if !is_release_url(url) {
        bail!("unexpected release link");
    }
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        url: url.to_string(),
    })
}

/// Closes a WinHTTP handle when it goes out of scope.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn valid(handle: *mut c_void, what: &str) -> Result<Handle> {
    if handle.is_null() {
        bail!("{what} failed: {}", std::io::Error::last_os_error());
    }
    Ok(Handle(handle))
}

/// HTTPS GET, returns the body of a "200 OK" answer.
fn http_get(host: &str, path: &str) -> Result<Vec<u8>> {
    unsafe {
        let agent = HSTRING::from(format!("AirBridge/{}", env!("CARGO_PKG_VERSION")));
        let session = valid(
            WinHttpOpen(
                &agent,
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 8000, 8000)?;
        let connection = valid(
            WinHttpConnect(session.0, &HSTRING::from(host), 443, 0),
            "WinHttpConnect",
        )?;
        let request = valid(
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;

        let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n"
            .encode_utf16()
            .collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;

        let mut status: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            bail!("GitHub answered with status {status}");
        }

        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available)?;
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available.min(16 * 1024) as usize];
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )?;
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_BYTES {
                bail!("answer too large");
            }
        }
        Ok(body)
    }
}

/// Asks GitHub; `Ok(None)` means "we are up to date".
pub fn check() -> Result<Option<Release>> {
    let release = parse_release(&http_get(HOST, PATH)?)?;
    Ok(is_newer(&release.version, env!("CARGO_PKG_VERSION")).then_some(release))
}

/// Checks in the background and puts the result into the shared state.
pub fn spawn_check() {
    let already_running = shared::with(|s| {
        let running = s.update == UpdateState::Checking;
        s.update = UpdateState::Checking;
        running
    });
    if already_running {
        return;
    }
    thread::spawn(|| {
        let state = match check() {
            Ok(Some(release)) => {
                log!("[Update] Version {} is available", release.version);
                UpdateState::Available(release)
            }
            Ok(None) => {
                log!("[Update] AirBridge is up to date");
                UpdateState::UpToDate
            }
            Err(e) => {
                log!("[Update] Check failed: {e:#}");
                UpdateState::Failed(format!("{e:#}"))
            }
        };
        shared::with(|s| s.update = state);
        shared::wake();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_parsed() {
        assert_eq!(parse_version("0.3.1"), Some((0, 3, 1)));
        assert_eq!(parse_version("v1.12.0"), Some((1, 12, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn newer_means_strictly_higher() {
        assert!(is_newer("0.4.0", "0.3.1"));
        assert!(is_newer("v0.3.2", "0.3.1"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3.0", "0.3.1"));
        assert!(is_newer("0.10.0", "0.9.0")); // 0.10 is higher than 0.9 (numeric, not text)
        assert!(!is_newer("kaputt", "0.3.1"));
        assert!(!is_newer("0.4.0", "kaputt"));
    }

    #[test]
    fn release_json_is_parsed() {
        let json = br#"{"tag_name":"v0.4.0","html_url":"https://github.com/30jannik06/AirBridge/releases/tag/v0.4.0","draft":false}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, "0.4.0");
        assert!(release.url.ends_with("v0.4.0"));
    }

    #[test]
    fn suspicious_answers_are_rejected() {
        // a link to somewhere else must never be offered
        let evil = br#"{"tag_name":"v9.9.9","html_url":"https://evil.example/download"}"#;
        assert!(parse_release(evil).is_err());
        assert!(
            parse_release(br#"{"html_url":"https://github.com/30jannik06/AirBridge/x"}"#).is_err()
        );
        assert!(parse_release(br#"{"tag_name":"nonsense","html_url":"https://github.com/30jannik06/AirBridge/x"}"#).is_err());
        assert!(parse_release(b"not json").is_err());
        assert!(!is_release_url(
            "https://github.com/30jannik06/AirBridge-evil/"
        ));
        assert!(!is_release_url("http://github.com/30jannik06/AirBridge/"));
    }
    /// Talks to the real GitHub API (run with `cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn live_check_against_github() {
        let release = parse_release(&http_get(HOST, PATH).unwrap()).unwrap();
        println!("latest release on GitHub: {release:?}");
        assert!(parse_version(&release.version).is_some());
        assert!(is_release_url(&release.url));
    }
}
