//! A deep-sky object's picture, or the sun's, the moon's or a planet's:
//! the lead image of its Wikipedia article, in a box in the middle of
//! the screen. `f` turns it the way the naked
//! eye, a telescope (south up) or a star diagonal (mirrored) shows the
//! object, as in the moon app.
//!
//! Each picture is fetched once, at 1200 pixels wide, and kept in
//! ~/.astro/images/dso/, so looking again costs no network.
//!
//! `r` swaps the photo for the eyepiece view: a plate of the Digitized
//! Sky Survey cut to what the eyepiece shows, grey and dimmed the way
//! the eye sees a faint object through that telescope. The sun, the
//! moon and a planet get their true size in the field instead, lit from
//! the side the sun is on; the sun as today's white-light picture from
//! the SDO satellite, the view a solar filter gives.

use crate::sky::Field;
use crust::{Crust, Input, Popup};
use image::{imageops::FilterType, DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use starmap::Dso;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

/// Which way the picture is turned.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Flip {
    #[default]
    Eye,
    Telescope,
    Diagonal,
}

impl Flip {
    pub fn next(self) -> Flip {
        match self {
            Flip::Eye => Flip::Telescope,
            Flip::Telescope => Flip::Diagonal,
            Flip::Diagonal => Flip::Eye,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Flip::Eye => "naked eye",
            Flip::Telescope => "telescope (south up)",
            Flip::Diagonal => "star diagonal (mirrored)",
        }
    }

    fn apply(self, img: &DynamicImage) -> DynamicImage {
        match self {
            Flip::Eye => img.clone(),
            Flip::Telescope => img.rotate180(),
            Flip::Diagonal => img.fliph(),
        }
    }
}

/// Wikimedia asks every program to say who it is.
const AGENT: &str = "astro (https://github.com/isene/astro)";

/// What the picture box shows: a deep-sky object, or the sun, the moon
/// or a planet by its lower-case name ("jupiter") and where it stands.
#[derive(Clone, Copy)]
pub enum Subject<'a> {
    Dso(&'a Dso),
    Body(&'a str, Stance),
}

/// Where the sun, the moon or a planet stands, for its eyepiece view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stance {
    /// How big it looks, in degrees across; Saturn across its rings.
    pub size_deg: f64,
    /// The angle sun, body, earth: 0 is full, 180 is new.
    pub phase_deg: f64,
    /// Which way the sun lies from it on the sky, from north through east.
    pub sun_pa_deg: f64,
}

impl Subject<'_> {
    fn label(&self) -> String {
        match self {
            Subject::Dso(d) => d.id.to_string(),
            Subject::Body(n, _) => body_name(n),
        }
    }

    /// How big it looks, for picking the eyepiece that frames it.
    fn size_deg(&self) -> f64 {
        match self {
            Subject::Dso(d) => d.major / 60.0,
            Subject::Body(_, st) => st.size_deg,
        }
    }

    fn titles(&self) -> Vec<String> {
        match self {
            Subject::Dso(d) => titles(d),
            Subject::Body(n, _) => vec![body_article(n)],
        }
    }
}

/// The name a body goes by: "jupiter" is Jupiter.
pub fn body_name(n: &str) -> String {
    let mut c = n.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A body's Wikipedia article. Mercury the planet shares its name.
fn body_article(n: &str) -> String {
    if n == "mercury" { "Mercury (planet)".into() } else { body_name(n) }
}

fn cache_path(s: &Subject) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let (dir, key) = match s {
        Subject::Dso(d) => ("dso", d.id.to_string()),
        Subject::Body(n, _) => ("body", n.to_string()),
    };
    PathBuf::from(home).join(".astro/images").join(dir).join(format!("{key}.img"))
}

/// Article titles to try, best first. Wikipedia keeps "Messier 31" and
/// "Caldwell 14" as ways in, even for an object with no common name;
/// the NGC number and the common name come after.
fn titles(d: &Dso) -> Vec<String> {
    let mut t = Vec::new();
    if let Some(n) = d.id.strip_prefix('M') {
        t.push(format!("Messier {n}"));
    }
    if let Some(n) = d.id.strip_prefix('C') {
        t.push(format!("Caldwell {n}"));
    }
    let ngc = d.alt.split(['&', '/', ',']).next().unwrap_or("").trim();
    if !ngc.is_empty() {
        t.push(ngc.to_string());
    }
    let name = d.name.split([',', '(']).next().unwrap_or("").split(" or ").next().unwrap_or("").trim();
    if !name.is_empty() {
        t.push(name.to_string());
    }
    t
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

/// The address of an article's lead picture, 1200 pixels wide.
fn image_url(title: &str) -> Option<String> {
    let url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&format=json&redirects=1&prop=pageimages&piprop=thumbnail&pithumbsize=1200&titles={}",
        encode(title)
    );
    let v: serde_json::Value = ureq::get(&url).set("User-Agent", AGENT)
        .timeout(Duration::from_secs(15)).call().ok()?.into_json().ok()?;
    v["query"]["pages"].as_object()?.values().next()?["thumbnail"]["source"].as_str().map(String::from)
}

/// The picture's bytes: kept ones, else fetched from Wikipedia and kept.
fn picture(s: &Subject) -> Result<Vec<u8>, String> {
    let path = cache_path(s);
    let id = s.label();
    if let Ok(b) = std::fs::read(&path) {
        if !b.is_empty() {
            return Ok(b);
        }
    }
    let url = s.titles().iter().find_map(|t| image_url(t))
        .ok_or_else(|| format!("Wikipedia has no picture of {id} (or could not be reached)"))?;
    let mut bytes = Vec::new();
    ureq::get(&url).set("User-Agent", AGENT).timeout(Duration::from_secs(30)).call()
        .map_err(|e| format!("the picture of {id} would not download: {e}"))?
        .into_reader().take(20 << 20).read_to_end(&mut bytes)
        .map_err(|e| format!("the picture of {id} broke off: {e}"))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes)
}

/// The CDS image service cuts a plate of any size and field out of the
/// Digitized Sky Survey (DSS2, the red plates): north up, east left. It
/// comes as FITS, the scanned density of each pixel, since the survey's
/// JPEG burns every bright core to plain white.
const SURVEY_PX: u32 = 800;

fn survey_path(d: &Dso, fov: f64) -> PathBuf {
    cache_path(&Subject::Dso(d)).with_file_name(format!("{}-dss-{:.0}.fits", d.id, fov * 60.0))
}

/// A survey plate: one value per pixel, the top row first.
pub struct Plate {
    w: u32,
    h: u32,
    px: Vec<f32>,
}

/// The first image in a FITS file: 80-character header cards in blocks
/// of 2880 bytes, then big-endian pixels with the bottom row first.
fn read_fits(b: &[u8]) -> Result<Plate, String> {
    let card = |i: usize| std::str::from_utf8(&b[i * 80..i * 80 + 80]).unwrap_or("");
    let (mut bitpix, mut w, mut h, mut zero, mut scale) = (0i64, 0usize, 0usize, 0f64, 1f64);
    let mut i = 0;
    loop {
        if (i + 1) * 80 > b.len() {
            return Err("the survey plate has no end to its header".into());
        }
        let c = card(i);
        i += 1;
        if c.len() < 8 {
            continue;
        }
        if c.starts_with("END") && c[3..].trim().is_empty() {
            break;
        }
        let value = c.get(10..).unwrap_or("").split('/').next().unwrap_or("").trim();
        match c[..8].trim() {
            "BITPIX" => bitpix = value.parse().unwrap_or(0),
            "NAXIS1" => w = value.parse().unwrap_or(0),
            "NAXIS2" => h = value.parse().unwrap_or(0),
            "BZERO" => zero = value.parse().unwrap_or(0.0),
            "BSCALE" => scale = value.parse().unwrap_or(1.0),
            _ => {}
        }
    }
    let start = (i * 80).div_ceil(2880) * 2880;
    let size = (bitpix.unsigned_abs() / 8) as usize;
    if w == 0 || h == 0 || !matches!(bitpix, 16 | 32 | -32) || b.len() < start + w * h * size {
        return Err("the survey plate is not an image astro reads".into());
    }
    let at = |k: usize| {
        let q = &b[start + k * size..start + (k + 1) * size];
        let raw = match bitpix {
            16 => i16::from_be_bytes([q[0], q[1]]) as f64,
            32 => i32::from_be_bytes([q[0], q[1], q[2], q[3]]) as f64,
            _ => f32::from_be_bytes([q[0], q[1], q[2], q[3]]) as f64,
        };
        (zero + scale * raw) as f32
    };
    let mut px = Vec::with_capacity(w * h);
    for row in (0..h).rev() {
        px.extend((0..w).map(|x| at(row * w + x)));
    }
    Ok(Plate { w: w as u32, h: h as u32, px })
}

/// The survey plate of `d`, `fov` degrees across: kept, else fetched.
fn survey(d: &Dso, fov: f64) -> Result<Vec<u8>, String> {
    let path = survey_path(d, fov);
    if let Ok(b) = std::fs::read(&path) {
        if !b.is_empty() {
            return Ok(b);
        }
    }
    let url = format!(
        "https://alasky.cds.unistra.fr/hips-image-services/hips2fits?hips=CDS%2FP%2FDSS2%2Fred&width={SURVEY_PX}&height={SURVEY_PX}&fov={fov:.4}&projection=TAN&coordsys=icrs&ra={:.5}&dec={:.5}&format=fits",
        d.ra, d.dec
    );
    let mut bytes = Vec::new();
    ureq::get(&url).set("User-Agent", AGENT).timeout(Duration::from_secs(30)).call()
        .map_err(|e| format!("the sky survey would not answer: {e}"))?
        .into_reader().take(8 << 20).read_to_end(&mut bytes)
        .map_err(|e| format!("the survey plate broke off: {e}"))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes)
}

/// The eyepiece to look at `d` through: the set's combo with the
/// smallest field that still holds the object with room round it, else
/// the widest one.
fn best_field(fields: &[Field], size_deg: f64) -> usize {
    let want = size_deg * 1.5;
    let mut order: Vec<usize> = (0..fields.len()).collect();
    order.sort_by(|&a, &b| fields[a].radius_deg.total_cmp(&fields[b].radius_deg));
    order.iter().copied().find(|&i| fields[i].radius_deg * 2.0 >= want).or(order.last().copied()).unwrap_or(0)
}

/// A stand-in when the eyepiece set is empty: a 100 mm telescope with a
/// field three times the object's size.
fn stand_in(size_deg: f64) -> Field {
    let fov = (size_deg * 3.0).clamp(0.5, 4.0);
    Field {
        label: format!("no eyepiece set (f in Gear mode): 100 mm, {fov:.1}°"),
        rgb: (0, 0, 0),
        radius_deg: fov / 2.0,
        aperture: 100.0,
        power: 0.0,
    }
}

/// The eyepiece view's line: the telescope, the power and the field.
fn field_note(f: &Field) -> String {
    if f.power > 0.0 {
        format!("{:.0} mm  {:.0}×  {:.2}°", f.aperture, f.power, f.radius_deg * 2.0)
    } else {
        f.label.clone()
    }
}

/// A survey plate as the eye sees it at the eyepiece. A plate's density
/// grows with the log of the light, as the eye's sense of brightness
/// does, from the sky up to where the plate burns out. The plate shows
/// far fainter light than any eye; what is left is the part above a
/// threshold that falls as the telescope grows and rises as the sky
/// brightens. Stars stay points, down to what the telescope reaches.
/// Then grey, never white, inside the round field stop.
///
/// A model of the view, not a measurement of it.
pub fn eyepiece_view(plate: &Plate, aperture: f64, bortle: f64) -> DynamicImage {
    let (w, h) = (plate.w, plate.h);
    let mut vals: Vec<f32> = plate.px.iter().copied().filter(|v| v.is_finite()).collect();
    if vals.is_empty() {
        return DynamicImage::ImageLuma8(GrayImage::new(w, h));
    }
    let n = vals.len();
    let sky = *vals.select_nth_unstable_by(n / 2, f32::total_cmp).1 as f64;
    let full = (*vals.select_nth_unstable_by(n - 1 - n / 1000, f32::total_cmp).1 as f64).max(sky + 1.0);
    // The eye needs area: softening before the cut drops the faint stars
    // and keeps the faint glow round a galaxy.
    let v: image::ImageBuffer<Luma<f32>, Vec<f32>> = image::ImageBuffer::from_fn(w, h, |x, y| {
        let p = plate.px[(y * w + x) as usize];
        Luma([if p.is_finite() { ((p as f64 - sky) / (full - sky)).clamp(0.0, 1.0) as f32 } else { 0.0 }])
    });
    let v = image::imageops::blur(&v, w as f32 / 300.0);
    let t = (0.16 - 0.12 * (aperture.max(20.0) / 150.0).log10() + 0.02 * (bortle - 4.0)).clamp(0.04, 0.6);
    // Stars stay points. Only the densest show, and fewer as the
    // telescope shrinks: a bright star burns the plate to the top.
    let star_cut = (0.8 - 0.25 * (aperture.max(20.0) / 150.0).log10() + 0.02 * (bortle - 4.0)).clamp(0.5, 0.97);
    let mut soft = GrayImage::from_fn(w, h, |x, y| {
        let glow = ((v.get_pixel(x, y)[0] as f64 - t) / (1.0 - t)).clamp(0.0, 1.0);
        let p = plate.px[(y * w + x) as usize];
        let raw = if p.is_finite() { (p as f64 - sky) / (full - sky) } else { 0.0 };
        let star = ((raw - star_cut) / (1.0 - star_cut)).clamp(0.0, 1.0);
        Luma([(glow.max(star) * 190.0) as u8])
    });
    soft = image::imageops::blur(&soft, w as f32 / 1000.0);
    // The sky in the field is never quite black, and paler in a brighter
    // sky; outside the field stop it is.
    let sky_grey = (4.0 + 2.0 * bortle.clamp(1.0, 9.0)) as u8;
    let (cx, cy, r) = (w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0);
    for (x, y, p) in soft.enumerate_pixels_mut() {
        p[0] = if (x as f64 + 0.5 - cx).hypot(y as f64 + 0.5 - cy) > r { 0 } else { p[0].max(sky_grey) };
    }
    DynamicImage::ImageLuma8(soft)
}

/// Today's white light from the sun, as the SDO satellite's HMI camera
/// sees it: the view through a solar filter, spots and all. Kept for
/// twelve hours, since the spots move and change from day to day.
fn sun_today() -> Result<Vec<u8>, String> {
    let path = cache_path(&Subject::Body("sun", Stance { size_deg: 0.0, phase_deg: 0.0, sun_pa_deg: 0.0 }))
        .with_file_name("sun-hmi.jpg");
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(12 * 3600));
    if fresh {
        if let Ok(b) = std::fs::read(&path) {
            return Ok(b);
        }
    }
    let mut bytes = Vec::new();
    let got = ureq::get("https://sdo.gsfc.nasa.gov/assets/img/latest/latest_1024_HMII.jpg")
        .set("User-Agent", AGENT).timeout(Duration::from_secs(30)).call()
        .map_err(|e| e.to_string())
        .and_then(|r| r.into_reader().take(8 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string()));
    match got {
        Ok(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &bytes);
            Ok(bytes)
        }
        // Yesterday's sun beats none.
        Err(e) => std::fs::read(&path).map_err(|_| format!("the SDO satellite's picture of the sun would not download: {e}")),
    }
}

/// Where the body is in its photo: the longest run of rows and of
/// columns with light in them, so a caption line does not count.
/// Gives the box as (left, top, width, height).
fn find_disk(img: &RgbImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = img.dimensions();
    let lit = |p: &Rgb<u8>| (p[0] as u32 + p[1] as u32 + p[2] as u32) > 90;
    let mut rows = vec![0u32; h as usize];
    let mut cols = vec![0u32; w as usize];
    for (x, y, p) in img.enumerate_pixels() {
        if lit(p) {
            rows[y as usize] += 1;
            cols[x as usize] += 1;
        }
    }
    let run = |counts: &[u32], min: u32| -> Option<(u32, u32)> {
        let (mut best, mut start) = (None::<(u32, u32)>, None::<u32>);
        for (i, &c) in counts.iter().chain(std::iter::once(&0)).enumerate() {
            match (c > min, start) {
                (true, None) => start = Some(i as u32),
                (false, Some(s0)) => {
                    let len = i as u32 - s0;
                    if best.is_none_or(|(_, l)| len > l) {
                        best = Some((s0, len));
                    }
                    start = None;
                }
                _ => {}
            }
        }
        best
    };
    let (top, height) = run(&rows, (w / 100).max(1))?;
    let (left, width) = run(&cols, (h / 100).max(1))?;
    Some((left, top, width, height))
}

/// The sun, the moon or a planet as the eyepiece shows it: its picture
/// cut to the body, at its true size in the field, lit from the side
/// the sun is on (a thin crescent for Venus near the sun, a gibbous
/// Mars), softened by the air or the telescope's own limit, whichever
/// is coarser, on the night sky in the round field stop. The sun shows
/// orange, as a glass solar filter shows it, on black.
pub fn body_view(picture: &DynamicImage, name: &str, st: Stance, field: &Field, bortle: f64) -> DynamicImage {
    const N: u32 = 800;
    let fov = field.radius_deg * 2.0;
    let sun = name == "sun";
    let sky = if sun { 0 } else { (4.0 + 2.0 * bortle.clamp(1.0, 9.0)) as u8 };
    let mut out = RgbImage::from_pixel(N, N, Rgb([sky, sky, sky]));
    let rgb = picture.to_rgb8();
    if let Some((left, top, bw, bh)) = find_disk(&rgb) {
        // Across the body on the plate, in pixels; the height keeps the
        // picture's own shape (Saturn is wider than tall).
        let across = st.size_deg / fov * N as f64;
        let tall = across * bh as f64 / bw as f64;
        let body = image::imageops::crop_imm(&rgb, left, top, bw, bh).to_image();
        let (tw, th) = (across.round().max(1.0) as u32, tall.round().max(1.0) as u32);
        let body = image::imageops::resize(&body, tw, th, if tw < bw { FilterType::Triangle } else { FilterType::CatmullRom });
        // The sun's direction in the body's frame: x right (west), y up
        // (north), z toward us.
        let (i, pa) = (st.phase_deg.to_radians(), st.sun_pa_deg.to_radians());
        let s = (i.sin() * -pa.sin(), i.sin() * pa.cos(), i.cos());
        // Saturn's globe is a part of its rings' width.
        let globe = if name == "saturn" { 1.0 / 2.27 } else { 1.0 };
        let night = if name == "moon" { 0.04 } else { 0.0 };
        // Faint dots: one the size of a pixel or less keeps its light.
        let spread = if across < 1.0 { across * across } else { 1.0 };
        let (ox, oy) = ((N as f64 - tw as f64) / 2.0, (N as f64 - th as f64) / 2.0);
        for (bx, by, p) in body.enumerate_pixels() {
            let x = ((bx as f64 + 0.5) / tw as f64 * 2.0 - 1.0) / globe;
            let y = (1.0 - (by as f64 + 0.5) / th as f64 * 2.0) * (th as f64 / tw as f64) / globe;
            let rr = x * x + y * y;
            // Past the globe's edge is Saturn's rings, lit whole; on any
            // other body it is the edge itself, dark on the night side.
            let lit = if rr <= 1.0 || name != "saturn" {
                let z = (1.0 - rr).max(0.0).sqrt();
                let toward = x * s.0 + y * s.1 + z * s.2;
                night + (1.0 - night) * (toward * 5.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let tint = if sun { [1.0, 0.72, 0.38] } else { [1.0, 1.0, 1.0] };
            let (px, py) = ((ox + bx as f64) as u32, (oy + by as f64) as u32);
            if px < N && py < N {
                let o = out.get_pixel_mut(px, py);
                for c in 0..3 {
                    let v = (p[c] as f64 * lit * spread * tint[c]).round().min(255.0) as u8;
                    o[c] = o[c].max(v);
                }
            }
        }
    }
    // The air, or the telescope's own limit, whichever is coarser: two
    // arcseconds of seeing, or 116 / aperture (Dawes).
    let sharp = 2.0f64.max(116.0 / field.aperture.max(20.0));
    let sigma = sharp / (fov * 3600.0 / N as f64) / 2.355;
    if sigma > 0.3 {
        out = image::imageops::blur(&out, sigma as f32);
    }
    let (c, r) = (N as f64 / 2.0, N as f64 / 2.0);
    for (x, y, p) in out.enumerate_pixels_mut() {
        if (x as f64 + 0.5 - c).hypot(y as f64 + 0.5 - r) > r {
            *p = Rgb([0, 0, 0]);
        }
    }
    DynamicImage::ImageRgb8(out)
}

/// The picture turned by `flip` and fitted, whole, into a canvas of
/// `cols` by `rows` cells, in the middle on black.
fn fit(img: &DynamicImage, flip: Flip, cols: u16, rows: u16, cell: Option<(u16, u16)>) -> glow::Canvas {
    let mut canvas = glow::Canvas::sized(cols, rows, cell);
    let (pw, ph) = ((canvas.cell_w() * cols as f64) as u32, (canvas.cell_h() * rows as f64) as u32);
    let small = flip.apply(img).resize(pw.max(1), ph.max(1), FilterType::Triangle).to_rgb8();
    let (ox, oy) = ((pw - small.width()) / 2, (ph - small.height()) / 2);
    for (x, y, p) in small.enumerate_pixels() {
        canvas.put((ox + x) as usize, (oy + y) as usize, (p[0], p[1], p[2]));
    }
    canvas
}

/// Show `s` in a box in the middle of the screen until q, Esc or Enter:
/// its photo, or for a deep-sky object with `real` the eyepiece view.
/// `f` turns it, `r` swaps the two, `e` steps through the eyepiece set.
/// Gives back the turn and the choice, so the next object keeps them.
/// The sun, the moon and a planet get their own eyepiece view: a survey
/// plate cannot hold them, since they move across it.
pub fn show(s: Subject, mut flip: Flip, mut real: bool, bortle: f64) -> Result<(Flip, bool), String> {
    let mut display = glow::Display::new();
    if !display.supported() {
        return Err("this terminal shows no pictures".into());
    }
    let (cols, rows) = Crust::terminal_size();
    let w = (cols * 3 / 4).clamp(30, cols.saturating_sub(4).max(30));
    let h = (rows * 3 / 4).clamp(10, rows.saturating_sub(4).max(10));
    let mut popup = Popup::centered(w, h, 252, 234);
    popup.pane.wrap = false;
    let dso = match s {
        Subject::Dso(d) => Some(d),
        Subject::Body(..) => None,
    };
    let title = match dso {
        Some(d) if !d.name.is_empty() => format!(" {} {}", d.id, d.name),
        _ => format!(" {}", s.label()),
    };
    let text = |foot: &str| {
        let mut lines = vec![title.clone()];
        lines.extend(std::iter::repeat_n(String::new(), h.saturating_sub(2) as usize));
        lines.push(foot.to_string());
        lines.join("\n")
    };

    let mut fields = crate::sky::fields();
    if fields.is_empty() {
        fields.push(stand_in(s.size_deg()));
    }
    let mut pick = best_field(&fields, s.size_deg());
    let mut photo: Option<Result<DynamicImage, String>> = None;
    let mut plates: Vec<Option<Result<DynamicImage, String>>> = (0..fields.len()).map(|_| None).collect();
    let (x, y) = (popup.pane.x, popup.pane.y + 1);
    loop {
        let eyepiece = real;
        let loaded = if eyepiece {
            let f = &fields[pick];
            plates[pick].get_or_insert_with(|| match s {
                Subject::Dso(d) => {
                    popup.show(&text(" fetching the sky survey plate …"));
                    survey(d, f.radius_deg * 2.0)
                        .and_then(|b| read_fits(&b))
                        .map(|plate| eyepiece_view(&plate, f.aperture, bortle))
                }
                Subject::Body(name, st) => {
                    popup.show(&text(" fetching the picture …"));
                    let bytes = if name == "sun" { sun_today() } else { picture(&s) };
                    bytes
                        .and_then(|b| image::load_from_memory(&b).map_err(|e| format!("the picture of {} is unreadable: {e}", s.label())))
                        .map(|img| body_view(&img, name, st, f, bortle))
                }
            })
        } else {
            photo.get_or_insert_with(|| {
                popup.show(&text(" fetching the picture from Wikipedia …"));
                picture(&s).and_then(|b| image::load_from_memory(&b).map_err(|e| format!("the picture of {} is unreadable: {e}", s.label())))
            })
        };
        let credit = match s {
            Subject::Body("sun", _) => "only through a solar filter    SDO",
            Subject::Body(..) => "picture: Wikipedia",
            Subject::Dso(_) => "sky: DSS2 via CDS",
        };
        let foot = match (&loaded, eyepiece) {
            (Err(e), _) => format!(" {e}    r  {}    q  close", if real { "photo" } else { "eyepiece view" }),
            (Ok(_), true) => format!(
                " f  {}    r  photo    e  next eyepiece    q  close    {}    {credit}",
                flip.label(), field_note(&fields[pick])
            ),
            (Ok(_), false) => format!(" f  {}    r  eyepiece view    q  close    picture: Wikipedia", flip.label()),
        };
        popup.show(&text(&foot));
        match loaded {
            Ok(img) => { display.swap_canvas(&fit(img, flip, w, h.saturating_sub(2), None), x, y); }
            Err(_) => display.clear(x, y, w, h.saturating_sub(2), cols, rows),
        }
        match Input::getchr(None).as_deref() {
            Some("f") => flip = flip.next(),
            Some("r") => real = !real,
            Some("e") if eyepiece => pick = (pick + 1) % fields.len(),
            Some("q") | Some("ESC") | Some("ENTER") | None => break,
            _ => {}
        }
    }
    display.clear(popup.pane.x, popup.pane.y, w, h, cols, rows);
    popup.dismiss(&mut []);
    Ok((flip, real))
}

/// The eyepiece view of a body from its real picture, for tests that
/// write it out to look at.
#[cfg(test)]
pub fn body_view_for_test(name: &str, st: Stance, field: &Field) -> DynamicImage {
    let s = Subject::Body(name, st);
    let bytes = if name == "sun" { sun_today() } else { picture(&s) }.expect("fetched");
    body_view(&image::load_from_memory(&bytes).expect("decodes"), name, st, field, 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dso(id: &'static str) -> &'static Dso {
        starmap::dsos().iter().find(|d| d.id == id).unwrap()
    }

    #[test]
    fn a_body_has_its_own_article_and_its_own_shelf() {
        assert_eq!(body_name("jupiter"), "Jupiter");
        let st = Stance { size_deg: 0.01, phase_deg: 0.0, sun_pa_deg: 0.0 };
        assert_eq!(Subject::Body("mercury", st).titles(), ["Mercury (planet)"]);
        assert_eq!(Subject::Body("moon", st).titles(), ["Moon"]);
        assert!(cache_path(&Subject::Body("sun", st)).ends_with(".astro/images/body/sun.img"));
    }

    #[test]
    fn titles_start_with_the_catalogue_name() {
        assert_eq!(titles(dso("M31")), ["Messier 31", "NGC 224", "Andromeda Galaxy"]);
        assert_eq!(titles(dso("M2")), ["Messier 2", "NGC 7089"]);
        assert_eq!(titles(dso("C14")), ["Caldwell 14", "NGC 869", "Double Cluster"]);
        assert_eq!(encode("Double Cluster, h & χ"), "Double%20Cluster%2C%20h%20%26%20%CF%87");
    }

    #[test]
    fn the_turns_turn_the_right_way() {
        // Red at the top left, blue at the bottom left.
        let mut img = image::RgbImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        img.put_pixel(0, 1, image::Rgb([0, 0, 255]));
        let img = DynamicImage::ImageRgb8(img);
        let at = |f: Flip, x, y| f.apply(&img).to_rgb8().get_pixel(x, y).0;
        assert_eq!(at(Flip::Eye, 0, 0), [255, 0, 0]);
        assert_eq!(at(Flip::Telescope, 1, 1), [255, 0, 0], "south up: turned half round");
        assert_eq!(at(Flip::Diagonal, 1, 0), [255, 0, 0], "mirrored left to right");
        assert_eq!(Flip::Eye.next().next().next(), Flip::Eye);
    }

    /// Fetches from Wikipedia: `ASTRO_PHOTO_DUMP=dir cargo test -- --ignored photo`
    /// writes M31's picture, fitted and turned three ways, as PNGs.
    #[test]
    #[ignore]
    fn fetch_and_turn_a_real_picture() {
        let Ok(dir) = std::env::var("ASTRO_PHOTO_DUMP") else { return };
        let bytes = picture(&Subject::Dso(dso("M31"))).expect("fetched");
        let img = image::load_from_memory(&bytes).expect("decodes");
        for (name, f) in [("eye", Flip::Eye), ("telescope", Flip::Telescope), ("diagonal", Flip::Diagonal)] {
            let c = fit(&img, f, 60, 20, Some((11, 21)));
            std::fs::write(format!("{dir}/m31-{name}.png"), c.png()).unwrap();
        }
        assert!(cache_path(&Subject::Dso(dso("M31"))).exists(), "kept for next time");
    }

    /// Fetches from CDS: `ASTRO_PHOTO_DUMP=dir cargo test -- --ignored eyepiece`
    /// writes eyepiece views of a few objects through a 150 mm, 50× set.
    #[test]
    #[ignore]
    fn fetch_and_dim_real_plates() {
        let Ok(dir) = std::env::var("ASTRO_PHOTO_DUMP") else { return };
        for id in ["M51", "M13", "M42", "M57", "M31", "M1"] {
            let bytes = survey(dso(id), 2.0).expect("fetched");
            let plate = read_fits(&bytes).expect("reads");
            for ap in [80.0, 150.0, 300.0] {
                let view = eyepiece_view(&plate, ap, 4.0);
                view.save(format!("{dir}/{id}-{ap:.0}mm.png")).unwrap();
            }
        }
    }

    #[test]
    fn a_bigger_telescope_shows_more_and_the_field_is_round() {
        // A plate with a sky of 2000 and a smooth glow up to 24,000.
        let px = (0..200 * 200)
            .map(|k| { let x = k % 200; if x < 100 { 2000.0 } else { 2000.0 + (x - 100) as f32 * 220.0 } })
            .collect();
        let plate = Plate { w: 200, h: 200, px };
        let lit = |ap: f64| eyepiece_view(&plate, ap, 4.0).to_luma8().pixels().filter(|p| p[0] > 20).count();
        assert!(lit(300.0) > lit(80.0), "more of the glow shows in a bigger telescope");
        let v = eyepiece_view(&plate, 300.0, 4.0).to_luma8();
        assert_eq!(v.get_pixel(199, 0)[0], 0, "outside the field stop is black");
        assert!(v.pixels().all(|p| p[0] <= 200), "the eye never sees white");
    }

    /// A grey ball on black: the half toward the sun lit, the rest dark,
    /// at the size the field gives it.
    #[test]
    fn a_body_is_its_true_size_and_lit_from_the_sun() {
        let ball = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 200, |x, y| {
            let (dx, dy) = (x as f64 - 99.5, y as f64 - 99.5);
            if dx.hypot(dy) < 80.0 { Rgb([200, 200, 200]) } else { Rgb([0, 0, 0]) }
        }));
        let field = Field { label: String::new(), rgb: (0, 0, 0), radius_deg: 0.5, aperture: 150.0, power: 100.0 };
        // Half lit, the sun to the east: the left half of the disk.
        let st = Stance { size_deg: 0.5, phase_deg: 90.0, sun_pa_deg: 90.0 };
        let v = body_view(&ball, "moon", st, &field, 4.0).to_rgb8();
        let at = |x: u32, y: u32| v.get_pixel(x, y)[0];
        assert!(at(300, 400) > 150, "the east (left) half is lit");
        assert!(at(500, 400) < 40, "the west half is dark");
        assert!(at(300, 150) < 30 && at(300, 250) > 100, "half the field wide: 400 across of 800");
        assert_eq!(at(0, 0), 0, "outside the field stop");
        let (l, _, w, _) = find_disk(&ball.to_rgb8()).unwrap();
        assert!((18..=22).contains(&l) && (155..=162).contains(&w), "the disk found at {l}, {w} wide");
    }

    #[test]
    fn a_fits_plate_reads_with_the_bottom_row_last() {
        let mut f = Vec::new();
        for c in ["SIMPLE  =                    T", "BITPIX  =                   16", "NAXIS   =                    2",
                  "NAXIS1  =                    2", "NAXIS2  =                    2", "BZERO   =                32768", "END"] {
            f.extend(format!("{c:<80}").bytes());
        }
        f.resize(2880, b' ');
        // Stored bottom row first: 1 2, then 3 4; each less 32768.
        for v in [1i32, 2, 3, 4] {
            f.extend(((v - 32768) as i16).to_be_bytes());
        }
        let p = read_fits(&f).unwrap();
        assert_eq!((p.w, p.h), (2, 2));
        assert_eq!(p.px, [3.0, 4.0, 1.0, 2.0], "the top row comes first");
        assert!(read_fits(b"not a fits file").is_err());
    }

    #[test]
    fn the_eyepiece_that_frames_the_object_is_picked() {
        let f = |deg: f64| Field { label: String::new(), rgb: (0, 0, 0), radius_deg: deg / 2.0, aperture: 150.0, power: 50.0 };
        let set = [f(2.0), f(0.5), f(1.0)];
        // M57 is about 1.4 arcminutes: the narrowest field frames it.
        assert_eq!(best_field(&set, dso("M57").major / 60.0), 1);
        // M31 is three degrees: none holds it, so the widest.
        assert_eq!(best_field(&set, dso("M31").major / 60.0), 0);
    }

    #[test]
    fn a_picture_fits_whole_and_centred() {
        // A wide picture in a tall box: black bands above and below.
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(200, 100, image::Rgb([200, 200, 200])));
        let c = fit(&img, Flip::Eye, 10, 10, Some((10, 20)));
        let png = image::load_from_memory(&c.png()).unwrap().to_rgb8();
        assert_eq!(png.dimensions(), (100, 200));
        assert_eq!(png.get_pixel(50, 100).0, [200, 200, 200], "the middle is picture");
        assert_eq!(png.get_pixel(50, 5).0, [0, 0, 0], "the top is black");
        assert_eq!(png.get_pixel(50, 195).0, [0, 0, 0], "the bottom is black");
    }
}
