// Zentrale Konstanten: so sehen alle Module dieselbe "Geräte-Identität".
pub const NAME: &str = "AirBridge";
pub const MODEL: &str = "AppleTV3,2";
pub const SRCVERS: &str = "220.68";
pub const FEATURES: u64 = 0x5A7FFEE6;
pub const FEATURES_TXT: &str = "0x5A7FFEE6,0x0"; // dasselbe, als Text für mDNS
// Audioformate, die wir anbieten. Bits 2..21 = PCM und ALAC; AAC-LC/ELD (Bits 22+) lassen wir weg,
// weil Windows AAC-ELD nicht dekodieren kann. Dann weicht das iPhone auf ALAC/PCM aus.
pub const AUDIO_FORMATS: u64 = 0x3F_FFFC;
pub const AUDIO_CN: &str = "0,1"; // RAOP-Kompressionstypen im Discovery: 0 = PCM, 1 = ALAC
pub const RTSP_PORT: u16 = 7000;
pub const MIRROR_PORT: u16 = 7100;
pub const KEY_FILE: &str = "airbridge_key.bin";
