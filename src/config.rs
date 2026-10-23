// Central constants: this way all modules see the same "device identity".
pub const NAME: &str = "AirBridge";
pub const MODEL: &str = "AppleTV3,2";
pub const SRCVERS: &str = "220.68";
pub const FEATURES: u64 = 0x5A7FFEE6;
pub const FEATURES_TXT: &str = "0x5A7FFEE6,0x0"; // the same, as text for mDNS
// Audio formats we offer. Bits 2..21 = PCM and ALAC; we leave out AAC-LC/ELD (bits 22+),
// because the music path only supports ALAC. The iPhone then falls back to ALAC/PCM.
pub const AUDIO_FORMATS: u64 = 0x3F_FFFC;
pub const AUDIO_CN: &str = "0,1"; // RAOP compression types in discovery: 0 = PCM, 1 = ALAC
pub const RTSP_PORT: u16 = 7000;
pub const MIRROR_PORT: u16 = 7100;
pub const KEY_FILE: &str = "airbridge_key.bin";
