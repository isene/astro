use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn astro_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".astro")
}

pub fn config_path() -> PathBuf { astro_dir().join("config.yml") }

pub fn ensure_dir() {
    let _ = std::fs::create_dir_all(astro_dir());
    // One-shot migration from ~/.nova or ~/.scope on first launch.
    // Leaves the source directories alone (user can clean up manually).
    let cfg = config_path();
    if !cfg.exists() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let nova_cfg = PathBuf::from(&home).join(".nova/config.yml");
        if nova_cfg.exists() {
            let _ = std::fs::copy(&nova_cfg, &cfg);
        }
    }
    // Bring scope's catalog over too if it exists and astro's doesn't.
    let scope_db = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
        .join(".scope/data.json");
    let astro_gear = astro_dir().join("gear.json");
    if !astro_gear.exists() && scope_db.exists() {
        let _ = std::fs::copy(&scope_db, &astro_gear);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub location: String,
    #[serde(default = "default_tz_name")]
    pub tz_name: String,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
    #[serde(default = "default_cloud")]
    pub cloud_limit: i64,
    #[serde(default = "default_humidity")]
    pub humidity_limit: f64,
    #[serde(default = "default_temp")]
    pub temp_limit: f64,
    #[serde(default = "default_wind")]
    pub wind_limit: f64,
    #[serde(default = "default_bortle")]
    pub bortle: f64,
    #[serde(default = "default_true")]
    pub show_planets: bool,
    #[serde(default = "default_true")]
    pub show_events: bool,
    /// Pictures open as the eyepiece shows them (the `r` toggle).
    #[serde(default)]
    pub eyepiece_view: bool,
    /// Wi-Fi network name → "Place lat lon". Each hour the wallpaper
    /// looks up the network it is on, and a match becomes the location.
    #[serde(default)]
    pub wifi: std::collections::BTreeMap<String, String>,
}

fn default_cloud() -> i64 { 40 }
fn default_humidity() -> f64 { 80.0 }
fn default_temp() -> f64 { -10.0 }
fn default_wind() -> f64 { 8.0 }
fn default_bortle() -> f64 { 4.0 }
fn default_tz_name() -> String { "Europe/Oslo".into() }
fn default_true() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            location: "Oslo".into(),
            tz_name: default_tz_name(),
            lat: 59.91,
            lon: 10.75,
            tz: 1.0,
            cloud_limit: default_cloud(),
            humidity_limit: default_humidity(),
            temp_limit: default_temp(),
            wind_limit: default_wind(),
            bortle: default_bortle(),
            show_planets: true,
            show_events: true,
            eyepiece_view: false,
            wifi: Default::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            serde_yaml::from_str(&data).unwrap_or_default()
        } else {
            let cfg = Config::default();
            cfg.save();
            cfg
        }
    }

    /// Move to the place the Wi-Fi network names, and save it, so an
    /// unknown network keeps the last place. Nothing without a `wifi` list.
    pub fn follow_wifi(&mut self) {
        if self.wifi.is_empty() { return; }
        let Some((name, lat, lon)) = ssid().and_then(|s| self.wifi.get(&s)).and_then(|p| place(p)) else { return };
        if (name.as_str(), lat, lon) != (self.location.as_str(), self.lat, self.lon) {
            (self.location, self.lat, self.lon) = (name, lat, lon);
            self.save();
        }
    }

    pub fn save(&self) {
        ensure_dir();
        if let Ok(yaml) = serde_yaml::to_string(self) {
            let _ = std::fs::write(config_path(), yaml);
        }
    }
}

/// "Tromsø 69.65 18.96" → its name and place; the name may have spaces.
fn place(s: &str) -> Option<(String, f64, f64)> {
    let mut w = s.trim().rsplitn(3, ' ');
    let (lon, lat, name) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?, w.next()?.trim());
    (!name.is_empty()).then(|| (name.to_string(), lat, lon))
}

/// The Wi-Fi network the laptop is on: the wireless interface from /sys,
/// then one `iw <it> link`, which reads the connection and does not scan.
fn ssid() -> Option<String> {
    let iface = std::fs::read_dir("/sys/class/net").ok()?.flatten()
        .find(|e| e.path().join("wireless").exists())?.file_name();
    let out = std::process::Command::new("iw").arg("dev").arg(&iface).arg("link").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines()
        .find_map(|l| l.trim().strip_prefix("SSID: ").map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::place;

    #[test]
    fn a_wifi_place_reads_as_name_and_position() {
        assert_eq!(place("Tromsø 69.65 18.96"), Some(("Tromsø".into(), 69.65, 18.96)));
        assert_eq!(place("Le Havre 49.49 0.11"), Some(("Le Havre".into(), 49.49, 0.11)));
        assert_eq!(place("Oslo"), None);
        assert_eq!(place("69.65 18.96"), None);
    }
}
