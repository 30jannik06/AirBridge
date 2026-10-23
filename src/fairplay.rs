// FairPlay handshake (POST /fp-setup) and decryption of the stream key.
// The actual crypto lives in the C code under vendor/playfair (from UxPlay, GPL).
use anyhow::{Result, bail};

// `unsafe extern "C"`: tells Rust which functions/data exist in the C code.
// Rust cannot check their correctness, so every access is `unsafe`.
unsafe extern "C" {
    static fp_reply: [[u8; 142]; 4];
    fn playfair_decrypt(message3: *mut u8, cipher_text: *mut u8, key_out: *mut u8);
}

const FP_HEADER: [u8; 12] = [
    0x46, 0x50, 0x4c, 0x59, 0x03, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x14,
];

/// FairPlay state of one connection: remembers the 164-byte message from stage 2.
#[derive(Default)]
pub struct FairPlay {
    keymsg: Option<[u8; 164]>,
}

impl FairPlay {
    /// Stage 1: 16 bytes in, 142 bytes out (fixed table, the mode is in req[14]).
    pub fn setup(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        if req.len() != 16 || req[4] != 3 {
            bail!("fp-setup Stufe 1: ungültige Anfrage");
        }
        let mode = req[14] as usize;
        if mode >= 4 {
            bail!("fp-setup: unbekannter Modus {mode}");
        }
        self.keymsg = None;
        // SAFETY: fp_reply is a constant table from the C code, mode has been checked.
        Ok(unsafe { fp_reply[mode] }.to_vec())
    }

    /// Stage 2: 164 bytes in, 32 bytes out (header + last 20 bytes of the request).
    pub fn handshake(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        if req.len() != 164 || req[4] != 3 {
            bail!("fp-setup Stufe 2: ungültige Anfrage");
        }
        self.keymsg = Some(req.try_into()?);
        let mut res = FP_HEADER.to_vec();
        res.extend_from_slice(&req[144..164]);
        Ok(res)
    }

    /// Decrypts the 72-byte `ekey` from SETUP into the 16-byte AES key.
    #[allow(dead_code)]
    pub fn decrypt_key(&self, ekey: &[u8]) -> Result<[u8; 16]> {
        let Some(mut keymsg) = self.keymsg else {
            bail!("ekey vor fp-setup Stufe 2");
        };
        let mut input: [u8; 72] = ekey.try_into()?;
        let mut out = [0u8; 16];
        // SAFETY: All buffers have exactly the sizes playfair_decrypt expects.
        unsafe { playfair_decrypt(keymsg.as_mut_ptr(), input.as_mut_ptr(), out.as_mut_ptr()) };
        Ok(out)
    }
}
