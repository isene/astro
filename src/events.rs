//! Astronomical events from in-the-sky.org RSS feed.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Event {
    pub time: String,       // "HH:MM:SS"
    pub event: String,
    pub link: String,
}

pub fn fetch_events(lat: f64, lon: f64, loc: &str) -> HashMap<String, Event> {
    let url = format!(
        "https://in-the-sky.org/rss.php?feed=dfan&latitude={}&longitude={}&timezone={}",
        lat, lon, loc
    );
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(10))
        .build();
    let body = match agent.get(&url)
        .set("User-Agent", "nova-astronomy/0.1 g@isene.com")
        .call()
    {
        Ok(r) => r.into_string().unwrap_or_default(),
        Err(_) => return HashMap::new(),
    };
    parse_rss(&body)
}

fn parse_rss(body: &str) -> HashMap<String, Event> {
    let mut out = HashMap::new();
    let items: Vec<&str> = body.split("<item>").skip(1).collect();
    for item in items {
        let end = item.find("</item>").unwrap_or(item.len());
        let item = &item[..end];

        let title = extract(item, "<title>", "</title>").unwrap_or_default();
        if title.is_empty() { continue; }

        // Title format from in-the-sky.org: "23 Apr 2026 (3 days away): ..."
        let tokens: Vec<&str> = title.split_whitespace().collect();
        if tokens.len() < 3 { continue; }
        let Some(date) = parse_day_month_year(tokens[0], tokens[1], tokens[2]) else {
            continue;
        };

        // Time from <pubDate> if present, fallback to any HH:MM:SS in item.
        let time = extract(item, "<pubDate>", "</pubDate>")
            .and_then(|s| extract_regex_time(&s))
            .or_else(|| extract_regex_time(item))
            .unwrap_or_default();

        let desc = extract(item, "<description>&lt;p&gt;", "&lt;/p&gt;")
            .unwrap_or_default();
        let event = decode_html_entities(&desc);
        let link = extract(item, "<link>", "</link>").unwrap_or_default();

        out.insert(date, Event { time, event, link });
    }
    out
}

fn parse_day_month_year(day: &str, month: &str, year: &str) -> Option<String> {
    let d: u32 = day.parse().ok()?;
    let m = match month {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4,
        "May" => 5, "Jun" => 6, "Jul" => 7, "Aug" => 8,
        "Sep" => 9, "Oct" => 10, "Nov" => 11, "Dec" => 12,
        _ => return None,
    };
    let y: i32 = year.parse().ok()?;
    Some(format!("{:04}-{:02}-{:02}", y, m, d))
}

fn extract(s: &str, start: &str, end: &str) -> Option<String> {
    let i = s.find(start)?;
    let tail = &s[i + start.len()..];
    let j = tail.find(end)?;
    Some(tail[..j].to_string())
}

fn extract_regex_time(s: &str) -> Option<String> {
    // Find first "HH:MM:SS" in s
    let b = s.as_bytes();
    let mut i = 0;
    while i + 8 <= b.len() {
        if b[i].is_ascii_digit() && b[i + 1].is_ascii_digit() && b[i + 2] == b':'
            && b[i + 3].is_ascii_digit() && b[i + 4].is_ascii_digit() && b[i + 5] == b':'
            && b[i + 6].is_ascii_digit() && b[i + 7].is_ascii_digit()
        {
            return Some(s[i..i + 8].to_string());
        }
        i += 1;
    }
    None
}

fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(semi) = s[i..].find(';') {
                let entity = &s[i + 1..i + semi];
                match entity {
                    "amp" => { out.push('&'); i += semi + 1; continue; }
                    "lt" => { out.push('<'); i += semi + 1; continue; }
                    "gt" => { out.push('>'); i += semi + 1; continue; }
                    "quot" => { out.push('"'); i += semi + 1; continue; }
                    "apos" | "#39" => { out.push('\''); i += semi + 1; continue; }
                    _ => {}
                }
                if let Some(num) = entity.strip_prefix('#') {
                    if let Ok(code) = num.parse::<u32>() {
                        if let Some(c) = char::from_u32(code) {
                            out.push(c);
                            i += semi + 1;
                            continue;
                        }
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
