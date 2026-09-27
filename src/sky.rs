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
    /// How an object's picture is turned: eye, telescope, star diagonal.
    pub flip: crate::photo::Flip,
    /// Show an object as the eyepiece shows it, not as a photo.
    pub real: bool,
    /// The sky's Bortle class, for how much of a faint object shows.
    pub bortle: f64,
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
            flip: crate::photo::Flip::Eye,
            real: false,
            bortle,
        }
    }
}

/// One circle of the eyepiece set: what the combo shows of the sky.
pub struct Field {
    pub label: String,
    pub rgb: (u8, u8, u8),
    pub radius_deg: f64,
    /// The telescope's aperture, in millimetres, and the magnification.
    pub aperture: f64,
    pub power: f64,
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
                aperture: t.app,
                power: optics::magx(t.tfl, e.fl),
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

/// What Enter described last; Enter again shows its picture.
#[derive(Clone)]
enum Target {
    Dso(&'static starmap::Dso),
    /// The sun, the moon or a planet: its name and where it is.
    Body(String, f64, f64),
}

fn body_target(b: &Body) -> Target {
    Target::Body(b.name.clone(), b.ra, b.dec)
}

/// The deep-sky object, the sun, the moon or the planet nearest the
/// crosshair, if one is within reach.
fn target_under_crosshair(view: &View, zoom: f64, bodies: &[Body]) -> Option<Target> {
    let (ra, dec) = view.centre();
    let reach = (12.0 / zoom).max(0.5);
    let body = bodies
        .iter()
        .map(|b| (sep_deg(ra, dec, b.ra, b.dec), b))
        .filter(|(s, _)| *s <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let dso = under_crosshair(view, zoom).map(|d| (sep_deg(ra, dec, d.ra, d.dec), d));
    match (body, dso) {
        (Some((sb, b)), Some((sd, _))) if sb <= sd => Some(body_target(b)),
        (_, Some((_, d))) => Some(Target::Dso(d)),
        (Some((_, b)), None) => Some(body_target(b)),
        (None, None) => None,
    }
}

/// One line about the sun, the moon or a planet.
fn describe_body(name: &str, ra: f64, dec: f64, at: Moment) -> String {
    let kind = match name {
        "sun" => "our star".to_string(),
        "moon" => format!("{}% lit", orbit::moon_phase_pct(at.year, at.month, at.day)),
        _ => "planet".to_string(),
    };
    let hours = ra.rem_euclid(360.0) / 15.0;
    format!(
        "{} · {} · RA {}h {:02}m · Dec {:+.1}°",
        crate::photo::body_name(name),
        kind,
        hours as u32,
        (hours.fract() * 60.0) as u32,
        dec
    )
}

/// How big a body looks tonight and how the sun lights it, for its
/// eyepiece view: its size from its distance, and the angle sun, body,
/// earth from where the two stand on the sky.
#[allow(clippy::too_many_arguments)]
fn stance(name: &str, ra: f64, dec: f64, bodies: &[Body], at: Moment, lat: f64, lon: f64, tz: f64) -> crate::photo::Stance {
    const AU_KM: f64 = 149_597_870.7;
    let distance = orbit::all_bodies(at.year, at.month, at.day, lat, lon, tz)
        .into_iter()
        .find(|b| b.name == name)
        .map(|b| b.distance)
        .unwrap_or(1.0);
    // The moon's distance comes in earth radii, the rest in AU.
    let km = if name == "moon" { distance * 6378.14 } else { distance * AU_KM };
    let radius_km = match name {
        "sun" => 695_700.0,
        "moon" => 1_737.4,
        "mercury" => 2_439.7,
        "venus" => 6_051.8,
        "mars" => 3_389.5,
        "jupiter" => 71_492.0,
        // Across the rings, which reach 2.27 times the globe.
        "saturn" => 60_268.0 * 2.27,
        "uranus" => 25_559.0,
        "neptune" => 24_764.0,
        _ => 1.0,
    };
    let size_deg = 2.0 * (radius_km / km).atan().to_degrees();
    let (sra, sdec) = bodies.iter().find(|b| b.name == "sun").map(|b| (b.ra, b.dec)).unwrap_or((ra, dec));
    let phase_deg = if name == "sun" {
        0.0
    } else {
        // Earth to sun 1 AU, earth to body `d`, sun to body `r`.
        let (e, d) = (sep_deg(ra, dec, sra, sdec).to_radians(), km / AU_KM);
        let r = (1.0 + d * d - 2.0 * d * e.cos()).sqrt();
        ((r * r + d * d - 1.0) / (2.0 * r * d)).clamp(-1.0, 1.0).acos().to_degrees()
    };
    let da = (sra - ra).to_radians();
    let (d1, d2) = (dec.to_radians(), sdec.to_radians());
    let sun_pa_deg = (da.sin() * d2.cos()).atan2(d1.cos() * d2.sin() - d1.sin() * d2.cos() * da.cos()).to_degrees();
    let mut st = crate::photo::Stance { size_deg, phase_deg, sun_pa_deg, ..Default::default() };
    let ut = at.hour as f64 - tz;
    if name == "jupiter" {
        st.moons = jupiter_moons(days_since_j2000(at.year, at.month, at.day, ut));
    }
    if name == "saturn" {
        // Saturn's north pole points to RA 40.589°, Dec 83.537° (IAU).
        let (ap, dp) = (40.589f64.to_radians(), 83.537f64.to_radians());
        let (a, d) = (ra.to_radians(), dec.to_radians());
        let tilt = -(dp.sin() * d.sin() + dp.cos() * d.cos() * (ap - a).cos());
        st.ring_tilt_deg = tilt.clamp(-1.0, 1.0).asin().to_degrees();
        st.pole_pa_deg = (dp.cos() * (ap - a).sin())
            .atan2(dp.sin() * d.cos() - dp.cos() * d.sin() * (ap - a).cos())
            .to_degrees();
    }
    st
}

/// Days from noon, 1 January 2000 (UT), the epoch the moons count from.
fn days_since_j2000(year: i32, month: u32, day: u32, ut_hours: f64) -> f64 {
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m as i32 + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let since_1970 = era as i64 * 146_097 + doe as i64 - 719_468;
    since_1970 as f64 - 10_957.5 + ut_hours / 24.0
}

/// Where Jupiter's four moons are, `d` days from J2000: west and north
/// of its centre in its equatorial radii, and whether the planet hides
/// each one. Meeus, Astronomical Algorithms, chapter 44, the method good
/// to a few hundredths of a radius.
fn jupiter_moons(d: f64) -> [(f64, f64, bool); 4] {
    let sin = |x: f64| x.to_radians().sin();
    let cos = |x: f64| x.to_radians().cos();
    let v = 172.74 + 0.00111588 * d;
    let m = 357.529 + 0.9856003 * d;
    let n = 20.020 + 0.0830853 * d + 0.329 * sin(v);
    let j = 66.115 + 0.9025179 * d - 0.329 * sin(v);
    let a = 1.915 * sin(m) + 0.020 * sin(2.0 * m);
    let b = 5.555 * sin(n) + 0.168 * sin(2.0 * n);
    let k = j + a - b;
    let big_r = 1.00014 - 0.01671 * cos(m) - 0.00014 * cos(2.0 * m);
    let r = 5.20872 - 0.25208 * cos(n) - 0.00611 * cos(2.0 * n);
    let delta = (r * r + big_r * big_r - 2.0 * r * big_r * cos(k)).sqrt();
    let psi = (big_r / delta * sin(k)).asin().to_degrees();
    let t = d - delta / 173.0;
    let mut u = [
        163.8069 + 203.4058646 * t + psi - b,
        358.4140 + 101.2916335 * t + psi - b,
        5.7176 + 50.2345180 * t + psi - b,
        224.8092 + 21.4879800 * t + psi - b,
    ];
    let g = 331.18 + 50.310482 * t;
    let h = 87.45 + 21.569231 * t;
    let (u1, u2, u3) = (u[0], u[1], u[2]);
    u[0] += 0.473 * sin(2.0 * (u1 - u2));
    u[1] += 1.065 * sin(2.0 * (u2 - u3));
    u[2] += 0.165 * sin(g);
    u[3] += 0.843 * sin(h);
    let rr = [
        5.9057 - 0.0244 * cos(2.0 * (u1 - u2)),
        9.3966 - 0.0882 * cos(2.0 * (u2 - u3)),
        14.9883 - 0.0216 * cos(g),
        26.3627 - 0.1939 * cos(h),
    ];
    let lambda = 34.35 + 0.083091 * d + 0.329 * sin(v) + b;
    let ds = 3.12 * sin(lambda + 42.8);
    let de = ds - 2.22 * sin(psi) * cos(lambda + 22.0) - 1.30 * (r - delta) / delta * sin(lambda - 100.5);
    let mut out = [(0.0, 0.0, false); 4];
    for i in 0..4 {
        let (x, y) = (rr[i] * sin(u[i]), -rr[i] * cos(u[i]) * sin(de));
        // At u near 0 the moon is beyond the planet; inside the disk
        // it is hidden then.
        let behind = cos(u[i]) > 0.0 && x * x + (y / 0.935).powi(2) < 1.0;
        out[i] = (x, y, behind);
    }
    out
}

fn describe_target(t: &Target, at: Moment) -> String {
    match t {
        Target::Dso(d) => describe(d),
        Target::Body(name, ra, dec) => describe_body(name, *ra, *dec, at),
    }
}

/// Angle between two sky positions, in degrees.
fn sep_deg(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (d1, d2) = (dec1.to_radians(), dec2.to_radians());
    let c = d1.sin() * d2.sin() + d1.cos() * d2.cos() * (ra1 - ra2).to_radians().cos();
    c.clamp(-1.0, 1.0).acos().to_degrees()
}

/// The sky for one moment, drawn into a rectangle: the block in the main
/// pane on the front screen. It leaves out the Messier and Caldwell
/// objects, which crowd a chart that small.
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
    starmap::panel(&view, &small(opts), &bodies, x, y, w, h)
}

/// The same block as a picture for glow: the names as text to print,
/// and the chart as a canvas to show at (`x`, `y`).
pub fn picture(at: Moment, lat: f64, lon: f64, tz: f64, opts: &Opts, x: u16, y: u16, w: u16, h: u16) -> starmap::Picture {
    let (view, bodies) = view_at(at, lat, lon, tz);
    starmap::panel_pixels(&view, &small(opts), &bodies, x, y, w, h)
}

fn small(opts: &Opts) -> starmap::Opts {
    starmap::Opts { dso: false, ..opts.inner }
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
    // The object the note describes: Enter again shows its picture.
    let mut described: Option<Target> = None;
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
        let shown = described.take();
        let (view, bodies) = looking(moments[index], lat, lon, tz, opts);
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
            // Ctrl+A, as in every Fe2O3 app: a Claude session about the
            // chart and what sits under the crosshair.
            "C-A" => {
                if let Some(d) = display.as_mut() { d.clear_all(); }
                let m = moments[index];
                let what = target_under_crosshair(&view, opts.zoom, &bodies)
                    .map(|t| describe_target(&t, m))
                    .unwrap_or_else(|| "no named object".into());
                let ctx = format!(
                    "The sky chart for {place}, {:04}-{:02}-{:02} {:02}:00 (lat {lat:.2}, lon {lon:.2}), zoomed {:.0}×.\n\
                     Under the crosshair: {what}\n",
                    m.year, m.month, m.day, m.hour, opts.zoom,
                );
                if !crust::claude_session("Astro", crate::CLAUDE_INTRO, &ctx) {
                    note = "claude is not on the PATH".into();
                }
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
                let asked = p.ask_or_cancel(" Go to (M31, C14, NGC 7000, a name, a planet): ", "").unwrap_or_default();
                let body = bodies.iter().find(|b| !asked.trim().is_empty() && b.name.eq_ignore_ascii_case(asked.trim()));
                if let Some(b) = body {
                    opts.centre = Some((b.ra, b.dec));
                    opts.zoom = 60.0;
                    let t = body_target(b);
                    note = format!("{}   (⏎ again: its picture)", describe_target(&t, moments[index]));
                    described = Some(t);
                } else if let Some(d) = find(&asked) {
                    described = Some(Target::Dso(d));
                    opts.centre = Some((d.ra, d.dec));
                    // Close in until the object, or the widest eyepiece
                    // circle, fills about a third of the screen height.
                    let widest = set.iter().map(|f| f.radius_deg * 2.0).fold(0.0, f64::max);
                    let span = (d.major / 60.0).max(widest).max(0.3) * 3.0;
                    opts.zoom = (180.0 / span).clamp(1.5, 400.0);
                    note = format!("{}   (⏎ again: its picture)", describe(d));
                } else if !asked.trim().is_empty() {
                    note = format!("No Messier or Caldwell object or planet called {}", asked.trim());
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
                    described = Some(Target::Dso(d));
                    opts.centre = Some((d.ra, d.dec));
                    let widest = set.iter().map(|f| f.radius_deg * 2.0).fold(0.0, f64::max);
                    let span = (d.major / 60.0).max(widest).max(0.3) * 3.0;
                    opts.zoom = (180.0 / span).clamp(1.5, 400.0);
                    note = format!("{}   (⏎ again: its picture)", describe(d));
                }
            }
            "ENTER" => match shown {
                // Enter again: the object's picture, in a box over the chart.
                Some(t) => {
                    if let Some(disp) = display.as_mut() { disp.clear_all(); }
                    let subject = match &t {
                        Target::Dso(d) => crate::photo::Subject::Dso(d),
                        Target::Body(name, ra, dec) => crate::photo::Subject::Body(
                            name,
                            stance(name, *ra, *dec, &bodies, moments[index], lat, lon, tz),
                        ),
                    };
                    match crate::photo::show(subject, opts.flip, opts.real, opts.bortle) {
                        Ok((flip, real)) => { opts.flip = flip; opts.real = real; }
                        Err(e) => note = e,
                    }
                }
                None => {
                    note = match target_under_crosshair(&view, opts.zoom, &bodies) {
                        Some(t) => {
                            let line = format!("{}   (⏎ again: its picture)", describe_target(&t, moments[index]));
                            described = Some(t);
                            line
                        }
                        None => "Nothing to show under the crosshair: no Messier or Caldwell object, planet, Sun or Moon".into(),
                    };
                }
            },
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
    out.push_str(&bar(&title, cols, (255, 200, 120), "b"));
    out.push_str(&Cursor::at(1, rows));
    out.push_str(&bar(
        " h/l hour · j/k day · arrows move · +/- zoom · / go to · ⏎ what is this · a plan it · p the plan · v eyepieces · d objects · 0 all · [/] stars · q back",
        cols,
        (215, 215, 220),
        "",
    ));
    (out, canvas)
}

/// A full-width row on a dark grey bar: the title at the top and the
/// key legend at the bottom, set apart from the chart and the note.
pub fn bar(text: &str, cols: u16, fg: (u8, u8, u8), attrs: &str) -> String {
    let t: String = text.chars().take(cols as usize).collect();
    style::rgb(&crust::pad_display(&t, cols as usize), Some(fg), Some((60, 60, 66)), attrs)
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
    /// Venus three weeks before it passes the sun is a crescent, lit from
    /// the sun's side; the moon on 2026-09-27 is full, half a degree wide.
    #[test]
    fn a_body_stands_as_the_sky_has_it() {
        let at = Moment { year: 2026, month: 9, day: 27, hour: 20 };
        let (_, bodies) = sky_at(at, 59.9, 10.7, 2.0);
        let get = |n: &str| bodies.iter().find(|b| b.name == n).map(|b| (b.ra, b.dec)).unwrap();
        let (vra, vdec) = get("venus");
        let venus = stance("venus", vra, vdec, &bodies, at, 59.9, 10.7, 2.0);
        assert!((100.0..140.0).contains(&venus.phase_deg), "a crescent: phase angle {}", venus.phase_deg);
        assert!((35.0 / 3600.0..55.0 / 3600.0).contains(&venus.size_deg), "about 45 arcseconds: {}", venus.size_deg * 3600.0);
        assert!((200.0..340.0).contains(&venus.sun_pa_deg.rem_euclid(360.0)), "the sun lies west of Venus: {}", venus.sun_pa_deg);
        let (mra, mdec) = get("moon");
        let moon = stance("moon", mra, mdec, &bodies, at, 59.9, 10.7, 2.0);
        assert!(moon.phase_deg < 20.0, "full: {}", moon.phase_deg);
        assert!((0.48..0.57).contains(&moon.size_deg), "{}", moon.size_deg);
        let (sra, sdec) = get("sun");
        assert!((0.52..0.545).contains(&stance("sun", sra, sdec, &bodies, at, 59.9, 10.7, 2.0).size_deg));
    }

    /// Fetches pictures: `ASTRO_PHOTO_DUMP=dir cargo test -- --ignored body_views`
    /// writes the sun, the moon and the planets on 2026-09-27 through a
    /// 150 mm telescope at 50× (2°) and at 200× (0.4°).
    #[test]
    #[ignore]
    fn body_views() {
        let Ok(dir) = std::env::var("ASTRO_PHOTO_DUMP") else { return };
        let at = Moment { year: 2026, month: 9, day: 27, hour: 20 };
        let (_, bodies) = sky_at(at, 59.9, 10.7, 2.0);
        for b in &bodies {
            let st = stance(&b.name, b.ra, b.dec, &bodies, at, 59.9, 10.7, 2.0);
            for (fov, power) in [(2.0, 50.0), (0.4, 200.0)] {
                let field = Field { label: String::new(), rgb: (0, 0, 0), radius_deg: fov / 2.0, aperture: 150.0, power };
                let v = crate::photo::body_view_for_test(&b.name, st, &field);
                v.save(format!("{dir}/{}-{power:.0}.png", b.name)).unwrap();
            }
        }
    }

    /// Meeus's example 44.a, 1992 December 16 at 0h UT: Io 3.44 radii
    /// east, Europa 7.44 west, Ganymede 1.24 west, Callisto 7.08 west.
    #[test]
    fn jupiters_moons_stand_where_meeus_has_them() {
        let moons = jupiter_moons(days_since_j2000(1992, 12, 16, 0.0));
        let xs: Vec<f64> = moons.iter().map(|m| m.0).collect();
        for (got, want) in xs.iter().zip([-3.44, 7.44, 1.24, 7.08]) {
            assert!((got - want).abs() < 0.1, "moon at {got:.2}, Meeus {want}");
        }
        assert_eq!(days_since_j2000(2000, 1, 1, 12.0), 0.0);
    }

    /// Saturn in late 2026: the rings a few degrees open, the south face
    /// toward us since they passed edge-on in 2025.
    #[test]
    fn saturns_rings_are_nearly_shut_in_2026() {
        let at = Moment { year: 2026, month: 9, day: 27, hour: 20 };
        let (_, bodies) = sky_at(at, 59.9, 10.7, 2.0);
        let s = bodies.iter().find(|b| b.name == "saturn").unwrap();
        let st = stance("saturn", s.ra, s.dec, &bodies, at, 59.9, 10.7, 2.0);
        assert!((-12.0..0.0).contains(&st.ring_tilt_deg), "tilt {}", st.ring_tilt_deg);
    }

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
