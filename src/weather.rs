// Weather fetching from met.no. Adapted from tock's weather.rs with a
// file-based cache and hourly detail retention.

use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct HourPoint {
    pub time: String,
    pub date: String,
    pub hour: i64,
    pub hour_str: String,       // "HH" zero-padded
    pub temp: f64,
    pub wind: f64,
    pub gust: f64,
    pub wind_dir: i64,
    pub wind_dir_name: String,
    pub cloud: i64,
    pub cloud_low: i64,
    pub cloud_high: i64,
    pub fog: f64,
    pub humidity: f64,
    pub dew_point: f64,
    pub pressure: f64,
    pub uv: f64,
    pub precip: f64,
    pub symbol: String,         // met.no symbol code like "partlycloudy_day"
}

pub fn wind_dir_name(deg: i64) -> &'static str {
    let idx = ((deg.rem_euclid(360)) / 45) as usize;
    match idx {
        0 => "N", 1 => "NE", 2 => "E", 3 => "SE",
        4 => "S", 5 => "SW", 6 => "W", 7 => "NW",
        _ => "N",
    }
}

#[derive(Debug, Clone)]
pub struct DayForecast {
    pub date: String,
    pub temp_high: f64,
    pub temp_low: f64,
    pub temp_mid: f64,
    pub wind: f64,
    pub cloud: i64,
    pub humidity: f64,
    pub symbol: String,
    pub hours: Vec<HourPoint>,
}

fn cache_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".astro/weather_cache.json")
}

const CACHE_TTL_SECS: u64 = 3 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn fetch_cached(lat: f64, lon: f64, force: bool) -> Vec<DayForecast> {
    if !force {
        if let Some(cached) = read_cache(lat, lon) {
            return cached;
        }
    }
    let fresh = fetch_weather(lat, lon);
    if !fresh.is_empty() {
        write_cache(lat, lon, &fresh);
    }
    fresh
}

fn read_cache(lat: f64, lon: f64) -> Option<Vec<DayForecast>> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    let v: JsonValue = serde_json::from_str(&raw).ok()?;
    let obj = v.as_object()?;
    let fetched: u64 = obj.get("fetched_at")?.as_u64()?;
    let clat = obj.get("lat")?.as_f64()?;
    let clon = obj.get("lon")?.as_f64()?;
    if (clat - lat).abs() > 0.01 || (clon - lon).abs() > 0.01 { return None; }
    if now_secs().saturating_sub(fetched) >= CACHE_TTL_SECS { return None; }
    let days = obj.get("days")?.as_array()?;
    let mut out = Vec::new();
    for d in days { out.push(parse_day(d)?); }
    Some(out)
}

fn write_cache(lat: f64, lon: f64, days: &[DayForecast]) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({
        "fetched_at": now_secs(),
        "lat": lat,
        "lon": lon,
        "days": days.iter().map(serialize_day).collect::<Vec<_>>(),
    });
    let _ = std::fs::write(&path, payload.to_string());
}

fn serialize_day(d: &DayForecast) -> JsonValue {
    serde_json::json!({
        "date": d.date,
        "temp_high": d.temp_high,
        "temp_low": d.temp_low,
        "temp_mid": d.temp_mid,
        "wind": d.wind,
        "cloud": d.cloud,
        "humidity": d.humidity,
        "symbol": d.symbol,
        "hours": d.hours.iter().map(|h| serde_json::json!({
            "time": h.time, "date": h.date, "hour": h.hour, "hour_str": h.hour_str,
            "temp": h.temp, "wind": h.wind, "gust": h.gust,
            "wind_dir": h.wind_dir, "wind_dir_name": h.wind_dir_name,
            "cloud": h.cloud, "cloud_low": h.cloud_low, "cloud_high": h.cloud_high,
            "fog": h.fog, "humidity": h.humidity,
            "dew_point": h.dew_point, "pressure": h.pressure,
            "uv": h.uv, "precip": h.precip, "symbol": h.symbol,
        })).collect::<Vec<_>>(),
    })
}

fn parse_day(v: &JsonValue) -> Option<DayForecast> {
    let obj = v.as_object()?;
    let hours_arr = obj.get("hours")?.as_array()?;
    let mut hours = Vec::with_capacity(hours_arr.len());
    for h in hours_arr {
        let ho = h.as_object()?;
        let hour: i64 = ho.get("hour")?.as_i64()?;
        hours.push(HourPoint {
            time: ho.get("time")?.as_str()?.to_string(),
            date: ho.get("date")?.as_str()?.to_string(),
            hour,
            hour_str: ho.get("hour_str").and_then(|v| v.as_str()).map(|s| s.to_string())
                .unwrap_or_else(|| format!("{:02}", hour)),
            temp: ho.get("temp")?.as_f64()?,
            wind: ho.get("wind")?.as_f64()?,
            gust: ho.get("gust")?.as_f64()?,
            wind_dir: ho.get("wind_dir").and_then(|v| v.as_i64()).unwrap_or(0),
            wind_dir_name: ho.get("wind_dir_name").and_then(|v| v.as_str()).unwrap_or("N").to_string(),
            cloud: ho.get("cloud")?.as_i64()?,
            cloud_low: ho.get("cloud_low").and_then(|v| v.as_i64()).unwrap_or(0),
            cloud_high: ho.get("cloud_high").and_then(|v| v.as_i64()).unwrap_or(0),
            fog: ho.get("fog")?.as_f64()?,
            humidity: ho.get("humidity")?.as_f64()?,
            dew_point: ho.get("dew_point")?.as_f64()?,
            pressure: ho.get("pressure")?.as_f64()?,
            uv: ho.get("uv")?.as_f64()?,
            precip: ho.get("precip")?.as_f64()?,
            symbol: ho.get("symbol")?.as_str()?.to_string(),
        });
    }
    Some(DayForecast {
        date: obj.get("date")?.as_str()?.to_string(),
        temp_high: obj.get("temp_high")?.as_f64()?,
        temp_low: obj.get("temp_low")?.as_f64()?,
        temp_mid: obj.get("temp_mid")?.as_f64()?,
        wind: obj.get("wind")?.as_f64()?,
        cloud: obj.get("cloud")?.as_i64()?,
        humidity: obj.get("humidity")?.as_f64()?,
        symbol: obj.get("symbol")?.as_str()?.to_string(),
        hours,
    })
}

pub fn fetch_weather(lat: f64, lon: f64) -> Vec<DayForecast> {
    let url = format!(
        "https://api.met.no/weatherapi/locationforecast/2.0/complete?lat={}&lon={}",
        lat, lon
    );

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(10))
        .build();

    let resp = match agent
        .get(&url)
        .set("User-Agent", "nova-astronomy/0.1 g@isene.com")
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let body: JsonValue = match resp.into_json() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let timeseries = match body.pointer("/properties/timeseries") {
        Some(JsonValue::Array(arr)) => arr,
        _ => return Vec::new(),
    };

    let mut by_date: std::collections::BTreeMap<String, Vec<HourPoint>> = std::collections::BTreeMap::new();

    for ts in timeseries {
        let time = match ts.get("time").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => continue,
        };
        let details = match ts.pointer("/data/instant/details") {
            Some(d) => d,
            None => continue,
        };
        let next_1h = ts.pointer("/data/next_1_hours");

        let date = time[..10].to_string();
        let hour: i64 = time[11..13].parse().unwrap_or(-1);

        let f = |k: &str| details.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);

        let cloud = f("cloud_area_fraction") as i64;
        let precip = next_1h
            .and_then(|n| n.pointer("/details/precipitation_amount"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let symbol = next_1h
            .and_then(|n| n.pointer("/summary/symbol_code"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "fair".into());

        let wind_dir = f("wind_from_direction") as i64;
        by_date.entry(date.clone()).or_default().push(HourPoint {
            time: time.clone(),
            date: date.clone(),
            hour,
            hour_str: format!("{:02}", hour),
            temp: f("air_temperature"),
            wind: f("wind_speed"),
            gust: f("wind_speed_of_gust"),
            wind_dir,
            wind_dir_name: wind_dir_name(wind_dir).to_string(),
            cloud,
            cloud_low: f("cloud_area_fraction_low") as i64,
            cloud_high: f("cloud_area_fraction_high") as i64,
            fog: f("fog_area_fraction"),
            humidity: f("relative_humidity"),
            dew_point: f("dew_point_temperature"),
            pressure: f("air_pressure_at_sea_level"),
            uv: f("ultraviolet_index_clear_sky"),
            precip,
            symbol,
        });
    }

    let mut days = Vec::new();
    for (date, hours) in by_date {
        if hours.is_empty() { continue; }
        let temps: Vec<f64> = hours.iter().map(|h| h.temp).collect();
        let temp_high = (temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 10.0).round() / 10.0;
        let temp_low = (temps.iter().cloned().fold(f64::INFINITY, f64::min) * 10.0).round() / 10.0;
        let mid_idx = hours.iter().position(|h| h.hour == 12).unwrap_or(hours.len() / 2);
        let midday = &hours[mid_idx];
        let humidity: f64 = hours.iter().map(|h| h.humidity).sum::<f64>() / hours.len() as f64;
        days.push(DayForecast {
            date,
            temp_high,
            temp_low,
            temp_mid: (midday.temp * 10.0).round() / 10.0,
            wind: midday.wind,
            cloud: midday.cloud,
            humidity: (humidity * 10.0).round() / 10.0,
            symbol: symbol_char_from_code(&midday.symbol).to_string(),
            hours,
        });
    }
    days
}

fn symbol_char_from_code(code: &str) -> &'static str {
    let base = code.split('_').next().unwrap_or(code);
    match base {
        "clearsky" | "fair" => "\u{2600}",
        "partlycloudy" => "\u{26C5}",
        "cloudy" => "\u{2601}",
        "fog" => "\u{1F32B}",
        "lightrain" | "lightrainshowers" => "\u{1F326}",
        "rain" | "rainshowers" | "heavyrain" | "heavyrainshowers" => "\u{1F327}",
        "snow" | "snowshowers" | "lightsnow" | "heavysnow" => "\u{1F328}",
        "sleet" | "lightsleet" | "heavysleet" => "\u{1F328}",
        "thunderstorm" | "heavyrainandthunder" => "\u{26C8}",
        _ => "\u{26C5}",
    }
}

/// Astropanel condition points. Higher = worse viewing.
/// 0-1 = green, 2-3 = yellow, 4+ = red.
pub fn condition_points(
    cloud: i64,
    humidity: f64,
    temp: f64,
    wind: f64,
    cloud_limit: i64,
    humidity_limit: f64,
    temp_limit: f64,
    wind_limit: f64,
) -> i32 {
    let mut p = 0;
    if cloud > cloud_limit { p += 2; }
    if (cloud as f64) > (100.0 - cloud_limit as f64) / 2.0 { p += 1; }
    if cloud > 90 { p += 1; }
    if humidity > humidity_limit { p += 1; }
    if temp < temp_limit { p += 1; }
    if temp < temp_limit - 7.0 { p += 1; }
    if wind > wind_limit { p += 1; }
    if wind > 2.0 * wind_limit { p += 1; }
    p as i32
}

pub fn condition_color(points: i32) -> u8 {
    if points >= 4 { 196 } else if points >= 2 { 226 } else { 46 }
}

#[allow(dead_code)]
pub fn hashmap_by_date(days: &[DayForecast]) -> HashMap<String, DayForecast> {
    days.iter().map(|d| (d.date.clone(), d.clone())).collect()
}
