// FairPlay-Handshake (POST /fp-setup) und Entschlüsselung des Stream-Keys.
// Die eigentliche Krypto steckt im C-Code unter vendor/playfair (aus UxPlay, GPL).
use anyhow::{Result, bail};

// `unsafe extern "C"`: Wir sagen Rust, welche Funktionen/Daten es im C-Code gibt.
// Rust kann deren Korrektheit nicht prüfen, darum ist jeder Zugriff `unsafe`.
unsafe extern "C" {
    static fp_reply: [[u8; 142]; 4];
    fn playfair_decrypt(message3: *mut u8, cipher_text: *mut u8, key_out: *mut u8);
}

const FP_HEADER: [u8; 12] = [
    0x46, 0x50, 0x4c, 0x59, 0x03, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x14,
];

/// FairPlay-Zustand einer Verbindung: merkt sich die 164-Byte-Nachricht aus Stufe 2.
#[derive(Default)]
pub struct FairPlay {
    keymsg: Option<[u8; 164]>,
}

impl FairPlay {
    /// Stufe 1: 16 Byte rein, 142 Byte raus (feste Tabelle, Modus steht in req[14]).
    pub fn setup(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        if req.len() != 16 || req[4] != 3 {
            bail!("fp-setup Stufe 1: ungültige Anfrage");
        }
        let mode = req[14] as usize;
        if mode >= 4 {
            bail!("fp-setup: unbekannter Modus {mode}");
        }
        self.keymsg = None;
        // SAFETY: fp_reply ist eine konstante Tabelle aus dem C-Code, mode ist geprüft.
        Ok(unsafe { fp_reply[mode] }.to_vec())
    }

    /// Stufe 2: 164 Byte rein, 32 Byte raus (Header + letzte 20 Byte der Anfrage).
    pub fn handshake(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        if req.len() != 164 || req[4] != 3 {
            bail!("fp-setup Stufe 2: ungültige Anfrage");
        }
        self.keymsg = Some(req.try_into()?);
        let mut res = FP_HEADER.to_vec();
        res.extend_from_slice(&req[144..164]);
        Ok(res)
    }

    /// Entschlüsselt den 72-Byte-`ekey` aus SETUP zum 16-Byte-AES-Schlüssel (Phase 3).
    #[allow(dead_code)]
    pub fn decrypt_key(&self, ekey: &[u8]) -> Result<[u8; 16]> {
        let Some(mut keymsg) = self.keymsg else {
            bail!("ekey vor fp-setup Stufe 2");
        };
        let mut input: [u8; 72] = ekey.try_into()?;
        let mut out = [0u8; 16];
        // SAFETY: Alle Puffer haben genau die Größen, die playfair_decrypt erwartet.
        unsafe { playfair_decrypt(keymsg.as_mut_ptr(), input.as_mut_ptr(), out.as_mut_ptr()) };
        Ok(out)
    }
}
