//! Optical formulas ported verbatim from Ruby telescope.
//! All dimensions in millimeters, angles in degrees.

use std::f64::consts::PI;

fn to_rad(deg: f64) -> f64 { deg * PI / 180.0 }
fn to_deg(rad: f64) -> f64 { rad * 180.0 / PI }

/// Telescope focal ratio (f/#).
pub fn tfr(app: f64, tfl: f64) -> f64 {
    if app == 0.0 { 0.0 } else { tfl / app }
}

/// Magnitude limit (dimmest star visible) under dark skies.
pub fn mlim(app: f64) -> f64 {
    if app <= 0.0 { 0.0 } else { 5.0 * (app / 10.0).log10() + 7.5 }
}

/// Magnitude limit adjusted for the observer's Bortle scale.
/// Bortle 1–3 is roughly the textbook dark-sky condition (no penalty).
/// Bortle 4 onward each costs ~0.4 mag of limiting magnitude until urban
/// skies (Bortle 8–9) wipe out roughly two magnitudes' worth of dim
/// targets compared to a dark site.
pub fn mlim_bortle(app: f64, bortle: f64) -> f64 {
    let dark = mlim(app);
    if dark <= 0.0 { return 0.0; }
    let b = bortle.clamp(1.0, 9.0);
    let penalty = (b - 3.0).max(0.0) * 0.4;
    (dark - penalty).max(0.0)
}

/// Times-eye light gathering.
pub fn xeye(app: f64) -> f64 {
    app * app / 49.0
}

/// Minimum usable magnification.
pub fn minx(app: f64, tfl: f64) -> f64 {
    let r = tfr(app, tfl);
    if r == 0.0 { 0.0 } else { tfl / (7.0 * r) }
}

/// Eyepiece FL for minimum magnification.
pub fn mine(app: f64, tfl: f64) -> f64 { 7.0 * tfr(app, tfl) }

/// Maximum usable magnification.
pub fn maxx(app: f64) -> f64 { 2.0 * app }

/// Eyepiece FL for maximum magnification.
pub fn maxe(app: f64, tfl: f64) -> f64 {
    let m = maxx(app);
    if m == 0.0 { 0.0 } else { tfl / m }
}

/// Rayleigh separation (arcseconds).
pub fn sepr(app: f64) -> f64 {
    if app == 0.0 { 0.0 } else { 3600.0 * to_deg((671e-6 / app).asin()) }
}

/// Dawes separation (arcseconds).
pub fn sepd(app: f64) -> f64 {
    if app == 0.0 { 0.0 } else { 115.824 / app }
}

// Recommended eyepiece focal lengths:
pub fn e_st(app: f64, tfl: f64) -> f64 { if app == 0.0 { 0.0 } else { 6.4 * tfl / app } } // star fields
pub fn e_gx(app: f64, tfl: f64) -> f64 { if app == 0.0 { 0.0 } else { 3.6 * tfl / app } } // galaxies/nebulae
pub fn e_pl(app: f64, tfl: f64) -> f64 { if app == 0.0 { 0.0 } else { 2.1 * tfl / app } } // planets
pub fn e_2s(app: f64, tfl: f64) -> f64 { if app == 0.0 { 0.0 } else { 1.3 * tfl / app } } // doubles
pub fn e_t2(app: f64, tfl: f64) -> f64 { if app == 0.0 { 0.0 } else { 0.7 * tfl / app } } // tight doubles

/// Smallest visible Moon detail (km).
pub fn moon(tfl: f64) -> f64 {
    if tfl == 0.0 { 0.0 } else {
        384e6 * (to_rad(115.824 / tfl) / 360.0).tan()
    }
}

/// Smallest visible Sun detail (km).
pub fn sun(tfl: f64) -> f64 { moon(tfl) / 2.5668 }

/// Magnification (scope + eyepiece).
pub fn magx(tfl: f64, epfl: f64) -> f64 {
    if epfl == 0.0 { 0.0 } else { tfl / epfl }
}

/// True field of view (degrees).
pub fn tfov(tfl: f64, epfl: f64, afov: f64) -> f64 {
    let m = magx(tfl, epfl);
    if m == 0.0 { 0.0 } else { afov / m }
}

/// Exit pupil (mm).
pub fn pupl(app: f64, tfl: f64, epfl: f64) -> f64 {
    let m = magx(tfl, epfl);
    if m == 0.0 { 0.0 } else { app / m }
}

/// Per-eyepiece target-class suitability for one scope. Returns
/// `[*FLD, GLXY, PLNT, DBL*, >2*<]` booleans, mutually exclusive
/// across the exit-pupil ladder borrowed from Ruby telescope:
///   *FLD : exit pupil > 6     mm  (rich star fields)
///   GLXY : exit pupil 3-6     mm  (galaxies, nebulae)
///   PLNT : exit pupil 1.5-3   mm  (planets)
///   DBL* : exit pupil 1-1.5   mm  (doubles)
///   >2*< : exit pupil < 1     mm  (tight doubles, splitting)
pub fn ep_suitability(app: f64, tfl: f64, epfl: f64) -> [bool; 5] {
    if app <= 0.0 || epfl <= 0.0 || tfl <= 0.0 { return [false; 5]; }
    let p = pupl(app, tfl, epfl);
    [
        p > 6.0,
        p > 3.0  && p <= 6.0,
        p > 1.5  && p <= 3.0,
        p >= 1.0 && p <= 1.5,
        p < 1.0,
    ]
}
