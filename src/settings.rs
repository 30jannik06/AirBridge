// Settings the user can change in the interface. They are stored as `settings.json` in the same folder as
// the key (see `identity::key_path`), i.e. next to the exe in the portable version.
use crate::config;
use crate::identity;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A device the user trusts permanently ("Always allow").
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Trusted {
    pub id: String,
    pub name: String,
}

// `#[serde(default)]`: if a field is missing in the file (e.g. after an update), the default value applies.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Name under which AirBridge appears on the iPhone.
    pub name: String,
    /// Start with Windows (starts hidden in the tray).
    pub autostart: bool,
    /// Keep running in the tray when the window is closed instead of quitting.
    pub minimize_to_tray: bool,
    /// Keep the window on top.
    pub always_on_top: bool,
    /// Switch to fullscreen automatically as soon as an iPhone mirrors.
    pub start_fullscreen: bool,
    /// Ask before every new connection.
    pub require_confirm: bool,
    /// Reject unknown devices silently (no dialog); only trusted devices may mirror.
    pub trusted_only: bool,
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
            trusted_only: false,
            trusted: Vec::new(),
        }
    }
}

impl Settings {
    /// Turns the input into a usable device name (no control characters, at most 40 characters, never empty).
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

/// Loads the settings; if the file is missing or broken, the defaults apply.
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
        assert!(!s.trusted_only);
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
