//! A night at the telescope: what to look at, when each target is best,
//! which combo frames it, and what was seen.
//!
//! One night per file in ~/.astro/nights/: `<date>.json` holds it, and
//! `<date>.hl` is the same night as a HyperList log for scribe, written
//! again on every change. A night ends at 08:00: 02:00 belongs to the
//! evening before, and 10:00 plans the coming evening.

use crate::sky::{describe, Field};
use crust::style;
use crust::{Crust, Cursor, Input, Pane};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// How high an object must stand to be worth the look, in degrees.
const USEFUL_ALT: f64 = 20.0;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Seen {
    /// Local time, `HH:MM`.
    pub time: String,
    pub combo: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Target {
    /// `M31`, `C14`.
    pub id: String,
    #[serde(default)]
    pub seen: Option<Seen>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Night {
    /// The evening's date, `YYYY-MM-DD`.
    pub date: String,
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
    pub targets: Vec<Target>,
}

fn nights_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".astro").join("nights")
}

/// The evening a moment belongs to: before 08:00 is still the night
/// before; from then on it is the coming evening.
pub fn night_of(year: i32, month: u32, day: u32, hour: u32) -> (i32, u32, u32) {
    if hour >= 8 {
        return (year, month, day);
    }
    let jd = julian(year, month, day, 12.0) - 1.0;
    civil(jd)
}

impl Night {
    /// The night on disk, or a fresh one for that date and place.
    pub fn load(date: (i32, u32, u32), place: &str, lat: f64, lon: f64, tz: f64) -> Night {
        let date = format!("{:04}-{:02}-{:02}", date.0, date.1, date.2);
        std::fs::read_to_string(nights_dir().join(format!("{date}.json")))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Night { date, place: place.into(), lat, lon, tz, targets: Vec::new() })
    }

    /// Write the night, and its HyperList log beside it.
    pub fn save(&self, set: &[Field]) {
        let dir = nights_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join(format!("{}.json", self.date)), t);
        }
        let _ = std::fs::write(dir.join(format!("{}.hl", self.date)), self.hyperlist(set));
    }

    /// Add an object once; says whether it was new.
    pub fn add(&mut self, id: &str) -> bool {
        if self.targets.iter().any(|t| t.id == id) {
            return false;
        }
        self.targets.push(Target { id: id.into(), seen: None });
        true
    }

    fn ymd(&self) -> (i32, u32, u32) {
        let p: Vec<u32> = self.date.split('-').filter_map(|x| x.parse().ok()).collect();
        (p.first().copied().unwrap_or(2000) as i32, p.get(1).copied().unwrap_or(1), p.get(2).copied().unwrap_or(1))
    }

    /// When an object stands highest this night (18:00 to 06:00), how
    /// high, and the stretch it is above `USEFUL_ALT`, all in local time.
    pub fn when(&self, d: &starmap::Dso) -> Visibility {
        let (y, m, day) = self.ymd();
        let start = julian(y, m, day, 18.0 - self.tz);
        let mut best = (f64::MIN, 0.0);
        let (mut from, mut to) = (None, None);
        // Every five minutes over twelve hours: 145 cheap sums.
        for i in 0..=144 {
            let hours = i as f64 / 12.0;
            let lst = starmap::lst_deg(start + hours / 24.0, self.lon);
            let (alt, _) = starmap::altaz(d.ra, d.dec, lst, self.lat);
            if alt > best.0 {
                best = (alt, hours);
            }
            if alt >= USEFUL_ALT {
                from.get_or_insert(hours);
                to = Some(hours);
            }
        }
        Visibility { best_alt: best.0, best_at: 18.0 + best.1, from: from.map(|h| 18.0 + h), to: to.map(|h| 18.0 + h) }
    }

    fn hyperlist(&self, set: &[Field]) -> String {
        let mut out = format!(
            "Night of {} at {} ({:.1}°{}, {:.1}°{})\n",
            self.date, self.place, self.lat.abs(), if self.lat >= 0.0 { "N" } else { "S" },
            self.lon.abs(), if self.lon >= 0.0 { "E" } else { "W" }
        );
        out.push_str("\t[Written by astro on every change; notes go in through astro]\n");
        out.push_str("\tPlan\n");
        for t in &self.targets {
            let Some(d) = starmap::dsos().iter().find(|d| d.id == t.id) else { continue };
            let v = self.when(d);
            out.push_str(&format!("\t\t{}\n", describe(d)));
            out.push_str(&format!("\t\t\t{}\n", v.line()));
            if let Some(f) = best_field(d, set) {
                out.push_str(&format!("\t\t\tBest combo: {}\n", f.label));
            }
        }
        let seen: Vec<&Target> = self.targets.iter().filter(|t| t.seen.is_some()).collect();
        if !seen.is_empty() {
            out.push_str("\tSeen\n");
            for t in seen {
                let s = t.seen.as_ref().unwrap();
                let name = starmap::dsos().iter().find(|d| d.id == t.id).map(|d| d.label()).unwrap_or_else(|| t.id.clone());
                out.push_str(&format!("\t\t{} {}\n", s.time, name));
                if !s.combo.is_empty() {
                    out.push_str(&format!("\t\t\tWith: {}\n", s.combo));
                }
                if !s.note.is_empty() {
                    out.push_str(&format!("\t\t\t{}\n", s.note));
                }
            }
        }
        out
    }
}

/// An object's night: the best moment, and the stretch it is well up.
pub struct Visibility {
    pub best_alt: f64,
    /// Local hours from 18 (18.0 = 18:00, 26.5 = 02:30).
    pub best_at: f64,
    pub from: Option<f64>,
    pub to: Option<f64>,
}

impl Visibility {
    pub fn line(&self) -> String {
        if self.best_alt < USEFUL_ALT {
            return format!("never above {USEFUL_ALT:.0}° tonight (highest {:.0}° at {})", self.best_alt, clock(self.best_at));
        }
        let span = match (self.from, self.to) {
            (Some(a), Some(b)) => format!("above {USEFUL_ALT:.0}° {}–{}", clock(a), clock(b)),
            _ => String::new(),
        };
        format!("best {} at {:.0}° · {}", clock(self.best_at), self.best_alt, span)
    }
}

fn clock(h: f64) -> String {
    let m = (h * 60.0).round() as i64;
    format!("{:02}:{:02}", (m / 60) % 24, m % 60)
}

/// The combo that frames an object best: the tightest field that still
/// holds it with room round it, else the widest there is.
pub fn best_field<'a>(d: &starmap::Dso, set: &'a [Field]) -> Option<&'a Field> {
    let size = (d.major / 60.0).max(0.05);
    set.iter()
        .filter(|f| f.radius_deg * 2.0 >= size * 1.5)
        .min_by(|a, b| a.radius_deg.total_cmp(&b.radius_deg))
        .or_else(|| set.iter().max_by(|a, b| a.radius_deg.total_cmp(&b.radius_deg)))
}

/// Julian date of a UT calendar date and hour.
fn julian(y: i32, m: u32, d: u32, hour_ut: f64) -> f64 {
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor() + (30.6001 * (m as f64 + 1.0)).floor() + d as f64 + b - 1524.5 + hour_ut / 24.0
}

/// Calendar date of a Julian date (noon of that day).
fn civil(jd: f64) -> (i32, u32, u32) {
    let z = (jd + 0.5).floor();
    let a = ((z - 1867216.25) / 36524.25).floor();
    let a = z + 1.0 + a - (a / 4.0).floor();
    let b = a + 1524.0;
    let c = ((b - 122.1) / 365.25).floor();
    let d = (365.25 * c).floor();
    let e = ((b - d) / 30.6001).floor();
    let day = (b - d - (30.6001 * e).floor()) as u32;
    let month = if e < 14.0 { e - 1.0 } else { e - 13.0 } as u32;
    let year = if month > 2 { c - 4716.0 } else { c - 4715.0 } as i32;
    (year, month, day)
}

/// The plan, full screen. Returns the object to go to in the chart, if
/// Enter picked one.
pub fn run(night: &mut Night, set: &[Field]) -> Option<&'static starmap::Dso> {
    let mut sel = 0usize;
    let mut note = String::new();
    loop {
        let (cols, rows) = Crust::terminal_size();
        draw(night, set, sel, &note, cols, rows);
        note.clear();
        let Some(key) = Input::getchr(None) else { continue };
        let n = night.targets.len();
        match key.as_str() {
            "q" | "ESC" | "p" => return None,
            "j" | "DOWN" => sel = (sel + 1).min(n.saturating_sub(1)),
            "k" | "UP" => sel = sel.saturating_sub(1),
            "ENTER" => {
                let id = night.targets.get(sel)?.id.clone();
                return starmap::dsos().iter().find(|d| d.id == id);
            }
            "x" | "D" => {
                if sel < n {
                    let t = night.targets.remove(sel);
                    sel = sel.min(night.targets.len().saturating_sub(1));
                    night.save(set);
                    note = format!("{} left the plan", t.id);
                }
            }
            "o" => {
                let Some(t) = night.targets.get(sel).cloned() else { continue };
                let d = starmap::dsos().iter().find(|d| d.id == t.id);
                let best = d.and_then(|d| best_field(d, set));
                let hint = set.iter().enumerate().map(|(i, f)| format!("{} {}", i + 1, short(&f.label))).collect::<Vec<_>>().join(" · ");
                let combo = if set.is_empty() {
                    String::new()
                } else {
                    let pick = ask(&format!(" Combo ({hint}; Enter = {}): ", best.map(|f| short(&f.label)).unwrap_or_default()), "");
                    match pick.trim().parse::<usize>() {
                        Ok(i) if (1..=set.len()).contains(&i) => set[i - 1].label.clone(),
                        _ => best.map(|f| f.label.clone()).unwrap_or_default(),
                    }
                };
                let text = ask(&format!(" What you saw of {}: ", t.id), &t.seen.as_ref().map(|s| s.note.clone()).unwrap_or_default());
                let now = local_clock(night.tz);
                night.targets[sel].seen = Some(Seen { time: now, combo, note: text.trim().to_string() });
                night.save(set);
                note = format!("{} marked seen", t.id);
            }
            "u" => {
                if let Some(t) = night.targets.get_mut(sel) {
                    t.seen = None;
                    note = format!("{} no longer marked seen", t.id);
                    night.save(set);
                }
            }
            _ => {}
        }
    }
}

fn short(label: &str) -> String {
    label.split("  ").next().unwrap_or(label).to_string()
}

fn ask(prompt: &str, init: &str) -> String {
    let (cols, rows) = Crust::terminal_size();
    let mut p = Pane::new(1, rows, cols, 1, 255, 236);
    p.scroll = false;
    Cursor::show();
    let a = p.ask_or_cancel(prompt, init).unwrap_or_default();
    Cursor::hide();
    a
}

/// The wall clock now, `HH:MM`, at the night's time zone.
fn local_clock(tz: f64) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    clock((secs / 3600.0 + tz).rem_euclid(24.0))
}

fn draw(night: &Night, set: &[Field], sel: usize, note: &str, cols: u16, rows: u16) {
    let mut out = String::new();
    Crust::clear_screen();
    let seen = night.targets.iter().filter(|t| t.seen.is_some()).count();
    let title = format!(" The night of {} at {} · {} targets, {} seen", night.date, night.place, night.targets.len(), seen);
    out.push_str(&Cursor::at(1, 1));
    out.push_str(&crate::sky::bar(&title, cols, (255, 200, 120), "b"));
    let mut y = 3u16;
    if night.targets.is_empty() {
        out.push_str(&Cursor::at(3, y));
        out.push_str(&style::dim("No targets yet. In the sky chart, put the crosshair on an object (/ goes to one) and press a."));
    }
    for (i, t) in night.targets.iter().enumerate() {
        if y + 3 >= rows {
            break;
        }
        let Some(d) = starmap::dsos().iter().find(|d| d.id == t.id) else { continue };
        let mark = if t.seen.is_some() { style::rgb("✓", Some((120, 230, 140)), None, "b") } else { " ".into() };
        let head = format!("{} {}", mark, describe(d));
        let head = if i == sel { style::rgb(&format!("▸{head}"), Some((255, 255, 255)), Some((52, 48, 60)), "b") } else { format!(" {head}") };
        out.push_str(&Cursor::at(2, y));
        out.push_str(&crust::truncate_ansi(&head, cols as usize - 2));
        y += 1;
        let mut sub = night.when(d).line();
        if let Some(f) = best_field(d, set) {
            sub.push_str(&format!(" · best combo {}", short(&f.label)));
        }
        out.push_str(&Cursor::at(6, y));
        out.push_str(&style::rgb(&crust::truncate_ansi(&sub, cols as usize - 6), Some((150, 150, 160)), None, ""));
        y += 1;
        if let Some(s) = &t.seen {
            let line = format!("seen {} with {}{}", s.time, short(&s.combo), if s.note.is_empty() { String::new() } else { format!(": {}", s.note) });
            out.push_str(&Cursor::at(6, y));
            out.push_str(&style::rgb(&crust::truncate_ansi(&line, cols as usize - 6), Some((120, 230, 140)), None, ""));
            y += 1;
        }
        y += 1;
    }
    if !note.is_empty() {
        out.push_str(&Cursor::at(1, rows.saturating_sub(1)));
        out.push_str(&style::rgb(&format!(" {note}"), Some((230, 230, 235)), None, ""));
    }
    out.push_str(&Cursor::at(1, rows));
    out.push_str(&crate::sky::bar(
        &format!(" j/k move · o seen · u not seen · x remove · ⏎ go to it · q back   log: ~/.astro/nights/{}.hl", night.date),
        cols,
        (215, 215, 220),
        "",
    ));
    print!("{out}");
    std::io::stdout().flush().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_hour_belongs_to_the_evening_before() {
        assert_eq!(night_of(2026, 9, 26, 2), (2026, 9, 25));
        assert_eq!(night_of(2026, 3, 1, 3), (2026, 2, 28));
        assert_eq!(night_of(2026, 9, 25, 22), (2026, 9, 25));
        assert_eq!(night_of(2026, 9, 25, 10), (2026, 9, 25), "planning in the morning is for tonight");
    }

    #[test]
    fn andromeda_rides_high_over_oslo_in_late_september() {
        let n = Night { date: "2026-09-25".into(), place: "Oslo".into(), lat: 59.9, lon: 10.7, tz: 2.0, targets: vec![] };
        let m31 = starmap::dsos().iter().find(|d| d.id == "M31").unwrap();
        let v = n.when(m31);
        // M31 crosses the meridian near 01:00 local in late September,
        // at 90 − (59.9 − 41.3) ≈ 71°.
        assert!((v.best_alt - 71.4).abs() < 1.0, "best alt {}", v.best_alt);
        assert!((24.0..26.5).contains(&v.best_at), "best at {}", clock(v.best_at));
        assert!(v.from.is_some(), "it is well up all night");
    }

    #[test]
    fn the_best_combo_frames_with_room() {
        let f = |r: f64| Field { label: format!("{r}"), rgb: (0, 0, 0), radius_deg: r };
        let set = vec![f(0.4), f(0.78), f(0.97)];
        let m57 = starmap::dsos().iter().find(|d| d.id == "M57").unwrap();
        assert_eq!(best_field(m57, &set).unwrap().label, "0.4", "a small object gets the tightest field");
        let m31 = starmap::dsos().iter().find(|d| d.id == "M31").unwrap();
        assert_eq!(best_field(m31, &set).unwrap().label, "0.97", "too big for all: the widest");
    }
}
