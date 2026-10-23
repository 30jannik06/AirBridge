# Protocol notes

What AirBridge does to be accepted by iOS as a "legacy AirPlay mirroring" target. Everything here was verified against a
real iPhone. Reference implementations: UxPlay, shairplay.

## 1. Discovery (mDNS)

Two services, same port 7000:

- `_airplay._tcp` named `AirBridge`. TXT: `deviceid`, `features`, `model=AppleTV3,2`, `srcvers=220.68`, `flags=0x4`, `pk`, `pi`, `vv=2`.
- `_raop._tcp` named `<MAC without colons>@AirBridge`. TXT: `ch`, `cn`, `et`, `sf`, `tp=UDP`, `vn`, `vs`, `am`, `ft`, `pk`.

Features are `0x5A7FFEE6` (video, screen mirroring, FairPlay, legacy pairing, audio). `pk` is the Ed25519 public key (hex).
`cn=0,1` advertises PCM and ALAC; AAC-ELD (ct 8) is still what iOS uses for mirroring audio.

## 2. Pairing (legacy)

- `POST /pair-setup`: reply with our 32-byte Ed25519 public key.
- `POST /pair-verify`, two steps:
  1. Client sends `01 00 00 00 | X25519 pub (32) | Ed25519 pub (32)`. We reply with our X25519 public key plus an
     encrypted signature over `ourX25519 || theirX25519`.
  2. Client sends `00 00 00 00 | signature (64)` (encrypted). We verify it against their Ed25519 key.
- Cipher: AES-128-CTR, key `SHA512("Pair-Verify-AES-Key" || shared)[..16]`, IV `SHA512("Pair-Verify-AES-IV" || shared)[..16]`.
  Step 1 encrypts the signature after skipping nothing; step 2 uses a **fresh** cipher and skips 64 bytes of keystream
  (this was the "signature error" bug when done on one continuing cipher twice).

## 3. FairPlay (`/fp-setup`)

- Stage 1: 16-byte request → 142-byte reply, chosen from a fixed table by `req[14]` (the mode).
- Stage 2: 164-byte request → 32-byte reply: 12-byte header + last 20 bytes of the request.
- Later, `SETUP` carries a 72-byte `ekey`; `playfair_decrypt` turns it into a 16-byte key.

## 4. SETUP

iOS sends it twice:

1. With `ekey`, `eiv`, `timingPort`. Key derivation: `key16 = SHA512(fairplay_key || ecdh_secret)[..16]`.
   Reply `timingPort` (our UDP port), `eventPort: 0`. **We must send NTP requests to the iPhone's `timingPort`** or iOS
   drops the connection a few seconds later.
2. With `streams`:
   - type 110 (video): derive `key/iv = SHA512("AirPlayStreamKey<ID>" / "AirPlayStreamIV<ID>" || key16)[..16]`, reply `dataPort: 7100`.
   - type 96 (audio): bind two UDP sockets, reply `dataPort` and `controlPort`.

Also required to keep iOS happy: a complete `GET /info` plist (including `audioFormats`) and a `GET_PARAMETER volume` reply (`volume: 0.000000`).

## 5. Video stream (TCP 7100)

Each packet: 128-byte header, then the payload.

| Header field | Meaning |
|---|---|
| bytes 0..4 (u32 LE) | payload size |
| byte 4 | type: 0 = video frame, 1 = SPS/PPS (avcC), 2 / 5 = heartbeat / report |

- Video payloads are encrypted with **one continuous AES-128-CTR stream** across all video packets (not reset per packet).
- Payload is AVCC (4-byte length prefixes); convert to Annex B start codes before decoding.
- The avcC packet carries SPS and PPS.

### Latency: SPS rewrite

The iPhone's SPS has `max_num_reorder_frames` unset, so the Media Foundation decoder buffers about five frames.
`sps.rs` rewrites the SPS (sets `num_reorder_frames = 0`), after which output is immediate. Flushing/draining the decoder
does not work as an alternative.

## 6. Audio (UDP)

- RTP, AAC-ELD (ct 8), 480 samples per frame at 44.1 kHz stereo.
- Payload encrypted with AES-128-CBC; key is `key16`, IV is `eiv`.
- Each packet is sent three times (redundancy): dedupe by sequence number.
- Device usually runs at 48 kHz: resample 44.1 → device rate.
