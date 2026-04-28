//! Image fetchers: NASA APOD and Stelvision starchart.

use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".astro/images")
}

fn ensure_cache_dir() -> PathBuf {
    let d = cache_dir();
    let _ = std::fs::create_dir_all(&d);
    d
}

fn download(url: &str, dest: &std::path::Path) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(30))
        .redirects(5)
        .build();
    let Ok(resp) = agent.get(url)
        .set("User-Agent", "nova-astronomy/0.1 g@isene.com")
        .call()
    else { return false };
    let mut bytes = Vec::new();
    if std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes).is_err() {
        return false;
    }
    if bytes.len() < 100 { return false; }
    std::fs::write(dest, &bytes).is_ok()
}

/// Fetch today's APOD (Astronomy Picture Of the Day) from NASA.
/// Cached per UTC date at ~/.astro/images/apod_YYYY-MM-DD.jpg so the image
/// is only downloaded once per day.
pub fn fetch_apod() -> Option<PathBuf> {
    let dir = ensure_cache_dir();
    let today = today_utc();
    let dest = dir.join(format!("apod_{}.jpg", today));

    // Cache hit: return existing file if it's valid (size > 100 bytes).
    if dest.exists() {
        if let Ok(meta) = std::fs::metadata(&dest) {
            if meta.len() > 100 { return Some(dest); }
        }
    }

    // Also update the "latest" symlink convenience path.
    let html_url = "https://apod.nasa.gov/apod/astropix.html";
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(15))
        .build();
    let html = agent.get(html_url)
        .set("User-Agent", "nova-astronomy/0.1 g@isene.com")
        .call().ok()?
        .into_string().ok()?;

    let src = extract_between(&html, "IMG SRC=\"", "\"")
        .or_else(|| extract_between(&html, "img src=\"", "\""))?;

    let full_url = if src.starts_with("http") {
        src
    } else {
        format!("https://apod.nasa.gov/apod/{}", src)
    };

    if download(&full_url, &dest) {
        // Clean up yesterday's cache entries to avoid disk buildup.
        cleanup_old_apod(&dir, &today);
        Some(dest)
    } else {
        None
    }
}

/// Cache-only APOD lookup for today. Returns path if the file already
/// exists (no network access).
pub fn apod_cached() -> Option<PathBuf> {
    let dir = cache_dir();
    let dest = dir.join(format!("apod_{}.jpg", today_utc()));
    if dest.exists() {
        if let Ok(m) = std::fs::metadata(&dest) {
            if m.len() > 100 { return Some(dest); }
        }
    }
    None
}

/// Cache-only starchart lookup for the given parameters.
pub fn starchart_cached(year: i32, month: u32, day: u32, hour: u32,
    lat: f64, lon: f64, tz: f64) -> Option<PathBuf> {
    let dir = cache_dir();
    let stem = format!(
        "starchart_{:04}{:02}{:02}_{:02}_{:.2}_{:.2}_{}",
        year, month, day, hour, lat, lon, tz as i32
    );
    for ext in &["jpg", "png"] {
        let p = dir.join(format!("{}.{}", stem, ext));
        if p.exists() {
            if let Ok(m) = std::fs::metadata(&p) {
                if m.len() > 100 { return Some(p); }
            }
        }
    }
    None
}

fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Days since 1970-01-01 (Hinnant)
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn cleanup_old_apod(dir: &std::path::Path, keep_date: &str) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("apod_") && name.ends_with(".jpg")
                && !name.contains(keep_date)
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Fetch starchart PNG from Stelvision for the given date/time and location.
/// Cached per (date, hour, lat, lon, tz) tuple so repeated views of the same
/// slot are instant. Converts to JPG via ImageMagick's `convert` when
/// available for better terminal rendering.
pub fn fetch_starchart(year: i32, month: u32, day: u32, hour: u32,
    lat: f64, lon: f64, tz: f64) -> Option<PathBuf> {
    let dir = ensure_cache_dir();
    let stem = format!(
        "starchart_{:04}{:02}{:02}_{:02}_{:.2}_{:.2}_{}",
        year, month, day, hour, lat, lon, tz as i32
    );
    let jpg = dir.join(format!("{}.jpg", stem));
    let png = dir.join(format!("{}.png", stem));

    // Cache hit.
    if jpg.exists() {
        if let Ok(m) = std::fs::metadata(&jpg) {
            if m.len() > 100 { return Some(jpg); }
        }
    }
    if png.exists() {
        if let Ok(m) = std::fs::metadata(&png) {
            if m.len() > 100 { return Some(png); }
        }
    }

    let url = format!(
        "https://www.stelvision.com/carte-ciel/visu_carte.php?stelmarq=C&mode_affichage=normal&req=stel&date_j_carte={:02}&date_m_carte={:02}&date_a_carte={:04}&heure_h={}&heure_m=00&longi={}&lat={}&tzone={}.0&dst_offset=1&taille_carte=1200&fond_r=255&fond_v=255&fond_b=255&lang=en",
        day, month, year, hour, lon, lat, tz as i32
    );
    if !download(&url, &png) { return None; }

    let ok = std::process::Command::new("convert")
        .arg(&png).arg(&jpg)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { Some(jpg) } else { Some(png) }
}

/// Remove all cached images older than the given date. Useful for long-lived
/// sessions to bound disk usage.
pub fn cleanup_cache() {
    let dir = cache_dir();
    if !dir.exists() { return; }
    // Limit starchart cache to ~50 entries.
    let mut entries: Vec<_> = std::fs::read_dir(&dir).ok()
        .map(|iter| iter.flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("starchart_"))
            .collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    if entries.len() > 50 {
        for e in entries.iter().take(entries.len() - 50) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn extract_between(hay: &str, start: &str, end: &str) -> Option<String> {
    let i = hay.find(start)?;
    let tail = &hay[i + start.len()..];
    let j = tail.find(end)?;
    Some(tail[..j].to_string())
}
