//! The living wallpaper: the sky over you now, the Moon in its phase and
//! the Yr forecast, drawn as one picture for frame, the CHasm X server.
//!
//! `astro --wall` draws it into ~/.astro/wall.bgrx (raw BGRX at the
//! panel's size), tells frame to load it, and sleeps until the next full
//! hour. It draws only while ~/.framerc names that file as its
//! `background`, so pointing frame elsewhere (fx does it) ends the loop at
//! its next wake. `astro --wall --png FILE` draws one picture to look at.

use crate::config::Config;
use crate::weather;
use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use glow::Canvas;
use std::f64::consts::PI;
use std::fs;
use std::path::PathBuf;

/// The near side, 512 × 512 grey, from NASA's LRO camera (as in moon).
const MOON: &[u8] = include_bytes!("../img/moon-512.gray");
const MOON_N: usize = 512;
const MOON_WHITE: f64 = 0.72;
const SYNODIC: f64 = 29.530_588_853;
/// How bright the night side is (earthshine), and how fast the light
/// falls off at the line between day and night.
const NIGHT: f64 = 0.22;
const RAMP: f64 = 5.0;

const INK: (u8, u8, u8) = (220, 226, 238);
const DIM: (u8, u8, u8) = (135, 146, 172);
const WARM: (u8, u8, u8) = (235, 100, 82);
const COLD: (u8, u8, u8) = (95, 155, 245);

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

pub fn run(args: &[String]) {
    let (w, h) = panel_size();
    if let Some(i) = args.iter().position(|a| a == "--png") {
        let Some(out) = args.get(i + 1) else {
            eprintln!("astro: --png needs a file name");
            std::process::exit(2);
        };
        let (cfg, n) = (Config::load(), Now::get());
        let c = draw(&cfg, w, h, &n, &forecast(&cfg, &n));
        if let Err(e) = fs::write(out, c.png()) {
            eprintln!("astro: {out}: {e}");
            std::process::exit(1);
        }
        return;
    }
    let wall = wall_path();
    if !wall_is_on(&wall) {
        eprintln!("astro: ~/.framerc does not show {} as its background; nothing to draw", wall.display());
        return;
    }
    // One drawer at a time: a second start asks the first to draw now.
    let pidfile = pid_path();
    if let Some(pid) = running(&pidfile) {
        unsafe { libc::kill(pid, libc::SIGUSR1) };
        return;
    }
    let _ = fs::write(&pidfile, std::process::id().to_string());
    // SIGUSR1 cuts the hour's sleep short; no SA_RESTART, so it does.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = wake as extern "C" fn(libc::c_int) as usize;
        libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut());
    }
    loop {
        let (cfg, n) = (Config::load(), Now::get());
        let c = draw(&cfg, w, h, &n, &forecast(&cfg, &n));
        if save(&c, &wall).is_ok() {
            nudge_frame();
        }
        drop(c);
        sleep_to_next_hour();
        if !wall_is_on(&wall) {
            break;
        }
    }
    let _ = fs::remove_file(&pidfile);
}

extern "C" fn wake(_: libc::c_int) {}

// ── Time and place ─────────────────────────────────────────────────────

/// The local clock, read once per picture.
struct Now {
    y: i32,
    m: u32,
    d: u32,
    /// The local hour with its minutes.
    hour: f64,
    /// Hours east of UTC, summer time included.
    tz: f64,
    /// Seconds since 1970, UTC.
    epoch: i64,
    wday: usize,
}

impl Now {
    fn get() -> Now {
        let t = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&t, &mut tm) };
        Now {
            y: tm.tm_year + 1900,
            m: tm.tm_mon as u32 + 1,
            d: tm.tm_mday as u32,
            hour: tm.tm_hour as f64 + tm.tm_min as f64 / 60.0,
            tz: tm.tm_gmtoff as f64 / 3600.0,
            epoch: t as i64,
            wday: tm.tm_wday as usize,
        }
    }

    fn local(&self) -> i64 {
        self.epoch + (self.tz * 3600.0) as i64
    }
}

/// The panel's size: the first connected screen's own mode, as
/// chasm-bg finds it, else a common laptop panel.
fn panel_size() -> (usize, usize) {
    let found = fs::read_dir("/sys/class/drm").ok().and_then(|dir| {
        dir.flatten().find_map(|e| {
            let p = e.path();
            if fs::read_to_string(p.join("status")).ok()?.trim() != "connected" {
                return None;
            }
            let modes = fs::read_to_string(p.join("modes")).ok()?;
            let (w, h) = modes.lines().next()?.split_once('x')?;
            Some((w.parse().ok()?, h.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok()?))
        })
    });
    found.unwrap_or((1920, 1080))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

fn wall_path() -> PathBuf {
    home().join(".astro/wall.bgrx")
}

fn pid_path() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d).join("astro-wall.pid"),
        _ => PathBuf::from(format!("/tmp/astro-wall-{}.pid", unsafe { libc::getuid() })),
    }
}

/// The drawer already running, if the pid file names a live one.
fn running(pidfile: &PathBuf) -> Option<i32> {
    let pid: i32 = fs::read_to_string(pidfile).ok()?.trim().parse().ok()?;
    let cmd = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let mut parts = cmd.split(|&b| b == 0);
    let prog = parts.next()?;
    (prog.ends_with(b"astro") && parts.any(|a| a == b"--wall")).then_some(pid)
}

/// Whether ~/.framerc's `background` line names the wallpaper file.
fn wall_is_on(wall: &std::path::Path) -> bool {
    let Ok(rc) = fs::read_to_string(home().join(".framerc")) else { return false };
    rc.lines().any(|l| {
        let l = l.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = l.split_once('=') else { return false };
        let v = v.trim();
        let v = v.strip_prefix("~/").map(|r| home().join(r)).unwrap_or_else(|| PathBuf::from(v));
        k.trim() == "background" && v == wall
    })
}

/// Write the picture as frame wants it, whole or not at all.
fn save(c: &Canvas, wall: &std::path::Path) -> std::io::Result<()> {
    let mut out = Vec::with_capacity(c.w * c.h * 4);
    for px in c.rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    if let Some(dir) = wall.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = wall.with_extension("tmp");
    fs::write(&tmp, out)?;
    fs::rename(tmp, wall)
}

/// Ask this display's frame to load the new picture (SIGUSR2). An old
/// frame without the handler would die of it, so only one that catches
/// SIGUSR2 (bit 11 of SigCgt) gets it.
fn nudge_frame() {
    let Some(display) = std::env::var("DISPLAY").ok() else { return };
    let Some(num) = display.rsplit(':').next().and_then(|n| n.split('.').next()).map(str::to_string) else { return };
    let Ok(dir) = fs::read_dir("/proc") else { return };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        if fs::read_to_string(e.path().join("comm")).map(|c| c.trim() != "frame").unwrap_or(true) {
            continue;
        }
        let Ok(cmd) = fs::read(e.path().join("cmdline")) else { continue };
        if cmd.split(|&b| b == 0).nth(1) != Some(num.as_bytes()) {
            continue;
        }
        let caught = fs::read_to_string(e.path().join("status"))
            .ok()
            .and_then(|s| s.lines().find_map(|l| l.strip_prefix("SigCgt:").map(|v| u64::from_str_radix(v.trim(), 16))))
            .and_then(Result::ok)
            .unwrap_or(0);
        if caught & (1 << 11) != 0 {
            unsafe { libc::kill(pid, libc::SIGUSR2) };
        }
        return;
    }
}

/// Sleep until the next full hour on the wall clock. The clock is the
/// real-time one, so a laptop that slept past the hour wakes us at once;
/// SIGUSR1 ends the sleep early.
#[cfg(target_os = "linux")]
fn sleep_to_next_hour() {
    let n = Now::get();
    let off = (n.tz * 3600.0) as i64;
    let next = ((n.epoch + off) / 3600 + 1) * 3600 - off;
    let ts = libc::timespec { tv_sec: next as libc::time_t, tv_nsec: 0 };
    unsafe { libc::clock_nanosleep(libc::CLOCK_REALTIME, libc::TIMER_ABSTIME, &ts, std::ptr::null_mut()) };
}

#[cfg(not(target_os = "linux"))]
fn sleep_to_next_hour() {
    let n = Now::get();
    std::thread::sleep(std::time::Duration::from_secs((3600 - n.local().rem_euclid(3600)) as u64));
}

// ── The picture ────────────────────────────────────────────────────────

/// The whole wallpaper, `w` × `h` pixels, for the moment `n`.
fn draw(cfg: &Config, w: usize, h: usize, n: &Now, hours: &[Hour]) -> Canvas {
    let mut c = Canvas::with_cell(w as u16, h as u16, (1, 1));
    let k = h as f64 / 1200.0;
    let fonts = Fonts::load();
    let (lst, bodies) = orbit::sky_at(n.y, n.m, n.d, n.hour, cfg.lat, cfg.lon, n.tz);

    // The ground colour follows the sun: night blue, lighter by day.
    let sun_alt = bodies
        .iter()
        .find(|b| b.0 == "sun")
        .map(|b| starmap::altaz(b.1, b.2, lst, cfg.lat).0)
        .unwrap_or(-90.0);
    let day = ((sun_alt + 12.0) / 18.0).clamp(0.0, 1.0);
    let (top, bottom) = (mix((2, 4, 10), (11, 21, 40), day), mix((1, 1, 4), (4, 10, 20), day));
    for y in 0..h {
        let rgb = mix(top, bottom, y as f64 / h as f64);
        for x in 0..w {
            c.put(x, y, rgb);
        }
    }

    // The sky: a square on the left, the chart in its own pixels with the
    // names drawn as text.
    let side = (h as f64 * 0.9).min(w as f64 * 0.58);
    let cell = ((10.0 * k).round().max(6.0) as u16, (20.0 * k).round().max(12.0) as u16);
    let (cols, rows) = ((side / cell.0 as f64) as u16, (side / cell.1 as f64) as u16);
    let sx = (40.0 * k) as usize;
    let sy = ((h as f64 - rows as f64 * cell.1 as f64) / 2.0 + 14.0 * k).max(0.0) as usize;
    let view = starmap::View::new(starmap::Projection::Horizon { lst_deg: lst, lat_deg: cfg.lat });
    let opts = starmap::Opts { mag: 6.0, tint: 2.0, ..starmap::Opts::default() };
    let drawn: Vec<starmap::Body> = bodies
        .iter()
        .map(|&(name, ra, dec)| starmap::Body {
            name: capital(name),
            ra,
            dec,
            rgb: crate::sky::body_rgb(name),
            size: if name == "sun" || name == "moon" { 2 } else { 1 },
        })
        .collect();
    let pic = starmap::picture(&view, &opts, &drawn, &[], 0, 0, cols, rows, Some(cell));
    add_light(&mut c, &pic.canvas, sx, sy);
    let label = cell.1 as f64 * 0.78;
    for l in &pic.labels {
        fonts.write(&mut c, &l.text, sx as f64 + l.x, sy as f64 + l.y + cell.1 as f64 * 0.1, label, l.rgb, l.bold);
    }

    // The right column: where and when, the Moon, the weather.
    let x0 = sx as f64 + cols as f64 * cell.0 as f64 + 60.0 * k;
    let cw = w as f64 - x0 - 60.0 * k;
    let mut y = 70.0 * k;
    fonts.write(&mut c, &cfg.location, x0, y, 46.0 * k, INK, true);
    y += 62.0 * k;
    let when = format!("{} {} {}, {:02}:{:02}", DAYS[n.wday], n.d, MONTHS[n.m as usize - 1], n.hour as u32, ((n.hour.fract() * 60.0).round() as u32).min(59));
    fonts.write(&mut c, &when, x0, y, 24.0 * k, DIM, false);
    y += 70.0 * k;

    let f = (orbit::moon_phase(n.y, n.m, n.d).phase + n.hour / 24.0 / SYNODIC).rem_euclid(1.0);
    let r = (150.0 * k).min(cw * 0.22);
    moon_disk(&mut c, f, x0 + r, y + r, r, cfg.lat < 0.0);
    let tx = x0 + 2.0 * r + 40.0 * k;
    let mut ty = y + r - 90.0 * k;
    fonts.write(&mut c, phase_name(f), tx, ty, 34.0 * k, INK, true);
    ty += 50.0 * k;
    let lit = ((1.0 - (f * 2.0 * PI).cos()) / 2.0 * 100.0).round();
    let next = if f < 0.5 { until(f, 0.5, "full") } else { until(f, 1.0, "new") };
    fonts.write(&mut c, &format!("{lit:.0} % lit, {next}"), tx, ty, 22.0 * k, DIM, false);
    ty += 44.0 * k;
    if let Some((up, down)) = orbit::moon_times(n.y, n.m, n.d, cfg.lat, cfg.lon, n.tz) {
        fonts.write(&mut c, &format!("Moon up {up}, down {down}"), tx, ty, 22.0 * k, DIM, false);
        ty += 34.0 * k;
    }
    if let Some((up, down)) = orbit::sun_times(n.y, n.m, n.d, cfg.lat, cfg.lon, n.tz) {
        fonts.write(&mut c, &format!("Sun up {up}, down {down}"), tx, ty, 22.0 * k, DIM, false);
    }
    y += 2.0 * r + 70.0 * k;

    if !hours.is_empty() {
        fonts.write(&mut c, "Next 24 hours", x0, y, 20.0 * k, DIM, false);
        y += 36.0 * k;
        next_hours(&mut c, &fonts, hours, n, x0, y, cw, k);
        y += 250.0 * k;
        fonts.write(&mut c, "Coming days", x0, y, 20.0 * k, DIM, false);
        y += 36.0 * k;
        coming_days(&mut c, &fonts, hours, n, x0, y, cw, k);
        let credit = "Weather from Yr, by MET Norway and NRK";
        let size = 16.0 * k;
        let width = fonts.width(credit, size, false);
        fonts.write(&mut c, credit, w as f64 - 60.0 * k - width, h as f64 - 44.0 * k, size, DIM, false);
    }
    c
}

/// Add the chart's light to the ground: a star brightens what is under
/// it, and the chart's black stays the ground's colour.
fn add_light(c: &mut Canvas, sky: &Canvas, x0: usize, y0: usize) {
    for y in 0..sky.h.min(c.h.saturating_sub(y0)) {
        for x in 0..sky.w.min(c.w.saturating_sub(x0)) {
            let s = (y * sky.w + x) * 4;
            if sky.rgba[s + 3] == 0 {
                continue;
            }
            let o = ((y0 + y) * c.w + x0 + x) * 4;
            for i in 0..3 {
                c.rgba[o + i] = c.rgba[o + i].saturating_add(sky.rgba[s + i]);
            }
        }
    }
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let m = |p: u8, q: u8| (p as f64 + (q as f64 - p as f64) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn capital(s: &str) -> String {
    let mut ch = s.chars();
    ch.next().map(|f| f.to_uppercase().chain(ch).collect()).unwrap_or_default()
}

// ── The Moon ───────────────────────────────────────────────────────────

/// The Moon for phase `f` (0 new, 0.5 full), radius `r` about (`cx`,
/// `cy`): the map lit from the sunward side, earthshine on the rest.
/// South of the equator it is seen the other way up.
fn moon_disk(c: &mut Canvas, f: f64, cx: f64, cy: f64, r: f64, south: bool) {
    let sun = ((f * 2.0 * PI).sin(), -(f * 2.0 * PI).cos());
    for py in (cy - r - 1.0).max(0.0) as usize..((cy + r + 1.0) as usize).min(c.h) {
        for px in (cx - r - 1.0).max(0.0) as usize..((cx + r + 1.0) as usize).min(c.w) {
            let (mut x, mut y) = ((px as f64 + 0.5 - cx) / r, (cy - py as f64 - 0.5) / r);
            if south {
                (x, y) = (-x, -y);
            }
            let rr = x.hypot(y);
            let cover = ((1.0 - rr) * r + 0.5).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let inward = if rr > 0.995 { 0.995 / rr } else { 1.0 };
            let a = moon_map(x * inward, y * inward);
            let z = (1.0 - rr * rr).max(0.0).sqrt();
            let lit = ((x * sun.0 + z * sun.1) * RAMP).clamp(0.0, 1.0);
            let g = (a * 255.0 * (NIGHT + (1.0 - NIGHT) * lit)).round().clamp(0.0, 255.0) as u8;
            c.blend(px as i64, py as i64, (g, g, g), cover);
        }
    }
}

/// Map brightness (0..1) at disk point (x, y), y up, read between pixels.
fn moon_map(x: f64, y: f64) -> f64 {
    let n = MOON_N as f64;
    let u = ((x + 1.0) / 2.0 * n - 0.5).clamp(0.0, n - 1.0);
    let v = ((1.0 - y) / 2.0 * n - 0.5).clamp(0.0, n - 1.0);
    let (x0, y0) = (u as usize, v as usize);
    let (x1, y1) = ((x0 + 1).min(MOON_N - 1), (y0 + 1).min(MOON_N - 1));
    let (fx, fy) = (u - x0 as f64, v - y0 as f64);
    let at = |i: usize, j: usize| MOON[j * MOON_N + i] as f64;
    let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    ((top * (1.0 - fy) + bottom * fy) / 255.0 / MOON_WHITE).min(1.0)
}

/// The four named phases hold for half a day either side; the rest of
/// the cycle is crescent or gibbous.
fn phase_name(f: f64) -> &'static str {
    let near = |c: f64| {
        let d = (f - c).abs();
        d < 0.017 || d > 0.983
    };
    if near(0.0) {
        "New moon"
    } else if near(0.25) {
        "First quarter"
    } else if near(0.5) {
        "Full moon"
    } else if near(0.75) {
        "Last quarter"
    } else if f < 0.25 {
        "Waxing crescent"
    } else if f < 0.5 {
        "Waxing gibbous"
    } else if f < 0.75 {
        "Waning gibbous"
    } else {
        "Waning crescent"
    }
}

/// "full in 11 days", or "full today" within half a day of it.
fn until(f: f64, target: f64, what: &str) -> String {
    let d = (target - f).rem_euclid(1.0) * SYNODIC;
    if !(0.5..=SYNODIC - 0.5).contains(&d) {
        format!("{what} today")
    } else if d < 1.5 {
        format!("{what} tomorrow")
    } else {
        format!("{what} in {d:.0} days")
    }
}

// ── The weather ────────────────────────────────────────────────────────

/// One point of the forecast, on the local clock.
struct Hour {
    /// Local seconds since 1970 (UTC plus the offset).
    t: i64,
    temp: f64,
    precip: f64,
    symbol: String,
}

/// The forecast from the current hour on: hourly for two days or so,
/// then every six hours. Astro's own cache keeps Yr asked at most once in
/// three hours.
fn forecast(cfg: &Config, n: &Now) -> Vec<Hour> {
    let off = (n.tz * 3600.0) as i64;
    let start = n.local() - n.local().rem_euclid(3600);
    weather::fetch_cached(cfg.lat, cfg.lon, false)
        .into_iter()
        .flat_map(|d| d.hours)
        .filter_map(|p| {
            let t = &p.time;
            let num = |a: usize, b: usize| t.get(a..b)?.parse::<u32>().ok();
            let utc = crate::date_util::date_to_ts(num(0, 4)? as i32, num(5, 7)?, num(8, 10)?, num(11, 13)?, 0, 0);
            Some(Hour { t: utc + off, temp: p.temp, precip: p.precip, symbol: p.symbol })
        })
        .filter(|p| p.t >= start)
        .collect()
}

fn temp_rgb(t: f64) -> (u8, u8, u8) {
    if t > 0.0 {
        WARM
    } else {
        COLD
    }
}

/// The next 24 hours: a symbol and the temperature every third hour, a
/// line through every hour's temperature, and the rain as bars under it.
#[allow(clippy::too_many_arguments)]
fn next_hours(c: &mut Canvas, fonts: &Fonts, hours: &[Hour], n: &Now, x0: f64, y0: f64, w: f64, k: f64) {
    // Hours 0 to 21: the line ends under the last symbol.
    let day: Vec<&Hour> = hours.iter().take_while(|p| p.t < n.local() + 24 * 3600).take(22).collect();
    if day.len() < 2 {
        return;
    }
    let slot = w / 8.0;
    for i in 0..8 {
        let Some(p) = day.get(i * 3) else { break };
        let cx = x0 + slot * (i as f64 + 0.5);
        icon(c, &p.symbol, cx, y0 + 30.0 * k, 58.0 * k);
        let t = format!("{:.0}°", p.temp);
        let size = 24.0 * k;
        fonts.write(c, &t, cx - fonts.width(&t, size, true) / 2.0, y0 + 66.0 * k, size, temp_rgb(p.temp), true);
        let hh = format!("{:02}", p.t.rem_euclid(86400) / 3600);
        let size = 18.0 * k;
        fonts.write(c, &hh, cx - fonts.width(&hh, size, false) / 2.0, y0 + 196.0 * k, size, DIM, false);
    }
    // The line: hour i sits where its third-hour slot would.
    let (lo, hi) = day.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.temp), b.max(p.temp)));
    let span = (hi - lo).max(4.0);
    let mid = (hi + lo) / 2.0;
    let (top, height) = (y0 + 112.0 * k, 52.0 * k);
    let at = |i: usize, t: f64| (x0 + slot * (i as f64 / 3.0 + 0.5), top + height * (0.5 - (t - mid) / span));
    let bar = slot / 3.0 * 0.62;
    for (i, p) in day.iter().enumerate() {
        if p.precip > 0.05 {
            let (bx, _) = at(i, p.temp);
            let bh = (p.precip.min(4.0) / 4.0 * 34.0 * k).max(3.0 * k);
            let base = y0 + 186.0 * k;
            for yy in (base - bh) as i64..base as i64 {
                for xx in (bx - bar / 2.0) as i64..(bx + bar / 2.0) as i64 {
                    c.blend(xx, yy, COLD, 0.85);
                }
            }
        }
        if i + 1 < day.len() {
            c.line(at(i, p.temp), at(i + 1, day[i + 1].temp), 3.0 * k, temp_rgb((p.temp + day[i + 1].temp) / 2.0), 1.0);
        }
    }
}

/// The next five days: the day, its midday symbol, the warmest and the
/// coldest, and the rain.
#[allow(clippy::too_many_arguments)]
fn coming_days(c: &mut Canvas, fonts: &Fonts, hours: &[Hour], n: &Now, x0: f64, y0: f64, w: f64, k: f64) {
    let today = n.local().div_euclid(86400);
    let slot = w / 5.0;
    for i in 0..5 {
        let date = today + 1 + i as i64;
        let points: Vec<&Hour> = hours.iter().filter(|p| p.t.div_euclid(86400) == date).collect();
        if points.is_empty() {
            break;
        }
        let cx = x0 + slot * (i as f64 + 0.5);
        let name = &DAYS[(n.wday + 1 + i) % 7][..3];
        let size = 22.0 * k;
        fonts.write(c, name, cx - fonts.width(name, size, true) / 2.0, y0, size, INK, true);
        let noon = points.iter().min_by_key(|p| (p.t.rem_euclid(86400) / 60 - 13 * 60).abs()).unwrap();
        icon(c, &noon.symbol, cx, y0 + 76.0 * k, 64.0 * k);
        let hi = points.iter().map(|p| p.temp).fold(f64::MIN, f64::max);
        let lo = points.iter().map(|p| p.temp).fold(f64::MAX, f64::min);
        let size = 22.0 * k;
        let (a, b) = (format!("{hi:.0}°"), format!(" / {lo:.0}°"));
        let wa = fonts.width(&a, size, true);
        let x = cx - (wa + fonts.width(&b, size, false)) / 2.0;
        fonts.write(c, &a, x, y0 + 124.0 * k, size, temp_rgb(hi), true);
        fonts.write(c, &b, x + wa, y0 + 124.0 * k, size, DIM, false);
        let rain: f64 = points.iter().map(|p| p.precip).sum();
        if rain >= 0.1 {
            let r = format!("{rain:.1} mm");
            let size = 18.0 * k;
            fonts.write(c, &r, cx - fonts.width(&r, size, false) / 2.0, y0 + 158.0 * k, size, COLD, false);
        }
    }
}

/// A Yr symbol code (`partlycloudy_day`, `lightrainshowers_night`, …)
/// drawn in a box `s` pixels wide about (`cx`, `cy`).
fn icon(c: &mut Canvas, symbol: &str, cx: f64, cy: f64, s: f64) {
    let night = symbol.ends_with("_night");
    let base = symbol.split('_').next().unwrap_or(symbol);
    let light = (205, 210, 222);
    let heavens = |c: &mut Canvas, x: f64, y: f64, r: f64| if night { crescent(c, x, y, r) } else { sun(c, x, y, r) };
    match base {
        "clearsky" => heavens(c, cx, cy, s * 0.26),
        "fair" => {
            heavens(c, cx - s * 0.12, cy - s * 0.1, s * 0.22);
            cloud(c, cx + s * 0.12, cy + s * 0.14, s * 0.55, light);
        }
        "partlycloudy" => {
            heavens(c, cx - s * 0.16, cy - s * 0.16, s * 0.2);
            cloud(c, cx + s * 0.05, cy + s * 0.06, s * 0.8, light);
        }
        "cloudy" => cloud(c, cx, cy, s * 0.92, light),
        "fog" => {
            for i in 0..3 {
                let y = cy - s * 0.18 + i as f64 * s * 0.18;
                let dx = if i == 1 { s * 0.08 } else { 0.0 };
                c.line((cx - s * 0.36 + dx, y), (cx + s * 0.36 - dx, y), s * 0.07, (170, 176, 190), 1.0);
            }
        }
        _ => {
            if base.contains("showers") {
                heavens(c, cx - s * 0.2, cy - s * 0.24, s * 0.17);
            }
            cloud(c, cx, cy - s * 0.1, s * 0.86, (150, 157, 172));
            let drops = if base.starts_with("light") { 2 } else if base.starts_with("heavy") { 4 } else { 3 };
            let (snow, rain) = (base.contains("snow") || base.contains("sleet"), !base.contains("snow"));
            for i in 0..drops {
                let x = cx - s * 0.24 + i as f64 * s * 0.48 / (drops - 1).max(1) as f64;
                let y = cy + s * 0.24;
                if snow && (!rain || i % 2 == 1) {
                    c.disc(x, y + s * 0.06, s * 0.05, (238, 242, 255), 1.0);
                } else {
                    c.line((x, y), (x - s * 0.05, y + s * 0.15), s * 0.05, COLD, 1.0);
                }
            }
            if base.contains("thunder") {
                let p = |dx: f64, dy: f64| (cx + dx * s, cy + dy * s);
                let bolt = [p(0.04, 0.08), p(-0.06, 0.26), p(0.03, 0.26), p(-0.05, 0.44)];
                for pair in bolt.windows(2) {
                    c.line(pair[0], pair[1], s * 0.06, (255, 215, 70), 1.0);
                }
            }
        }
    }
}

fn sun(c: &mut Canvas, cx: f64, cy: f64, r: f64) {
    let gold = (255, 196, 64);
    c.disc(cx, cy, r, gold, 1.0);
    for i in 0..8 {
        let a = i as f64 * PI / 4.0;
        let (dx, dy) = (a.cos(), a.sin());
        c.line((cx + dx * r * 1.3, cy + dy * r * 1.3), (cx + dx * r * 1.65, cy + dy * r * 1.65), r * 0.16, gold, 1.0);
    }
}

/// A crescent Moon: a disc with a second, shifted disc taken out of it.
fn crescent(c: &mut Canvas, cx: f64, cy: f64, r: f64) {
    let (bx, by) = (cx + r * 0.45, cy - r * 0.3);
    for y in (cy - r - 1.0) as i64..=(cy + r + 1.0) as i64 {
        for x in (cx - r - 1.0) as i64..=(cx + r + 1.0) as i64 {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let inside = (r - (px - cx).hypot(py - cy) + 0.5).clamp(0.0, 1.0);
            let outside = ((px - bx).hypot(py - by) - r * 0.82 + 0.5).clamp(0.0, 1.0);
            if inside * outside > 0.0 {
                c.blend(x, y, (232, 232, 214), inside * outside);
            }
        }
    }
}

/// A cloud `w` pixels wide about (`cx`, `cy`): three puffs on a flat base.
fn cloud(c: &mut Canvas, cx: f64, cy: f64, w: f64, rgb: (u8, u8, u8)) {
    c.disc(cx - w * 0.22, cy + w * 0.04, w * 0.19, rgb, 1.0);
    c.disc(cx + w * 0.04, cy - w * 0.08, w * 0.26, rgb, 1.0);
    c.disc(cx + w * 0.26, cy + w * 0.06, w * 0.17, rgb, 1.0);
    c.line((cx - w * 0.3, cy + w * 0.13), (cx + w * 0.33, cy + w * 0.13), w * 0.2, rgb, 1.0);
}

// ── Letters ────────────────────────────────────────────────────────────

/// A sans-serif face and its bold, from the usual places. Without one the
/// picture still comes, with no words on it.
struct Fonts {
    regular: Option<FontVec>,
    bold: Option<FontVec>,
}

impl Fonts {
    fn load() -> Fonts {
        let first = |names: &[&str]| {
            names.iter().find_map(|p| fs::read(p).ok().and_then(|b| FontVec::try_from_vec(b).ok()))
        };
        Fonts {
            regular: first(&[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ]),
            bold: first(&[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            ]),
        }
    }

    fn face(&self, bold: bool) -> Option<&FontVec> {
        if bold { self.bold.as_ref().or(self.regular.as_ref()) } else { self.regular.as_ref() }
    }

    /// How wide `s` is at `size` pixels.
    fn width(&self, s: &str, size: f64, bold: bool) -> f64 {
        let Some(font) = self.face(bold) else { return 0.0 };
        let sf = font.as_scaled(PxScale::from(size as f32));
        let mut prev = None;
        let mut x = 0.0;
        for ch in s.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            x += sf.h_advance(id);
            prev = Some(id);
        }
        x as f64
    }

    /// Write `s` with its top at (`x`, `top`), `size` pixels high.
    #[allow(clippy::too_many_arguments)]
    fn write(&self, c: &mut Canvas, s: &str, x: f64, top: f64, size: f64, rgb: (u8, u8, u8), bold: bool) {
        let Some(font) = self.face(bold) else { return };
        let scale = PxScale::from(size as f32);
        let sf = font.as_scaled(scale);
        let base = top as f32 + sf.ascent();
        let mut pen = x as f32;
        let mut prev = None;
        for ch in s.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                pen += sf.kern(p, id);
            }
            if let Some(g) = font.outline_glyph(id.with_scale_and_position(scale, point(pen, base))) {
                let b = g.px_bounds();
                g.draw(|gx, gy, v| {
                    if v > 0.0 {
                        c.blend(b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64, rgb, v.min(1.0) as f64);
                    }
                });
            }
            pen += sf.h_advance(id);
            prev = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: f64) -> Now {
        // 2026-10-31 local, UTC+1.
        let epoch = crate::date_util::date_to_ts(2026, 10, 31, 0, 0, 0) - 3600 + (h * 3600.0) as i64;
        Now { y: 2026, m: 10, d: 31, hour: h, tz: 1.0, epoch, wday: 6 }
    }

    #[test]
    fn the_wallpaper_fills_the_panel_with_a_night_sky_and_a_lit_moon() {
        let c = draw(&Config::default(), 960, 600, &at(22.0), &[]);
        assert_eq!((c.w, c.h), (960, 600));
        // Night: the ground is dark blue, and stars brighten parts of it.
        let o = (590 * c.w + 900) * 4;
        assert!(c.rgba[o + 2] < 40 && c.rgba[o + 2] >= c.rgba[o], "ground {:?}", &c.rgba[o..o + 3]);
        let bright = c.rgba.chunks(4).filter(|p| p[0].min(p[1]).min(p[2]) > 150).count();
        assert!(bright > 300, "only {bright} bright pixels");
    }

    #[test]
    fn the_moon_is_lit_on_the_sunward_side() {
        let mut c = Canvas::with_cell(200, 200, (1, 1));
        moon_disk(&mut c, 0.25, 100.0, 100.0, 90.0, false);
        let g = |x: usize| c.rgba[(100 * 200 + x) * 4];
        assert!(g(150) > 2 * g(50), "first quarter: right {} left {}", g(150), g(50));
        let mut s = Canvas::with_cell(200, 200, (1, 1));
        moon_disk(&mut s, 0.25, 100.0, 100.0, 90.0, true);
        let g = |x: usize| s.rgba[(100 * 200 + x) * 4];
        assert!(g(50) > 2 * g(150), "south: left {} right {}", g(50), g(150));
    }

    #[test]
    fn phases_have_names_and_a_countdown() {
        assert_eq!(phase_name(0.5), "Full moon");
        assert_eq!(phase_name(0.1), "Waxing crescent");
        assert_eq!(until(0.5 - 1.0 / SYNODIC, 0.5, "full"), "full tomorrow");
        assert_eq!(until(0.2, 0.5, "full"), "full in 9 days");
    }

    #[test]
    fn every_symbol_draws_something() {
        for s in ["clearsky_day", "clearsky_night", "fair_day", "partlycloudy_night", "cloudy", "fog", "lightrain",
            "heavysnowshowers_day", "sleetandthunder", "rainshowersandthunder_night"]
        {
            let mut c = Canvas::with_cell(80, 80, (1, 1));
            icon(&mut c, s, 40.0, 40.0, 64.0);
            let lit = c.rgba.chunks(4).filter(|p| p[0].max(p[1]).max(p[2]) > 60).count();
            assert!(lit > 200, "{s}: {lit} pixels");
        }
    }
}
