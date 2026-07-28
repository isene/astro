//! The sky over your head for the hour the left pane has selected.
//!
//! The sky itself, the catalogue and the drawing all live in
//! [`starmap`](https://github.com/isene/starmap). What is left here is
//! astro's half: which moment to draw, where the sun, moon and planets
//! are in it, and the keys that walk the night.

use crust::style;
use crust::{Crust, Cursor, Input};
use starmap::{Body, Projection, View};

/// What the chart shows, kept across redraws while the view is open.
pub struct Opts {
    pub inner: starmap::Opts,
}

impl Opts {
    /// Start at the faintest star the configured Bortle sky shows.
    pub fn for_bortle(bortle: f64) -> Self {
        Self { inner: starmap::Opts::for_bortle(bortle) }
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

/// Where the sun, moon and planets are at `at`, as things to draw, and
/// the local sidereal time that places everything else.
fn sky_at(at: Moment, lat: f64, lon: f64, tz: f64) -> (f64, Vec<Body>) {
    let (lst, bodies) = orbit::sky_at(at.year, at.month, at.day, at.hour as f64, lat, lon, tz);
    let drawn = bodies
        .into_iter()
        .map(|(name, ra, dec)| Body {
            rgb: body_rgb(name),
            size: if name == "sun" || name == "moon" { 2 } else { 1 },
            name: name.to_string(),
            ra,
            dec,
        })
        .collect();
    (lst, drawn)
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

fn view_at(at: Moment, lat: f64, lon: f64, tz: f64) -> (View, Vec<Body>) {
    let (lst, bodies) = sky_at(at, lat, lon, tz);
    (View::new(Projection::Horizon { lst_deg: lst, lat_deg: lat }), bodies)
}

/// The sky for one moment, drawn into a rectangle: the block in the main
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
    let (view, bodies) = view_at(at, lat, lon, tz);
    starmap::panel(&view, &opts.inner, &bodies, x, y, w, h)
}

/// Draw the sky for `at` full screen, then own the keyboard until the
/// user leaves.
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
            "c" => opts.inner.figures = !opts.inner.figures,
            "n" => opts.inner.names = !opts.inner.names,
            "+" | "=" => opts.inner.mag = (opts.inner.mag + 0.5).min(6.5),
            "-" | "_" => opts.inner.mag = (opts.inner.mag - 0.5).max(1.0),
            _ => {}
        }
    }
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
    let (view, bodies) = view_at(at, lat, lon, tz);
    let mut out = starmap::panel(&view, &opts.inner, &bodies, 1, 2, cols, rows.saturating_sub(2));

    let up = |name: &str| {
        bodies
            .iter()
            .find(|b| b.name == name)
            .map(|b| view.place(b.ra, b.dec).is_some())
            .unwrap_or(false)
    };
    let planets = bodies
        .iter()
        .filter(|b| b.name != "sun" && b.name != "moon" && view.place(b.ra, b.dec).is_some())
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
        sky_state, planets, opts.inner.mag,
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
        " ←/→ hour · ↑/↓ day · c figures · n names · +/- fainter, brighter · q back",
        cols as usize,
    )));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full frame draws, carries its cardinal points, and is quick
    /// enough to redraw on every keypress.
    #[test]
    fn draws_a_frame_fast() {
        let at = Moment { year: 2026, month: 7, day: 28, hour: 23 };
        let opts = Opts::for_bortle(4.0);
        let t = std::time::Instant::now();
        let mut frame = String::new();
        for _ in 0..20 {
            frame = draw(at, 59.9, 10.7, 2.0, "Oslo", &opts, 150, 42);
        }
        let per = t.elapsed() / 20;
        assert!(frame.contains('N') && frame.contains('S'), "no cardinal points");
        assert!(frame.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)), "no braille");
        assert!(per.as_millis() < 20, "one frame took {per:?}");
    }

    /// The moon is a body on the chart, and it moves during the day.
    #[test]
    fn bodies_track_the_hour() {
        let m = |h| {
            let (_, b) = sky_at(Moment { year: 2026, month: 7, day: 28, hour: h }, 59.9, 10.7, 2.0);
            b.into_iter().find(|b| b.name == "moon").unwrap().ra
        };
        let moved = (m(23) - m(0) + 360.0) % 360.0;
        assert!((8.0..18.0).contains(&moved), "moon moved {moved}° over the day");
    }
}
