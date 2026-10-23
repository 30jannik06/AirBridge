// Legacy AirPlay pairing (like UxPlay/shairplay):
//   /pair-setup  : the server sends its Ed25519 public key (32 bytes).
//   /pair-verify : two steps with Curve25519 ECDH and Ed25519 signatures,
//                  the signature is encrypted with AES-128-CTR.
// (ChaCha20 only comes with the newer AirPlay 2 pairing, which iOS does not ask for here.)
use crate::identity::{Identity, random32};
use aes::Aes128;
use anyhow::{Result, bail};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use sha2::{Digest, Sha512};
use x25519_dalek::{X25519_BASEPOINT_BYTES, x25519};

type AesCtr = Ctr128BE<Aes128>;

/// State between verify step 1 and 2 (belongs to exactly one TCP connection).
pub struct VerifySession {
    ecdh_ours: [u8; 32],
    ecdh_theirs: [u8; 32],
    ed_theirs: [u8; 32],
    key: [u8; 16],
    iv: [u8; 16],
    pub shared: [u8; 32], // needed later to derive the stream key
}

fn derive(label: &str, shared: &[u8; 32]) -> [u8; 16] {
    let mut h = Sha512::new();
    h.update(label.as_bytes());
    h.update(shared);
    let digest = h.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

pub fn verify_step1(id: &Identity, body: &[u8]) -> Result<(VerifySession, Vec<u8>)> {
    if body.len() != 68 || body[0] != 1 {
        bail!(
            "pair-verify Schritt 1: unerwartetes Format ({} Byte)",
            body.len()
        );
    }
    let ecdh_theirs: [u8; 32] = body[4..36].try_into()?;
    let ed_theirs: [u8; 32] = body[36..68].try_into()?;

    let secret = random32()?;
    let ecdh_ours = x25519(secret, X25519_BASEPOINT_BYTES);
    let shared = x25519(secret, ecdh_theirs);

    let key = derive("Pair-Verify-AES-Key", &shared);
    let iv = derive("Pair-Verify-AES-IV", &shared);
    let mut cipher = AesCtr::new(&key.into(), &iv.into());

    // We sign (our key || their key) and encrypt the signature.
    let mut msg = Vec::with_capacity(64);
    msg.extend_from_slice(&ecdh_ours);
    msg.extend_from_slice(&ecdh_theirs);
    let mut sig = id.signing.sign(&msg).to_bytes();
    cipher.apply_keystream(&mut sig);

    let mut reply = Vec::with_capacity(96);
    reply.extend_from_slice(&ecdh_ours);
    reply.extend_from_slice(&sig);

    Ok((
        VerifySession {
            ecdh_ours,
            ecdh_theirs,
            ed_theirs,
            key,
            iv,
            shared,
        },
        reply,
    ))
}

/// Step 2: checks the (encrypted) signature from the iPhone.
pub fn verify_step2(session: &mut VerifySession, body: &[u8]) -> Result<()> {
    if body.len() != 68 {
        bail!(
            "pair-verify Schritt 2: unerwartetes Format ({} Byte)",
            body.len()
        );
    }
    // Fresh cipher, then skip the 64 bytes of keystream that step 1 used up
    // for our own signature.
    let mut cipher = AesCtr::new(&session.key.into(), &session.iv.into());
    cipher.apply_keystream(&mut [0u8; 64]);

    let mut sig: [u8; 64] = body[4..68].try_into()?;
    cipher.apply_keystream(&mut sig);

    // The iPhone signs (its key || our key).
    let mut msg = Vec::with_capacity(64);
    msg.extend_from_slice(&session.ecdh_theirs);
    msg.extend_from_slice(&session.ecdh_ours);
    let vk = VerifyingKey::from_bytes(&session.ed_theirs)?;
    vk.verify(&msg, &Signature::from_bytes(&sig))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    /// Independently replays the iPhone side of the protocol and checks that our server works with it.
    #[test]
    fn pair_verify_roundtrip_with_independent_client() {
        let server = Identity {
            signing: SigningKey::from_bytes(&[5; 32]),
        };
        let client_key = SigningKey::from_bytes(&[9; 32]);
        let client_secret = [11u8; 32];
        let client_pub = x25519(client_secret, X25519_BASEPOINT_BYTES);

        // Step 1: 4-byte header (first byte = 1) + Curve25519 key + Ed25519 key
        let mut step1 = vec![1, 0, 0, 0];
        step1.extend_from_slice(&client_pub);
        step1.extend_from_slice(&client_key.verifying_key().to_bytes());
        let (mut session, reply) = verify_step1(&server, &step1).unwrap();
        assert_eq!(reply.len(), 96);

        // Client: derive the shared secret and check the encrypted server signature
        let server_pub: [u8; 32] = reply[..32].try_into().unwrap();
        let shared = x25519(client_secret, server_pub);
        assert_eq!(shared, session.shared);
        let mut cipher = AesCtr::new(
            &derive("Pair-Verify-AES-Key", &shared).into(),
            &derive("Pair-Verify-AES-IV", &shared).into(),
        );
        let mut sig: [u8; 64] = reply[32..].try_into().unwrap();
        cipher.apply_keystream(&mut sig);
        let mut msg = server_pub.to_vec();
        msg.extend_from_slice(&client_pub);
        server
            .signing
            .verifying_key()
            .verify(&msg, &Signature::from_bytes(&sig))
            .expect("server signature");

        // Step 2: client signs (its key || server key); the keystream continues
        let mut msg2 = client_pub.to_vec();
        msg2.extend_from_slice(&server_pub);
        let mut client_sig = client_key.sign(&msg2).to_bytes();
        cipher.apply_keystream(&mut client_sig);
        let mut step2 = vec![0, 0, 0, 0];
        step2.extend_from_slice(&client_sig);

        let mut tampered = step2.clone();
        tampered[10] ^= 1;
        assert!(
            verify_step2(&mut session, &tampered).is_err(),
            "manipulated signature must be rejected"
        );
        verify_step2(&mut session, &step2).expect("valid signature");
    }

    #[test]
    fn malformed_requests_are_rejected() {
        let server = Identity {
            signing: SigningKey::from_bytes(&[5; 32]),
        };
        assert!(verify_step1(&server, &[1; 10]).is_err(), "wrong length");
        assert!(
            verify_step1(&server, &[0; 68]).is_err(),
            "wrong step marker"
        );
    }
}
