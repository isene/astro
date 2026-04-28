//! Gear mode — telescope + eyepiece catalog and observation
//! calculator. Inherited from the standalone `scope` app, embedded
//! here as a sub-mode reachable via the `g` key in Sky mode.

pub mod data;
pub mod optics;
mod ui;

pub use ui::run;

/// Snapshot of Sky-mode state passed into Gear at mode-switch time.
/// Used to: (a) adjust magnitude-limit display for the user's Bortle,
/// (b) auto-fill observation logs with date / weather / phase / visible
/// bodies. None when Gear is reached without going through Sky.
#[derive(Clone, Default, Debug)]
pub struct SkyEnv {
    pub date: String,
    pub hour_str: String,
    pub location: String,
    pub bortle: f64,
    /// "First Quarter 84%"
    pub moon_summary: String,
    /// "8.3°C  clouds 80%  wind 1.8 m/s SE  hum 48%"
    pub weather: String,
    /// "Mercury, Venus, Mars, Jupiter, Saturn"
    pub visible_bodies: String,
}
