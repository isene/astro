/// Minimal date helpers, copied from tock. Self-contained: no external deps.

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn local_tz_offset_secs() -> i64 {
    unsafe {
        let now = now_secs() as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm.tm_gmtoff as i64
    }
}

pub fn today() -> (i32, u32, u32) {
    let local = now_secs() + local_tz_offset_secs();
    let (y, m, d, _, _, _) = ts_to_parts(local);
    (y, m, d)
}

pub fn date_to_ts(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 { month + 9 } else { month - 3 } as i64;
    let d = day as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64
}

pub fn ts_to_parts(ts: i64) -> (i32, u32, u32, u32, u32, u32) {
    let secs = ts.rem_euclid(86400);
    let days = ts.div_euclid(86400);
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
    (
        y as i32, m as u32, d as u32,
        (secs / 3600) as u32,
        ((secs % 3600) / 60) as u32,
        (secs % 60) as u32,
    )
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap(year) { 29 } else { 28 },
        _ => 0,
    }
}

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    // Zeller's congruence, returning 1=Mon..7=Sun
    let (y, m) = if month < 3 { (year - 1, month + 12) } else { (year, month) };
    let k = y.rem_euclid(100);
    let j = y.div_euclid(100);
    let h = ((day as i32) + (13 * ((m as i32) + 1) / 5) + k + k/4 + j/4 + 5*j).rem_euclid(7);
    // Zeller: 0=Sat,1=Sun,2=Mon... convert to 1=Mon..7=Sun
    match h { 0 => 6, 1 => 7, n => (n - 1) as u32 }
}

pub fn weekday_short(wd: u32) -> &'static str {
    match wd {
        1 => "Mon", 2 => "Tue", 3 => "Wed", 4 => "Thu",
        5 => "Fri", 6 => "Sat", 7 => "Sun", _ => "???",
    }
}

pub fn month_short(m: u32) -> &'static str {
    match m {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "?",
    }
}

pub fn add_days(date: (i32, u32, u32), n: i32) -> (i32, u32, u32) {
    let ts = date_to_ts(date.0, date.1, date.2, 0, 0, 0) + (n as i64) * 86400;
    let (y, m, d, _, _, _) = ts_to_parts(ts);
    (y, m, d)
}

pub fn format_ymd(date: (i32, u32, u32)) -> String {
    format!("{:04}-{:02}-{:02}", date.0, date.1, date.2)
}
