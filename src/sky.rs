//! The sky over your head for the hour the left pane has selected.
//!
//! The sky itself, the catalogue and the drawing all live in
//! [`starmap`](https://github.com/isene/starmap). What is left here is
//! astro's half: which moment to draw, where the sun, moon and planets
//! are in it, and the keys that walk the night.

use crate::gear::{data, optics};
use crust::style;
use crust::{Crust, Cursor, Input};
use starmap::{Body, Mark, Projection, View};

/// What the chart shows, kept across redraws and across visits.
pub struct Opts {
    pub inner: starmap::Opts,
    /// 1 is the whole sky; more closes in on `centre`.
    pub zoom: f64,
    /// The sky position under the crosshair, followed as the hours turn.
    /// `None` at zoom 1: the whole sky, the zenith in the middle.
    pub centre: Option<(f64, f64)>,
    /// Where the centre last was on screen, for when it has set.
    pan: (f64, f64),
    /// Draw the eyepiece set's circles round the crosshair.
    pub circles: bool,
}

impl Opts {
    /// Start at the faintest star the configured Bortle sky shows, with
    /// the Messier and Caldwell objects on.
    pub fn for_bortle(bortle: f64) -> Self {
        Self {
            inner: starmap::Opts { dso: true, ..starmap::Opts::for_bortle(bortle) },
            zoom: 1.0,
            centre: None,
            pan: (0.0, 0.0),
            circles: false,
        }
    }
}

/// One circle of the eyepiece set: what the combo shows of the sky.
pub struct Field {
    pub label: String,
    pub rgb: (u8, u8, u8),
    pub radius_deg: f64,
}

/// The eyepiece set as circles, from ~/.astro/combos.json and the gear
/// catalogue. A combo whose telescope or eyepiece is gone is skipped.
pub fn fields() -> Vec<Field> {
    let store = data::Store::load();
    data::load_combos()
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let t = store.telescopes.iter().find(|t| t.name == c.scope)?;
            let e = store.eyepieces.iter().find(|e| e.name == c.eyepiece)?;
            let tfov = optics::tfov(t.tfl, e.fl, e.afov);
            Some(Field {
                label: format!("{} + {}  {:.0}×  {:.2}°", t.name, e.name, optics::magx(t.tfl, e.fl), tfov),
                rgb: data::combo_rgb(i),
                radius_deg: tfov / 2.0,
            })
        })
        .collect()
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

/// The view with the chart's zoom, turned to keep `centre` in the middle.
/// A centre that has set keeps the last place it had on screen.
fn looking(at: Moment, lat: f64, lon: f64, tz: f64, opts: &mut Opts) -> (View, Vec<Body>) {
    let (mut view, bodies) = view_at(at, lat, lon, tz);
    view.zoom = opts.zoom;
    if let Some((ra, dec)) = opts.centre {
        if let Some(u) = view.place(ra, dec) {
            opts.pan = u;
        }
        view.pan = opts.pan;
    }
    (view, bodies)
}

/// The Messier or Caldwell object nearest the crosshair, if one is
/// within reach: half a degree, or more on a wide view.
pub fn under_crosshair(view: &View, zoom: f64) -> Option<&'static starmap::Dso> {
    let (ra, dec) = view.centre();
    let reach = (12.0 / zoom).max(0.5);
    starmap::dsos()
        .iter()
        .map(|d| (sep_deg(ra, dec, d.ra, d.dec), d))
        .filter(|(s, _)| *s <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, d)| d)
}

/// Angle between two sky positions, in degrees.
fn sep_deg(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (d1, d2) = (dec1.to_radians(), dec2.to_radians());
    let c = d1.sin() * d2.sin() + d1.cos() * d2.cos() * (ra1 - ra2).to_radians().cos();
    c.clamp(-1.0, 1.0).acos().to_degrees()
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

/// The same block as a picture for glow: the names as text to print,
/// and the chart as a canvas to show at (`x`, `y`).
pub fn picture(at: Moment, lat: f64, lon: f64, tz: f64, opts: &Opts, x: u16, y: u16, w: u16, h: u16) -> starmap::Picture {
    let (view, bodies) = view_at(at, lat, lon, tz);
    starmap::panel_pixels(&view, &opts.inner, &bodies, x, y, w, h)
}

/// Draw the sky for `at` full screen, then own the keyboard until the
/// user leaves. Where `display` shows images the chart is real pixels
/// through glow, else braille.
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
    display: &mut Option<glow::Display>,
) -> usize {
    if moments.is_empty() {
        return index;
    }
    let pixels = display.get_or_insert_with(glow::Display::new).supported();
    let mut set = if opts.circles { fields() } else { Vec::new() };
    let mut note = String::new();
    loop {
        let (cols, rows) = Crust::terminal_size();
        Crust::clear_screen();
        let (text, canvas) = draw(moments[index], lat, lon, tz, place, opts, &set, &note, cols, rows, pixels);
        note.clear();
        print!("{text}");
        use std::io::Write;
        std::io::stdout().flush().ok();
        if let (Some(c), Some(d)) = (canvas, display.as_mut()) {
            d.swap_canvas(&c, 1, 2);
        }

        let Some(key) = Input::getchr(None) else { continue };
        let (view, _) = looking(moments[index], lat, lon, tz, opts);
        // Arrows walk the crosshair a tenth of the screen at a time.
        let walk = |dx: f64, dy: f64, opts: &mut Opts| {
            let step = 0.1 / opts.zoom;
            let p = (view.pan.0 + dx * step, view.pan.1 + dy * step);
            opts.centre = Some(view.unplace(p));
            if opts.zoom <= 1.0 {
                opts.zoom = 1.5;
            }
        };
        match key.as_str() {
            "q" | "Q" | "ESC" | "s" => {
                // The picture would sit over the front screen.
                if let Some(d) = display.as_mut() { d.clear_all(); }
                return index;
            }
            "l" => index = (index + 1).min(moments.len() - 1),
            "h" => index = index.saturating_sub(1),
            "j" | "PgDOWN" => index = (index + 24).min(moments.len() - 1),
            "k" | "PgUP" => index = index.saturating_sub(24),
            "RIGHT" => walk(1.0, 0.0, opts),
            "LEFT" => walk(-1.0, 0.0, opts),
            "DOWN" => walk(0.0, 1.0, opts),
            "UP" => walk(0.0, -1.0, opts),
            "+" | "=" => {
                opts.centre.get_or_insert_with(|| view.centre());
                opts.zoom = (opts.zoom * 1.5).min(400.0);
            }
            "-" | "_" => {
                opts.zoom = (opts.zoom / 1.5).max(1.0);
                if opts.zoom <= 1.0 {
                    opts.centre = None;
                    opts.pan = (0.0, 0.0);
                }
            }
            "0" => {
                opts.zoom = 1.0;
                opts.centre = None;
                opts.pan = (0.0, 0.0);
            }
            "]" => opts.inner.mag = (opts.inner.mag + 0.5).min(6.5),
            "[" => opts.inner.mag = (opts.inner.mag - 0.5).max(1.0),
            "c" => opts.inner.figures = !opts.inner.figures,
            "n" => opts.inner.names = !opts.inner.names,
            "d" => opts.inner.dso = !opts.inner.dso,
            "v" => {
                opts.circles = !opts.circles;
                set = if opts.circles { fields() } else { Vec::new() };
                if opts.circles && set.is_empty() {
                    note = "The eyepiece set is empty: in Gear mode (g), f puts the selected telescope + eyepiece in it".into();
                }
            }
            "/" => {
                let (cols, rows) = Crust::terminal_size();
                let mut p = crust::Pane::new(1, rows, cols, 1, 255, 236);
                p.scroll = false;
                let asked = p.ask_or_cancel(" Go to (M31, C14, NGC 7000, a name): ", "").unwrap_or_default();
                if let Some(d) = find(&asked) {
                    opts.centre = Some((d.ra, d.dec));
                    // Close in until the object, or the widest eyepiece
                    // circle, fills about a third of the screen height.
                    let widest = set.iter().map(|f| f.radius_deg * 2.0).fold(0.0, f64::max);
                    let span = (d.major / 60.0).max(widest).max(0.3) * 3.0;
                    opts.zoom = (180.0 / span).clamp(1.5, 400.0);
                    note = describe(d);
                } else if !asked.trim().is_empty() {
                    note = format!("No Messier or Caldwell object called {}", asked.trim());
                }
            }
            "a" => {
                let at = moments[index];
                let mut night = crate::plan::Night::load(crate::plan::night_of(at.year, at.month, at.day, at.hour), place, lat, lon, tz);
                note = match under_crosshair(&view, opts.zoom) {
                    Some(d) if night.add(d.id) => {
                        let all = fields();
                        night.save(&all);
                        format!("{} added to the plan for the night of {} ({} targets; p shows it)", d.label(), night.date, night.targets.len())
                    }
                    Some(d) => format!("{} is already in the plan", d.label()),
                    None => "Put the crosshair on an object first (/ goes to one)".into(),
                };
            }
            "p" => {
                let at = moments[index];
                let mut night = crate::plan::Night::load(crate::plan::night_of(at.year, at.month, at.day, at.hour), place, lat, lon, tz);
                let all = fields();
                if let Some(d) = display.as_mut() { d.clear_all(); }
                if let Some(d) = crate::plan::run(&mut night, &all) {
                    opts.centre = Some((d.ra, d.dec));
                    let widest = set.iter().map(|f| f.radius_deg * 2.0).fold(0.0, f64::max);
                    let span = (d.major / 60.0).max(widest).max(0.3) * 3.0;
                    opts.zoom = (180.0 / span).clamp(1.5, 400.0);
                    note = describe(d);
                }
            }
            "ENTER" => {
                note = match under_crosshair(&view, opts.zoom) {
                    Some(d) => describe(d),
                    None => "No Messier or Caldwell object under the crosshair".into(),
                };
            }
            _ => {}
        }
    }
}

/// The object a typed name means: its id (`M31`, `m 31`, `C14`), its
/// NGC or IC number (`NGC 7000`, `ngc7000`), or part of its common name.
pub fn find(asked: &str) -> Option<&'static starmap::Dso> {
    let squash = |s: &str| s.to_lowercase().replace(' ', "");
    let q = squash(asked.trim());
    if q.is_empty() {
        return None;
    }
    let all = starmap::dsos();
    all.iter()
        .find(|d| squash(d.id) == q || d.alt.split(['&', '/', ',']).any(|a| squash(a) == q))
        .or_else(|| all.iter().find(|d| !d.name.is_empty() && squash(d.name).contains(&q)))
}

/// One line about an object: what it is, how bright, how big, where.
pub fn describe(d: &starmap::Dso) -> String {
    let mag = d.mag.map(|m| format!("mag {m:.1}")).unwrap_or_else(|| "no magnitude".into());
    let size = if (d.major - d.minor).abs() < 0.05 {
        format!("{:.1}′", d.major)
    } else {
        format!("{:.1}′ × {:.1}′", d.major, d.minor)
    };
    let alt = if d.alt.is_empty() { String::new() } else { format!(" ({})", d.alt) };
    format!("{}{} · {} in {} · {} · {}", d.label(), alt, d.kind.name(), d.constellation, mag, size)
}

/// The whole screen for one moment: title row, chart, key line. With
/// `pixels` the chart comes back as a canvas to show at (1, 2), and the
/// text holds only its names.
#[allow(clippy::too_many_arguments)]
fn draw(
    at: Moment,
    lat: f64,
    lon: f64,
    tz: f64,
    place: &str,
    opts: &mut Opts,
    set: &[Field],
    note: &str,
    cols: u16,
    rows: u16,
    pixels: bool,
) -> (String, Option<glow::Canvas>) {
    let (view, bodies) = looking(at, lat, lon, tz, opts);
    // The crosshair once you close in, and the eyepiece circles round it.
    let mut marks = Vec::new();
    let (cra, cdec) = view.centre();
    if opts.zoom > 1.0 || opts.circles {
        marks.push(Mark { ra: cra, dec: cdec, radius_deg: 0.0, rgb: (255, 255, 255) });
    }
    if opts.circles {
        for f in set {
            marks.push(Mark { ra: cra, dec: cdec, radius_deg: f.radius_deg, rgb: f.rgb });
        }
    }
    // One row under the chart for the circles' legend or a note.
    let legend = (opts.circles && !set.is_empty()) || !note.is_empty();
    let h = rows.saturating_sub(if legend { 3 } else { 2 });
    let (mut out, canvas) = if pixels {
        let p = starmap::panel_pixels_marked(&view, &opts.inner, &bodies, &marks, 1, 2, cols, h);
        (p.text, Some(p.canvas))
    } else {
        (starmap::panel_marked(&view, &opts.inner, &bodies, &marks, 1, 2, cols, h), None)
    };
    if legend {
        let line = if !note.is_empty() {
            style::rgb(&format!(" {note}"), Some((230, 230, 235)), None, "")
        } else {
            set.iter()
                .map(|f| format!("{} {}", style::rgb("●", Some(f.rgb), None, ""), style::rgb(&f.label, Some((200, 200, 205)), None, "")))
                .collect::<Vec<_>>()
                .join("   ")
        };
        out.push_str(&Cursor::at(1, rows.saturating_sub(1)));
        out.push_str(&crust::truncate_ansi(&format!(" {line}"), cols as usize));
    }

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
    let zoomed = if opts.zoom > 1.0 {
        format!(" · ×{:.0} zoom, about {:.0}° high", opts.zoom, view.degrees_across())
    } else {
        String::new()
    };
    let title = format!(
        " {}  {:04}-{:02}-{:02} {:02}:00  {}  ·  {} · {} planets up · stars to mag {:.1}{}",
        place, at.year, at.month, at.day, at.hour,
        if lat >= 0.0 { format!("{lat:.1}°N") } else { format!("{:.1}°S", -lat) },
        sky_state, planets, opts.inner.mag, zoomed,
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
        " h/l hour · j/k day · arrows move · +/- zoom · / go to · ⏎ what is this · a plan it · p the plan · v eyepieces · d objects · 0 all · [/] stars · q back",
        cols as usize,
    )));
    (out, canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full frame draws, carries its cardinal points, and is quick
    /// enough to redraw on every keypress.
    #[test]
    fn draws_a_frame_fast() {
        let at = Moment { year: 2026, month: 7, day: 28, hour: 23 };
        let mut opts = Opts::for_bortle(4.0);
        let t = std::time::Instant::now();
        let mut frame = String::new();
        for _ in 0..20 {
            frame = draw(at, 59.9, 10.7, 2.0, "Oslo", &mut opts, &[], "", 150, 42, false).0;
        }
        let per = t.elapsed() / 20;
        assert!(frame.contains('N') && frame.contains('S'), "no cardinal points");
        assert!(frame.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)), "no braille");
        assert!(per.as_millis() < 20, "one frame took {per:?}");
        let (text, canvas) = draw(at, 59.9, 10.7, 2.0, "Oslo", &mut opts, &[], "", 150, 42, true);
        assert!(canvas.is_some() && text.contains("Oslo"), "the pixel frame has its canvas and title");
    }

    #[test]
    fn finds_objects_by_id_number_and_name() {
        assert_eq!(find("M31").map(|d| d.id), Some("M31"));
        assert_eq!(find("m 42").map(|d| d.id), Some("M42"));
        assert_eq!(find("ngc7000").map(|d| d.id), Some("C20"));
        assert_eq!(find("NGC 884").map(|d| d.id), Some("C14"));
        assert_eq!(find("ring nebula").map(|d| d.id), Some("M57"));
        assert!(find("nothing like it").is_none());
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
