use crate::config::KEY_FILE;
use anyhow::{Result, anyhow};
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha512};
use std::fs;
use std::path::{Path, PathBuf};

pub fn random32() -> Result<[u8; 32]> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("Zufall nicht verfügbar: {e}"))?;
    Ok(buf)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Persistent Ed25519 identity of the receiver. It is created once and stored in a
/// file so the iPhone recognises us the next time.
pub struct Identity {
    pub signing: SigningKey,
}

/// Where the key lives:
/// - **Portable version** (file `portable.txt` next to the exe): key next to the exe, everything stays in one folder.
/// - **Installed version** (normal case): `%APPDATA%\AirBridge`, because you may not write under "Program Files".
pub fn key_path() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = &exe_dir {
        if dir.join("portable.txt").exists() {
            return dir.join(KEY_FILE);
        }
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or(exe_dir)
        .unwrap_or_default();
    let dir = base.join("AirBridge");
    let path = dir.join(KEY_FILE);
    // Older versions stored the key in the working directory: take it over once so
    // the iPhone recognises the device.
    if !path.exists() && Path::new(KEY_FILE).exists() {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::copy(KEY_FILE, &path);
    }
    path
}

impl Identity {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        let seed: [u8; 32] = match fs::read(path) {
            // `try_into` converts the Vec into an array of fixed length (fails if != 32)
            Ok(bytes) => bytes
                .try_into()
                .map_err(|_| anyhow!("{} ist beschädigt (muss 32 Byte sein)", path.display()))?,
            Err(_) => {
                let seed = random32()?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, seed)?;
                log!("Neuer Schlüssel erzeugt: {}", path.display());
                seed
            }
        };
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    pub fn public(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// 16 bytes that depend only on THIS key: this gives every installation stable, unique identifiers.
    fn fingerprint(&self, label: &str) -> [u8; 16] {
        let mut h = Sha512::new();
        h.update(label.as_bytes());
        h.update(self.public());
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.finalize()[..16]);
        out
    }

    /// Device identifier in MAC format ("AA:BB:..."). Bit 1 = "locally administered", bit 0 = "not a multicast address".
    pub fn device_id(&self) -> String {
        let mut mac: [u8; 6] = self.fingerprint("AirBridge-DeviceID")[..6]
            .try_into()
            .unwrap();
        mac[0] = (mac[0] | 0x02) & 0xFE;
        mac.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Pairing identifier as UUID text (version 4).
    pub fn pi(&self) -> String {
        let mut b = self.fingerprint("AirBridge-PI");
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let h = hex(&b);
        format!(
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(seed_byte: u8) -> Identity {
        Identity {
            signing: SigningKey::from_bytes(&[seed_byte; 32]),
        }
    }

    #[test]
    fn device_id_looks_like_a_local_unicast_mac() {
        let id = identity(7).device_id();
        let bytes: Vec<u8> = id
            .split(':')
            .map(|p| u8::from_str_radix(p, 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 6);
        assert_eq!(
            bytes[0] & 0x02,
            0x02,
            "locally administered bit must be set"
        );
        assert_eq!(bytes[0] & 0x01, 0x00, "multicast bit must be clear");
    }

    #[test]
    fn ids_are_stable_and_differ_between_keys() {
        assert_eq!(identity(1).device_id(), identity(1).device_id());
        assert_ne!(identity(1).device_id(), identity(2).device_id());
        assert_ne!(identity(1).pi(), identity(2).pi());
    }

    #[test]
    fn pi_is_a_version_4_uuid() {
        let pi = identity(3).pi();
        let parts: Vec<&str> = pi.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
    }

    #[test]
    fn key_is_created_once_and_reloaded() {
        let dir = std::env::temp_dir().join(format!("airbridge-test-{}", std::process::id()));
        let path = dir.join("key.bin");
        let first = Identity::load_or_create(&path).unwrap();
        let second = Identity::load_or_create(&path).unwrap();
        assert_eq!(first.public(), second.public());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_key_file_is_rejected() {
        let dir = std::env::temp_dir().join(format!("airbridge-test-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.bin");
        fs::write(&path, b"too short").unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
