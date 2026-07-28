//! The sky over your head, for the hour the left pane has selected,
//! drawn in braille.
//!
//! A planisphere: zenith at the centre, horizon as the rim, north up and
//! east to the left, which is what you get looking up rather than at a
//! map. Braille cells carry a 2×4 dot matrix and those dots are close to
//! square, so the circle comes out round without an aspect fudge.
//!
//! Everything here is arithmetic over two embedded tables. No network,
//! no image protocol, no temp files, and nothing at all between
//! keypresses: the view blocks on stdin like the rest of the suite.

use crust::style;
use crust::{Crust, Cursor, Input};
use std::sync::OnceLock;

/// Yale Bright Star Catalogue: `ra,dec,mag,bv,name`, brightest first.
const STAR_DATA: &str = include_str!("../data/stars.csv");
/// Constellation stick figures: `ABR:ra,dec ra,dec …`, one polyline each.
const LINE_DATA: &str = include_str!("../data/constellations.csv");

pub struct Star {
    pub ra: f64,
    pub dec: f64,
    pub mag: f64,
    pub bv: f64,
    pub name: &'static str,
}

/// One stroke of a stick figure, as equatorial waypoints.
pub struct Seg {
    pub pts: Vec<(f64, f64)>,
}

/// Parse both tables once, on first use. A chart the user never opens
/// costs nothing but the bytes in the binary.
fn catalog() -> &'static (Vec<Star>, Vec<Seg>) {
    static CAT: OnceLock<(Vec<Star>, Vec<Seg>)> = OnceLock::new();
    CAT.get_or_init(|| {
        let mut stars = Vec::with_capacity(9200);
        for line in STAR_DATA.lines() {
            let mut f = line.splitn(5, ',');
            let (Some(ra), Some(dec), Some(mag), Some(bv), Some(name)) =
                (f.next(), f.next(), f.next(), f.next(), f.next())
            else {
                continue;
            };
            let (Ok(ra), Ok(dec), Ok(mag), Ok(bv)) =
                (ra.parse(), dec.parse(), mag.parse(), bv.parse())
            else {
                continue;
            };
            stars.push(Star { ra, dec, mag, bv, name });
        }
        let mut segs = Vec::with_capacity(160);
        for line in LINE_DATA.lines() {
            let Some((_abr, rest)) = line.split_once(':') else { continue };
            let pts: Vec<(f64, f64)> = rest
                .split(' ')
                .filter_map(|p| {
                    let (a, b) = p.split_once(',')?;
                    Some((a.parse().ok()?, b.parse().ok()?))
                })
                .collect();
            if pts.len() > 1 {
                segs.push(Seg { pts });
            }
        }
        (stars, segs)
    })
}

// ─────────────────────────── the canvas ──────────────────────────────

/// Braille dot bit for a sub-pixel within a cell. Rows 0-2 use bits
/// 0,1,2 / 3,4,5; row 3 uses bits 6,7.
const DOTS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

struct Canvas {
    w: usize,
    h: usize,
    bits: Vec<u8>,
    color: Vec<Option<(u8, u8, u8)>>,
    /// How bright the thing that claimed this cell's colour was, so a
    /// star wins the cell over a constellation line through it.
    weight: Vec<f64>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            bits: vec![0; w * h],
            color: vec![None; w * h],
            weight: vec![f64::NEG_INFINITY; w * h],
        }
    }

    fn set(&mut self, x: i32, y: i32, rgb: (u8, u8, u8), weight: f64) {
        if x < 0 || y < 0 {
            return;
        }
        let (px, py) = (x as usize, y as usize);
        let (cx, cy) = (px / 2, py / 4);
        if cx >= self.w || cy >= self.h {
            return;
        }
        let i = cy * self.w + cx;
        self.bits[i] |= DOTS[py % 4][px % 2];
        if weight > self.weight[i] {
            self.weight[i] = weight;
            self.color[i] = Some(rgb);
        }
    }

    fn disc(&mut self, x: i32, y: i32, r: i32, rgb: (u8, u8, u8), weight: f64) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.set(x + dx, y + dy, rgb, weight);
                }
            }
        }
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: (u8, u8, u8), weight: f64) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, rgb, weight);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// The whole canvas as one printable frame, its top left cell at
    /// (`left`, `top`). Colour is switched only when it changes, so a row
    /// of one-colour stars costs one escape sequence, not one per cell.
    fn frame(&self, left: u16, top: u16) -> String {
        let mut out = String::with_capacity(self.w * self.h * 3);
        for y in 0..self.h {
            out.push_str(&Cursor::at(left, top + y as u16));
            let mut cur: Option<(u8, u8, u8)> = None;
            for x in 0..self.w {
                let i = y * self.w + x;
                let b = self.bits[i];
                if b == 0 {
                    out.push(' ');
                    continue;
                }
                if self.color[i] != cur {
                    if let Some((r, g, bl)) = self.color[i] {
                        out.push_str(&style::set_fg_rgb(r, g, bl));
                        cur = self.color[i];
                    }
                }
                out.push(char::from_u32(0x2800 + b as u32).unwrap_or(' '));
            }
            out.push_str(style::RESET);
        }
        out
    }
}

// ─────────────────────────── the sky maths ───────────────────────────

/// Altitude and azimuth (from north through east) of an equatorial
/// position, for an observer at `lat` when the local sidereal time is
/// `lst` degrees.
fn altaz(ra: f64, dec: f64, lst: f64, lat: f64) -> (f64, f64) {
    let ha = (lst - ra).to_radians();
    let (d, p) = (dec.to_radians(), lat.to_radians());
    let (sin_ha, cos_ha) = ha.sin_cos();
    let (sin_d, cos_d) = d.sin_cos();
    let (sin_p, cos_p) = p.sin_cos();
    let alt = (sin_d * sin_p + cos_d * cos_p * cos_ha).clamp(-1.0, 1.0).asin();
    let az = (-cos_d * sin_ha).atan2(sin_d * cos_p - cos_d * sin_p * cos_ha);
    (alt.to_degrees(), (az.to_degrees() + 360.0) % 360.0)
}

/// Effective temperature from the B−V colour index (Ballesteros 2012).
fn teff_from_bv(bv: f64) -> f64 {
    4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62))
}

/// The colour a star of this temperature shows. Same stops as the
/// `stars` app uses for its diagram, so a star looks like itself in both.
fn teff_rgb(teff: f64) -> (u8, u8, u8) {
    const STOPS: [(f64, (u8, u8, u8)); 7] = [
        (40000.0, (120, 150, 255)),
        (20000.0, (160, 195, 255)),
        (9700.0, (225, 235, 255)),
        (7200.0, (255, 245, 200)),
        (5800.0, (255, 215, 90)),
        (4400.0, (255, 150, 60)),
        (3000.0, (255, 90, 60)),
    ];
    let t = teff.clamp(3000.0, 40000.0);
    for w in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t0 && t >= t1 {
            let f = (t0.ln() - t.ln()) / (t0.ln() - t1.ln());
            let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * f) as u8;
            return (mix(c0.0, c1.0), mix(c0.1, c1.1), mix(c0.2, c1.2));
        }
    }
    STOPS[0].1
}

/// Dim a colour toward the background by how faint the star is. A 5th
/// magnitude star and a 1st magnitude star both get one dot, so the only
/// brightness left to work with is the colour itself.
fn faded(rgb: (u8, u8, u8), mag: f64) -> (u8, u8, u8) {
    let f = (1.15 - (mag + 1.5) * 0.115).clamp(0.32, 1.0);
    (
        (rgb.0 as f64 * f) as u8,
        (rgb.1 as f64 * f) as u8,
        (rgb.2 as f64 * f) as u8,
    )
}

fn body_rgb(name: &str) -> (u8, u8, u8) {
    match name {
        "sun" => (255, 225, 120),
        "moon" => (235, 235, 225),
        "mercury" => (190, 180, 170),
        "venus" => (255, 245, 210),
        "mars" => (235, 110, 70),
        "jupiter" => (240, 205, 150),
        "saturn" => (230, 205, 140),
        "uranus" => (150, 220, 230),
        "neptune" => (130, 160, 245),
        _ => (200, 200, 200),
    }
}

// ─────────────────────────── the view ────────────────────────────────

/// What the chart shows, kept across redraws while the view is open.
pub struct Opts {
    pub lines: bool,
    pub names: bool,
    pub mag: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Self { lines: true, names: true, mag: 5.5 }
    }
}

impl Opts {
    /// Start at the faintest star the sky you configured actually shows.
    /// Bortle 1 is a desert sky at about magnitude 7.8; Bortle 9 is an
    /// inner city, where four is a good night. The catalogue stops at
    /// 6.5, which is roughly the naked-eye limit anyway.
    pub fn for_bortle(bortle: f64) -> Self {
        let mag = (7.8 - 0.475 * (bortle.clamp(1.0, 9.0) - 1.0)).clamp(3.5, 6.5);
        Self { mag, ..Self::default() }
    }
}

/// One hour of the app's left pane: the moment a chart is drawn for.
#[derive(Clone, Copy)]
pub struct Moment {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
}

/// Draw the sky for `at`, then own the keyboard until the user leaves.
///
/// Left and right walk the hours the app already has, so stepping
/// through the night in the chart moves the selection in the left pane
/// too. Returns the index the user ended on.
pub fn run(
    moments: &[Moment],
    mut index: usize,
    lat: f64,
    lon: f64,
    tz: f64,
    place: &str,
    opts: &mut Opts,
) -> usize {
    if moments.is_empty() {
        return index;
    }
    loop {
        let (cols, rows) = Crust::terminal_size();
        Crust::clear_screen();
        print!("{}", draw(moments[index], lat, lon, tz, place, opts, cols, rows));
        use std::io::Write;
        std::io::stdout().flush().ok();

        let Some(key) = Input::getchr(None) else { continue };
        match key.as_str() {
            "q" | "Q" | "ESC" | "s" => return index,
            "RIGHT" | "l" => index = (index + 1).min(moments.len() - 1),
            "LEFT" | "h" => index = index.saturating_sub(1),
            "DOWN" | "j" | "PgDOWN" => index = (index + 24).min(moments.len() - 1),
            "UP" | "k" | "PgUP" => index = index.saturating_sub(24),
            "c" => opts.lines = !opts.lines,
            "n" => opts.names = !opts.names,
            "+" | "=" => opts.mag = (opts.mag + 0.5).min(6.5),
            "-" | "_" => opts.mag = (opts.mag - 0.5).max(1.0),
            _ => {}
        }
    }
}

/// The chart alone, inside the rectangle at (`x`, `y`) of `w`×`h` cells.
///
/// It takes a rectangle rather than the terminal because it is drawn at
/// two sizes: the whole screen under `s`, and the lower half of the main
/// pane on the front screen.
pub fn panel(
    at: Moment,
    lat: f64,
    lon: f64,
    tz: f64,
    opts: &Opts,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> String {
    let (stars, segs) = catalog();
    let (lst, bodies) = orbit::sky_at(at.year, at.month, at.day, at.hour as f64, lat, lon, tz);

    let ch = h.max(4) as usize;
    let cw = w.max(20) as usize;
    // Names need room. In a pane a third of the screen high they would
    // be more text than sky, so they wait for the full-screen view.
    let names = opts.names && w >= 60 && h >= 16;
    // A dot at (px, py) lands at column x + px/2, row y + py/4.
    let (x_off, y_off) = (x, y);
    let mut canvas = Canvas::new(cw, ch);
    let (dw, dh) = (cw as f64 * 2.0, ch as f64 * 4.0);
    let (cx, cy) = (dw / 2.0, dh / 2.0);
    let r = (dw.min(dh) / 2.0) - 2.0;
    // How faint this much room can take. Star counts run about half a
    // magnitude per factor of ten (15 stars brighter than 1st, 4,800
    // brighter than 6th), so invert that for a tenth of the dots filled.
    // Without it the pane-sized chart is a solid block of braille.
    let dots = std::f64::consts::PI * r * r;
    let fits = ((2.0 * 0.10 * dots).log10() - 0.68) / 0.5;
    let mag_limit = opts.mag.min(fits).max(1.0);

    // Zenith at the centre, horizon at the rim, north up, east left:
    // the sky as you see it looking up, not as a map sees it.
    let project = |alt: f64, az: f64| -> (i32, i32) {
        let rr = (90.0 - alt) / 90.0 * r;
        let a = az.to_radians();
        ((cx - rr * a.sin()) as i32, (cy - rr * a.cos()) as i32)
    };

    // Horizon.
    let horizon = (70, 70, 85);
    for i in 0..1440 {
        let a = (i as f64) * std::f64::consts::TAU / 1440.0;
        canvas.set((cx + r * a.sin()) as i32, (cy + r * a.cos()) as i32, horizon, -1.0);
    }

    // Stick figures under everything else.
    if opts.lines {
        // Fainter when the chart is small: with few stars plotted, full
        // strength figures read as a web with the sky behind it.
        let ink = if r < 40.0 { (46, 60, 88) } else { (60, 78, 110) };
        for seg in segs {
            for pair in seg.pts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let (alt_a, az_a) = altaz(a.0, a.1, lst, lat);
                let (alt_b, az_b) = altaz(b.0, b.1, lst, lat);
                if alt_a < 0.0 || alt_b < 0.0 {
                    continue;
                }
                let (x0, y0) = project(alt_a, az_a);
                let (x1, y1) = project(alt_b, az_b);
                canvas.line(x0, y0, x1, y1, ink, -0.5);
            }
        }
    }

    // Stars. The catalogue is sorted brightest first, so the loop can
    // stop as soon as it passes the magnitude limit.
    let mut star_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for s in stars {
        if s.mag > mag_limit {
            break;
        }
        let (alt, az) = altaz(s.ra, s.dec, lst, lat);
        if alt < 0.0 {
            continue;
        }
        let (px, py) = project(alt, az);
        let rgb = faded(teff_rgb(teff_from_bv(s.bv)), s.mag);
        if s.mag < 1.0 {
            canvas.disc(px, py, 1, rgb, 10.0 - s.mag);
        } else {
            canvas.set(px, py, rgb, 10.0 - s.mag);
        }
        if names && !s.name.is_empty() && s.mag < 1.8 {
            let (col, row) = (x_off + px as u16 / 2 + 2, y_off + py as u16 / 4);
            if (col + s.name.len() as u16) < x_off + w {
                star_labels.push((col, row, s.name.to_string(), (150, 150, 165)));
            }
        }
    }

    // Sun, moon and planets on top, each with its name beside it.
    let mut body_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for (name, ra, dec) in &bodies {
        let (alt, az) = altaz(*ra, *dec, lst, lat);
        if alt < 0.0 {
            continue;
        }
        let (px, py) = project(alt, az);
        let rgb = body_rgb(name);
        let size = if *name == "sun" || *name == "moon" { 2 } else { 1 };
        canvas.disc(px, py, size, rgb, 100.0);
        let (col, row) = (x_off + px as u16 / 2 + 2, y_off + py as u16 / 4);
        if (col + name.len() as u16) < x_off + w {
            body_labels.push((col, row, (*name).to_string(), rgb));
        }
    }

    // Place the labels, planets first: two names on one patch of sky
    // print over each other, and "jupiterollux" is worse than no Pollux.
    let mut taken: Vec<(u16, u16, u16)> = Vec::new();
    let mut labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for (col, row, text, rgb) in body_labels.into_iter().chain(star_labels) {
        let end = col + text.chars().count() as u16;
        if taken.iter().any(|&(r, c0, c1)| r == row && col <= c1 && c0 <= end) {
            continue;
        }
        taken.push((row, col, end));
        labels.push((col, row, text, rgb));
    }

    let mut out = canvas.frame(x, y);

    // Cardinal points sit on the rim, over whatever is under them.
    for (label, az) in [("N", 0.0), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
        let (px, py) = project(0.0, az);
        let col = (x_off + px as u16 / 2).clamp(x, x + w - 1);
        let row = (y_off + py as u16 / 4).clamp(y, y + h - 1);
        out.push_str(&Cursor::at(col, row));
        out.push_str(&style::rgb(label, Some((255, 190, 90)), None, "b"));
    }

    for (col, row, text, rgb) in labels {
        if row >= y + h {
            continue;
        }
        out.push_str(&Cursor::at(col, row));
        out.push_str(&style::rgb(&text, Some(rgb), None, ""));
    }
    out
}

/// The whole screen for one moment: title row, chart, key line.
fn draw(
    at: Moment,
    lat: f64,
    lon: f64,
    tz: f64,
    place: &str,
    opts: &Opts,
    cols: u16,
    rows: u16,
) -> String {
    let mut out = panel(at, lat, lon, tz, opts, 1, 2, cols, rows.saturating_sub(2));
    let (lst, bodies) = orbit::sky_at(at.year, at.month, at.day, at.hour as f64, lat, lon, tz);

    // Title and keys.
    let up = |name: &str| {
        bodies
            .iter()
            .find(|b| b.0 == name)
            .map(|&(_, ra, dec)| altaz(ra, dec, lst, lat).0 > 0.0)
            .unwrap_or(false)
    };
    let planets = bodies
        .iter()
        .filter(|(n, ra, dec)| {
            *n != "sun" && *n != "moon" && altaz(*ra, *dec, lst, lat).0 > 0.0
        })
        .count();
    let sky_state = match (up("sun"), up("moon")) {
        (true, _) => "sun up".to_string(),
        (false, true) => format!("moon {}% up", orbit::moon_phase_pct(at.year, at.month, at.day)),
        (false, false) => "dark".to_string(),
    };
    let title = format!(
        " {}  {:04}-{:02}-{:02} {:02}:00  {}  ·  {} · {} planets up · stars to mag {:.1}",
        place, at.year, at.month, at.day, at.hour,
        if lat >= 0.0 { format!("{lat:.1}°N") } else { format!("{:.1}°S", -lat) },
        sky_state, planets, opts.mag,
    );
    out.push_str(&Cursor::at(1, 1));
    out.push_str(&style::rgb(
        &crust::truncate_ansi(&title, cols as usize),
        Some((255, 200, 120)),
        None,
        "b",
    ));
    out.push_str(&Cursor::at(1, rows));
    out.push_str(&style::dim(&crust::truncate_ansi(
        " ←/→ hour · ↑/↓ day · c lines · n names · +/- fainter, brighter · q back",
        cols as usize,
    )));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both tables parse, and the brightest entry really is Sirius.
    #[test]
    fn catalog_parses() {
        let (stars, segs) = catalog();
        assert!(stars.len() > 9000, "only {} stars", stars.len());
        assert_eq!(stars[0].name, "Sirius");
        assert!((stars[0].mag + 1.46).abs() < 0.01);
        assert!(segs.len() > 100, "only {} stick figures", segs.len());
    }

    /// The pole star sits at altitude ≈ latitude, due north, whatever
    /// the hour. That is the one check that catches a sign error in the
    /// hour angle or the azimuth quadrant.
    #[test]
    fn polaris_sits_at_the_pole() {
        for lst in [0.0, 90.0, 187.5, 300.0] {
            let (alt, az) = altaz(37.95, 89.26, lst, 59.9);
            assert!((alt - 59.9).abs() < 1.0, "alt {alt} at lst {lst}");
            assert!(az < 3.0 || az > 357.0, "az {az} at lst {lst}");
        }
    }

    /// A star on the meridian at the observer's latitude is overhead,
    /// and one 90° away in hour angle is on the horizon.
    #[test]
    fn zenith_and_horizon() {
        let (alt, _) = altaz(100.0, 59.9, 100.0, 59.9);
        assert!(alt > 89.0, "meridian star at own latitude: alt {alt}");
        let (alt, az) = altaz(0.0, 0.0, 90.0, 0.0);
        assert!(alt.abs() < 1.0, "alt {alt}");
        assert!((az - 270.0).abs() < 1.0, "setting star should be west, az {az}");
    }

    /// A city sky shows less than a desert one.
    #[test]
    fn bortle_sets_the_limit() {
        assert!(Opts::for_bortle(1.0).mag > Opts::for_bortle(9.0).mag);
        assert!((Opts::for_bortle(4.0).mag - 6.4).abs() < 0.1);
        assert!(Opts::for_bortle(9.0).mag >= 3.5);
    }

    /// A full frame draws, carries its cardinal points, and is quick
    /// enough to redraw on every keypress.
    #[test]
    fn draws_a_frame_fast() {
        let at = Moment { year: 2026, month: 7, day: 28, hour: 23 };
        let opts = Opts::default();
        let t = std::time::Instant::now();
        let mut frame = String::new();
        for _ in 0..20 {
            frame = draw(at, 59.9, 10.7, 2.0, "Oslo", &opts, 150, 42);
        }
        let per = t.elapsed() / 20;
        assert!(frame.contains('N') && frame.contains('S'), "no cardinal points");
        assert!(frame.contains('\u{2800}') || frame.contains('\u{2801}'), "no braille");
        assert!(per.as_millis() < 20, "one frame took {per:?}");
    }
}
