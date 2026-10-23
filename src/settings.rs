// Einstellungen, die der Nutzer in der Oberfläche ändern kann. Sie liegen als `settings.json` im selben Ordner wie
// der Schlüssel (siehe `identity::key_path`), also bei der portablen Version neben der .exe.
use crate::config;
use crate::identity;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Ein Gerät, dem der Nutzer dauerhaft vertraut ("Immer erlauben").
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Trusted {
    pub id: String,
    pub name: String,
}

// `#[serde(default)]`: fehlt ein Feld in der Datei (z. B. nach einem Update), gilt der Standardwert.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Name, unter dem AirBridge auf dem iPhone erscheint.
    pub name: String,
    /// Mit Windows starten (startet versteckt im Tray).
    pub autostart: bool,
    /// Beim Schließen des Fensters im Tray weiterlaufen statt zu beenden.
    pub minimize_to_tray: bool,
    /// Fenster immer im Vordergrund.
    pub always_on_top: bool,
    /// Sobald ein iPhone spiegelt, automatisch in den Vollbildmodus wechseln.
    pub start_fullscreen: bool,
    /// Vor jeder neuen Verbindung nachfragen.
    pub require_confirm: bool,
    pub trusted: Vec<Trusted>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: config::NAME.to_string(),
            autostart: false,
            minimize_to_tray: false,
            always_on_top: false,
            start_fullscreen: false,
            require_confirm: false,
            trusted: Vec::new(),
        }
    }
}

impl Settings {
    /// Macht aus der Eingabe einen brauchbaren Gerätenamen (keine Steuerzeichen, höchstens 40 Zeichen, nie leer).
    pub fn clean_name(raw: &str) -> String {
        let name: String = raw
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(40)
            .collect();
        if name.is_empty() {
            config::NAME.to_string()
        } else {
            name
        }
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        self.trusted.iter().any(|t| t.id == id)
    }
}

pub fn path() -> PathBuf {
    identity::key_path().with_file_name("settings.json")
}

/// Lädt die Einstellungen; fehlt die Datei oder ist sie kaputt, gelten die Standardwerte.
pub fn load() -> Settings {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .map(|mut s| {
            s.name = Settings::clean_name(&s.name);
            s
        })
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let s = Settings {
            name: "Wohnzimmer".into(),
            require_confirm: true,
            trusted: vec![Trusted {
                id: "AA:BB".into(),
                name: "iPhone".into(),
            }],
            ..Default::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"name":"X","autostart":true}"#).unwrap();
        assert_eq!(s.name, "X");
        assert!(s.autostart);
        assert!(!s.require_confirm);
        assert!(s.trusted.is_empty());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(serde_json::from_str::<Settings>("kein json").is_err());
    }

    #[test]
    fn name_is_cleaned() {
        assert_eq!(Settings::clean_name("  Büro\n"), "Büro");
        assert_eq!(Settings::clean_name("   "), config::NAME);
        assert_eq!(Settings::clean_name(&"x".repeat(100)).chars().count(), 40);
    }

    #[test]
    fn trusted_lookup() {
        let mut s = Settings::default();
        assert!(!s.is_trusted("AA"));
        s.trusted.push(Trusted {
            id: "AA".into(),
            name: "x".into(),
        });
        assert!(s.is_trusted("AA"));
    }
}
