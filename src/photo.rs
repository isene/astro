//! A deep-sky object's picture, the lead image of its Wikipedia article,
//! in a box in the middle of the screen. `f` turns it the way the naked
//! eye, a telescope (south up) or a star diagonal (mirrored) shows the
//! object, as in the moon app.
//!
//! Each picture is fetched once, at 1200 pixels wide, and kept in
//! ~/.astro/images/dso/, so looking again costs no network.

use crust::{Crust, Input, Popup};
use image::{imageops::FilterType, DynamicImage};
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

fn cache_path(d: &Dso) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".astro/images/dso").join(format!("{}.img", d.id))
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
fn picture(d: &Dso) -> Result<Vec<u8>, String> {
    let path = cache_path(d);
    if let Ok(b) = std::fs::read(&path) {
        if !b.is_empty() {
            return Ok(b);
        }
    }
    let url = titles(d).iter().find_map(|t| image_url(t))
        .ok_or_else(|| format!("Wikipedia has no picture of {} (or could not be reached)", d.id))?;
    let mut bytes = Vec::new();
    ureq::get(&url).set("User-Agent", AGENT).timeout(Duration::from_secs(30)).call()
        .map_err(|e| format!("the picture of {} would not download: {e}", d.id))?
        .into_reader().take(20 << 20).read_to_end(&mut bytes)
        .map_err(|e| format!("the picture of {} broke off: {e}", d.id))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes)
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

/// Show `d`'s picture in a box in the middle of the screen until q, Esc
/// or Enter; `f` turns it. Gives back the turn, so the next picture
/// keeps it.
pub fn show(d: &Dso, mut flip: Flip) -> Result<Flip, String> {
    let mut display = glow::Display::new();
    if !display.supported() {
        return Err("this terminal shows no pictures".into());
    }
    let (cols, rows) = Crust::terminal_size();
    let w = (cols * 3 / 4).clamp(30, cols.saturating_sub(4).max(30));
    let h = (rows * 3 / 4).clamp(10, rows.saturating_sub(4).max(10));
    let mut popup = Popup::centered(w, h, 252, 234);
    popup.pane.wrap = false;
    let title = if d.name.is_empty() { format!(" {}", d.id) } else { format!(" {} {}", d.id, d.name) };
    let text = |foot: &str| {
        let mut lines = vec![title.clone()];
        lines.extend(std::iter::repeat_n(String::new(), h.saturating_sub(2) as usize));
        lines.push(foot.to_string());
        lines.join("\n")
    };
    popup.show(&text(" fetching the picture from Wikipedia …"));

    let result = picture(d).and_then(|bytes| {
        image::load_from_memory(&bytes).map_err(|e| format!("the picture of {} is unreadable: {e}", d.id))
    });
    let img = match result {
        Ok(i) => i,
        Err(e) => {
            popup.dismiss(&mut []);
            return Err(e);
        }
    };
    let (x, y) = (popup.pane.x, popup.pane.y + 1);
    loop {
        popup.show(&text(&format!(" f  {}    q  close    picture: Wikipedia", flip.label())));
        display.swap_canvas(&fit(&img, flip, w, h.saturating_sub(2), None), x, y);
        match Input::getchr(None).as_deref() {
            Some("f") => flip = flip.next(),
            Some("q") | Some("ESC") | Some("ENTER") | None => break,
            _ => {}
        }
    }
    display.clear(popup.pane.x, popup.pane.y, w, h, cols, rows);
    popup.dismiss(&mut []);
    Ok(flip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dso(id: &'static str) -> &'static Dso {
        starmap::dsos().iter().find(|d| d.id == id).unwrap()
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
        let bytes = picture(dso("M31")).expect("fetched");
        let img = image::load_from_memory(&bytes).expect("decodes");
        for (name, f) in [("eye", Flip::Eye), ("telescope", Flip::Telescope), ("diagonal", Flip::Diagonal)] {
            let c = fit(&img, f, 60, 20, Some((11, 21)));
            std::fs::write(format!("{dir}/m31-{name}.png"), c.png()).unwrap();
        }
        assert!(cache_path(dso("M31")).exists(), "kept for next time");
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
