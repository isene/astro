use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn astro_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".astro")
}

pub fn save_path() -> PathBuf { astro_dir().join("gear.json") }
pub fn config_path() -> PathBuf { astro_dir().join("gear_config.json") }
pub fn backup_dir() -> PathBuf { astro_dir().join("gear_backups") }

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Telescope {
    pub name: String,
    pub app: f64,       // aperture (mm)
    pub tfl: f64,       // focal length (mm)
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Eyepiece {
    pub name: String,
    pub fl: f64,        // focal length (mm)
    pub afov: f64,      // apparent FOV (degrees)
    #[serde(default)]
    pub notes: String,
}

/// Catch-all for the gear that isn't a scope or an eyepiece:
/// barlow lenses, focal reducers, filters, diagonals, finders, etc.
/// `kind` is a free-text tag the user picks ("barlow", "filter",
/// "diagonal", …); `factor` carries the magnification multiplier
/// for a barlow / reducer (0.0 when not applicable).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MiscEquipment {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub factor: f64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Store {
    #[serde(default)]
    pub telescopes: Vec<Telescope>,
    #[serde(default)]
    pub eyepieces: Vec<Eyepiece>,
    #[serde(default)]
    pub misc: Vec<MiscEquipment>,
}

impl Store {
    pub fn load() -> Self {
        // Native JSON first, fall back to Ruby YAML marshal file if present.
        if let Ok(data) = std::fs::read_to_string(save_path()) {
            if let Ok(s) = serde_json::from_str::<Self>(&data) {
                return s;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            std::fs::write(save_path(), s)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "default_ts_bg")] pub ts_header_bg: String,
    #[serde(default = "default_ep_bg")] pub ep_header_bg: String,
    #[serde(default = "default_tag")] pub tag_color: u16,
    #[serde(default = "default_cursor_bg")] pub cursor_bg: u16,
    #[serde(default = "default_text")] pub text_color: u16,
    #[serde(default = "default_check_good")] pub check_good: u16,
    #[serde(default = "default_check_bad")] pub check_bad: u16,
    #[serde(default = "default_true")] pub auto_backup: bool,
    #[serde(default = "default_backup_count")] pub backup_count: usize,
}

fn default_ts_bg() -> String { "00524b".into() }
fn default_ep_bg() -> String { "4c3c1d".into() }
fn default_tag() -> u16 { 46 }
fn default_cursor_bg() -> u16 { 234 }
fn default_text() -> u16 { 248 }
fn default_check_good() -> u16 { 112 }
fn default_check_bad() -> u16 { 208 }
fn default_true() -> bool { true }
fn default_backup_count() -> usize { 5 }

impl Default for Config {
    fn default() -> Self {
        Self {
            ts_header_bg: default_ts_bg(),
            ep_header_bg: default_ep_bg(),
            tag_color: default_tag(),
            cursor_bg: default_cursor_bg(),
            text_color: default_text(),
            check_good: default_check_good(),
            check_bad: default_check_bad(),
            auto_backup: default_true(),
            backup_count: default_backup_count(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(config_path()) {
            if let Ok(c) = serde_yaml::from_str(&data) {
                return c;
            }
        }
        Self::default()
    }
}

/// Rotate daily backups, keeping the most recent N.
pub fn backup(store: &Store, count: usize) {
    let _ = std::fs::create_dir_all(backup_dir());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let path = backup_dir().join(format!("astro_gear_{}.json", ts));
    if let Ok(s) = serde_json::to_string(store) {
        let _ = std::fs::write(&path, s);
    }
    // Trim old backups.
    if let Ok(entries) = std::fs::read_dir(backup_dir()) {
        let mut files: Vec<_> = entries.flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("astro_gear_"))
            .collect();
        files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        if files.len() > count {
            for e in files.iter().take(files.len() - count) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
