//! Gear mode — telescope + eyepiece catalog and observation
//! calculator. Inherited from the standalone `scope` app, embedded
//! here as a sub-mode reachable via the `g` key in Sky mode.

pub mod data;
pub mod optics;
mod ui;

pub use ui::run;
