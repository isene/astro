
mod config;
mod date_util;
mod events;
mod gear;
mod images;
mod weather;

use crust::{Crust, Input, Pane};
use crust::style;
use config::Config;
use std::collections::HashMap;
use std::sync::mpsc;
use weather::{DayForecast, HourPoint};

fn main() {
    config::ensure_dir();
    // Config::load() creates the file with defaults if missing. Detect
    // first run by checking existence BEFORE load.
    let first_run = !config::config_path().exists();
    let mut cfg = Config::load();
    if first_run {
        cfg = first_run_setup(cfg);
        cfg.save();
    }

    Crust::init();
    let mut app = App::new(cfg);
    app.fetch_all();
    app.render_all();
    app.auto_image();
    if app.current_image.is_some() {
        app.refresh_image();
    }

    loop {
        let Some(key) = Input::getchr(Some(1)) else {
            // Idle tick: poll any in-flight async fetches.
            if app.poll_async() {
                app.render_all();
                if app.current_image.is_some() {
                    app.refresh_image();
                }
            }
            app.tick();
            continue;
        };
        // Also poll before handling the key (pick up any newly-ready data).
        if app.poll_async() {
            app.render_all();
            if app.current_image.is_some() {
                app.refresh_image();
            }
        }
        match key.as_str() {
            "q" | "Q" => break,
            "g" | "G" => {
                // Switch to Gear mode. gear::run() takes over the
                // alt screen and runs its own loop. When it returns
                // true the user wants to quit astro entirely;
                // otherwise we resume Sky mode.
                app.clear_image();
                if gear::run() {
                    break;
                }
                // Resume Sky: redraw everything fresh.
                app.render_all();
                if app.current_image.is_some() { app.refresh_image(); }
            }
            "?" => app.show_help(),
            "UP" | "k" => { app.move_row(-1); app.render_all(); }
            "DOWN" | "j" => { app.move_row(1); app.render_all(); }
            "PgUP" | "K" => { app.page_up(); app.render_all(); }
            "PgDOWN" | "J" => { app.page_down(); app.render_all(); }
            "HOME" => { app.go_first(); app.render_all(); }
            "END" => { app.go_last(); app.render_all(); }
            "l" => { app.prompt_loc(); app.render_all(); }
            "a" => { app.prompt_lat(); app.render_all(); }
            "o" => { app.prompt_lon(); app.render_all(); }
            "c" => { app.prompt_cloud(); app.render_all(); }
            "h" => { app.prompt_humidity(); app.render_all(); }
            "t" => { app.prompt_temp(); app.render_all(); }
            "w" => { app.prompt_wind(); app.render_all(); }
            "b" => { app.prompt_bortle(); app.render_all(); }
            "e" => { app.show_all_events(); }
            "s" => { app.show_starchart(); }
            "S" => { app.open_starchart_external(); }
            "A" => { app.show_apod(); }
            "ENTER" => { app.refresh_image(); }
            "r" => { app.render_all(); }
            "R" => { app.fetch_all(); app.render_all(); }
            "W" => { app.cfg.save(); app.footer_say(" Config saved", 46); }
            _ => {}
        }
    }

    app.clear_image();
    app.cfg.save();
    Crust::cleanup();
    Crust::clear_screen();
}

struct PlanetData {
    table: String,
    mphase: u8,
    mph_s: &'static str,
    bodies: Vec<orbit::BodyObs>,
}

struct App {
    cfg: Config,
    hours: Vec<HourPoint>,
    days: HashMap<String, DayForecast>,
    planets: HashMap<String, PlanetData>,
    events: HashMap<String, events::Event>,
    /// Indices into `self.hours` where the "!" event marker should appear.
    event_marked: std::collections::HashSet<usize>,
    index: usize,
    cols: u16,
    rows: u16,
    header: Pane,
    titles: Pane,
    left: Pane,
    main_p: Pane,
    footer: Pane,
    last_updated: String,
    today: (i32, u32, u32),
    image_display: Option<glow::Display>,
    current_image: Option<std::path::PathBuf>,

    /// Async result channels. When a background fetch completes it sends
    /// the result here; the main loop polls and integrates it on the next
    /// render tick.
    event_rx: Option<mpsc::Receiver<HashMap<String, events::Event>>>,
    image_rx: Option<mpsc::Receiver<Option<std::path::PathBuf>>>,
}

impl App {
    fn new(cfg: Config) -> Self {
        let (cols, rows) = Crust::terminal_size();
        let today = date_util::today();
        let panes = Self::build_panes(cols, rows);
        Self {
            cfg,
            hours: Vec::new(),
            days: HashMap::new(),
            planets: HashMap::new(),
            events: HashMap::new(),
            event_marked: std::collections::HashSet::new(),
            index: 0,
            cols, rows,
            header: panes.0, titles: panes.1, left: panes.2,
            main_p: panes.3, footer: panes.4,
            last_updated: String::new(),
            today,
            image_display: None,
            current_image: None,
            event_rx: None,
            image_rx: None,
        }
    }

    fn build_panes(cols: u16, rows: u16) -> (Pane, Pane, Pane, Pane, Pane) {
        // Astropanel-style: left pane starts at x=2 to give a 1-col left
        // margin and align data rows with the header's leading space.
        let left_w: u16 = 70.min(cols.saturating_sub(20));
        let main_x: u16 = left_w + 4;
        let main_w: u16 = cols.saturating_sub(main_x);
        let content_h: u16 = rows.saturating_sub(3);
        let mut header = Pane::new(1, 1, cols, 1, 255, 236);
        header.wrap = false;
        let mut titles = Pane::new(1, 2, cols, 1, 255, 234);
        titles.wrap = false;
        let mut left = Pane::new(2, 3, left_w, content_h, 248, 232);
        left.wrap = false;
        let mut main_p = Pane::new(main_x, 3, main_w, content_h, 255, 232);
        main_p.wrap = false;
        let mut footer = Pane::new(1, rows, cols, 1, 255, 24);
        footer.wrap = false;
        (header, titles, left, main_p, footer)
    }

    fn tick(&mut self) {
        let now = date_util::today();
        if now != self.today {
            self.today = now;
            self.render_all();
        }
    }

    fn fetch_all(&mut self) {
        self.footer_say("Fetching weather...", 226);
        self.render_footer();
        let days = weather::fetch_cached(self.cfg.lat, self.cfg.lon, false);
        self.hours = days.iter().flat_map(|d| d.hours.clone()).collect();
        self.days = days.into_iter().map(|d| (d.date.clone(), d)).collect();

        // Compute ephemeris per unique date.
        self.planets.clear();
        let dates: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for h in &self.hours {
                if !seen.contains(&h.date) { seen.push(h.date.clone()); }
            }
            seen
        };
        for date in &dates {
            let (y, m, d) = parse_date(date);
            let bodies = orbit::all_bodies(y, m, d, self.cfg.lat, self.cfg.lon, self.cfg.tz);
            let mph = orbit::moon_phase(y, m, d);
            let table = orbit::ephemeris_table(&bodies);
            self.planets.insert(date.clone(), PlanetData {
                table,
                mphase: (mph.illumination * 100.0).round() as u8,
                mph_s: mph.phase_name,
                bodies,
            });
        }

        self.last_updated = now_hhmm();
        if self.hours.is_empty() {
            self.footer_say("Weather fetch failed", 196);
        } else {
            self.footer_say(&format!("Loaded {} hours; events loading...", self.hours.len()), 46);
        }

        // Events: spawn on a background thread, pick up via channel.
        self.spawn_events_fetch();

        // Prune old cached images periodically.
        images::cleanup_cache();
    }

    fn spawn_events_fetch(&mut self) {
        let (tx, rx) = mpsc::channel();
        let lat = self.cfg.lat;
        let lon = self.cfg.lon;
        let tz = self.cfg.tz_name.clone();
        std::thread::spawn(move || {
            let ev = events::fetch_events(lat, lon, &tz);
            let _ = tx.send(ev);
        });
        self.event_rx = Some(rx);
    }

    fn spawn_starchart_fetch(&mut self, year: i32, month: u32, day: u32, hour: u32) {
        let (tx, rx) = mpsc::channel();
        let lat = self.cfg.lat;
        let lon = self.cfg.lon;
        let tz = self.cfg.tz;
        std::thread::spawn(move || {
            let _ = tx.send(images::fetch_starchart(year, month, day, hour, lat, lon, tz));
        });
        self.image_rx = Some(rx);
    }

    fn spawn_apod_fetch(&mut self) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(images::fetch_apod());
        });
        self.image_rx = Some(rx);
    }

    /// Poll any in-flight async fetches and apply results when ready.
    /// Returns true if state changed and a re-render is warranted.
    fn poll_async(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = self.event_rx.take() {
            match rx.try_recv() {
                Ok(ev) => {
                    self.events = ev;
                    self.recompute_event_marks();
                    let n = self.events.len();
                    self.footer_say(&format!("Events loaded: {}", n), 46);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still running: put back.
                    self.event_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.image_rx.take() {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.current_image = Some(path.clone());
                    self.show_image_path(path);
                    changed = true;
                }
                Ok(None) => {
                    self.footer_say(" Image fetch failed", 196);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.image_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    /// For each date with an astronomical event, find the data row whose
    /// hour is the latest hour ≤ event time (so the "!" appears just before
    /// the event time slot). Falls back to the first row of that date when
    /// the event happens before any available data hour for the day.
    fn recompute_event_marks(&mut self) {
        self.event_marked.clear();
        for (date, ev) in &self.events {
            let event_hour: i64 = ev.time.get(..2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let mut best: Option<(usize, i64)> = None;
            let mut first_of_date: Option<usize> = None;
            for (i, h) in self.hours.iter().enumerate() {
                if h.date != *date { continue; }
                if first_of_date.is_none() { first_of_date = Some(i); }
                if h.hour <= event_hour {
                    if best.map(|(_, hr)| h.hour > hr).unwrap_or(true) {
                        best = Some((i, h.hour));
                    }
                }
            }
            let pick = best.map(|(i, _)| i).or(first_of_date);
            if let Some(i) = pick {
                self.event_marked.insert(i);
            }
        }
    }

    /// Astropanel behavior: auto-fetch starchart when lat > 23, else APOD.
    /// Called on startup. Uses cached image if available (instant), else
    /// kicks off a background fetch that populates when ready.
    fn auto_image(&mut self) {
        if self.cfg.lat > 23.0 {
            if let Some(h) = self.hours.get(self.index).cloned() {
                let (y, m, d) = parse_date(&h.date);
                if let Some(path) = images::starchart_cached(y, m, d, h.hour as u32,
                    self.cfg.lat, self.cfg.lon, self.cfg.tz) {
                    self.current_image = Some(path);
                } else {
                    self.spawn_starchart_fetch(y, m, d, h.hour as u32);
                }
            }
        } else if let Some(path) = images::apod_cached() {
            self.current_image = Some(path);
        } else {
            self.spawn_apod_fetch();
        }
    }

    fn move_row(&mut self, n: i32) {
        let len = self.hours.len();
        if len == 0 { return; }
        let new = self.index as i32 + n;
        self.index = new.rem_euclid(len as i32) as usize;
    }
    fn page_up(&mut self) {
        let h = self.left.h as usize;
        self.index = self.index.saturating_sub(h);
    }
    fn page_down(&mut self) {
        let h = self.left.h as usize;
        self.index = (self.index + h).min(self.hours.len().saturating_sub(1));
    }
    fn go_first(&mut self) { self.index = 0; }
    fn go_last(&mut self) { self.index = self.hours.len().saturating_sub(1); }

    fn render_all(&mut self) {
        // Don't clear_screen: each pane's refresh() handles its own area via
        // prev_frame diff. Clearing wipes terminal but leaves prev_frame stale,
        // so diff thinks nothing changed and nothing gets redrawn.
        self.render_header();
        self.render_titles();
        self.render_left();
        self.render_main();
        self.render_footer();
    }

    fn render_header(&mut self) {
        let body_list: String = orbit::BODY_ORDER.iter()
            .map(|b| style::fg(orbit::body_symbol(b), orbit::body_color_256(b)))
            .collect::<Vec<_>>().join(" ");
        let cols_header = format!(
            " YYYY-MM-DD  HH   Cld    Hum    Temp     Wind   ! {}      ",
            body_list
        );
        let (y, m, d) = self.today;
        let local = date_util::now_secs() + date_util::local_tz_offset_secs();
        let (_, _, _, hh, mm, ss) = date_util::ts_to_parts(local);
        let jd = orbit::julian_date_now(y, m, d, hh, mm, ss);
        let right = format!(
            "{} tz{:+} ({}/{})  Bortle {:.1}  Updated {}  JD:{:.5}",
            self.cfg.location,
            self.cfg.tz as i32,
            self.cfg.lat,
            self.cfg.lon,
            self.cfg.bortle,
            self.last_updated,
            jd,
        );
        let text = style::bold(&format!("{}{}", cols_header, right));
        self.header.say(&text);
    }

    fn render_titles(&mut self) {
        // Left-pane column layout (no leading space):
        //   date(10)  + "  " + hour(2) + "  " + cld(4) + "  " + hum(5)
        //   + "  " + temp(6) + "  " + wind(8) + event(3) = 45+3 = 48 cols.
        // Then " {body}" per body starting col 48, so body chars land at
        // cols 49, 51, 53, 55, 57, 59, 61, 63, 65.
        //
        // Title line: limits segment padded to col 48, then the 9 dots
        // prefixed with one space to align with the body columns.

        // Right-align each limit value so it lines up with the corresponding
        // data-row column end (title pane is x=1, data pane is x=2, so title
        // cols are +1 of data cols).
        //   cloud end: title col 20   (4 chars wide)
        //   hum end:   title col 27   (4 chars wide, +3 sep)
        //   temp end:  title col 35   (6 chars wide, +2 sep)
        //   wind end:  title col 45   (5 chars wide, +4 sep)
        let limits_raw = format!(
            "{:>21}{:>7}{:>8}{:>10}",
            format!("<{}%", self.cfg.cloud_limit),
            format!("<{:.0}%", self.cfg.humidity_limit),
            format!(">{:.0}\u{00B0}C", self.cfg.temp_limit),
            format!("<{:.0}m/s", self.cfg.wind_limit),
        );
        // Pad limits to pane col 49, then prefix dots with one space so the
        // first marker lands at pane col 50 (matching data-row body char).
        let limits = pad_right(&limits_raw, 49);
        let dots = " \u{2506} \u{2506} \u{2506} \u{2506} \u{2506} \u{2506} \u{2506} \u{2506} \u{2506}";

        // Current selection info
        let (date_s, hour_s, cond_col) = match self.hours.get(self.index) {
            Some(h) => {
                let color = self.cond_color_for(h);
                (h.date.clone(), h.hour_str.clone(), color)
            }
            None => ("".into(), "".into(), 244),
        };
        let weekday = if !date_s.is_empty() {
            let (y, m, d) = parse_date(&date_s);
            date_util::weekday_short(date_util::weekday(y, m, d)).to_string()
        } else { String::new() };

        let moon = self.hours.get(self.index)
            .and_then(|h| self.planets.get(&h.date))
            .map(|p| format!("  Moon: {} ({}%)", p.mph_s, p.mphase))
            .unwrap_or_default();

        let selection = style::bold(&style::fg(
            &format!("{} ({}) {}:00", date_s, weekday, hour_s),
            cond_col,
        ));
        let limits_part = style::fg(&limits, 244);
        let dots_part = style::fg(dots, 244);
        let moon_part = style::fg(&moon, 244);

        let line = format!("{}{}      {}{}", limits_part, dots_part, selection, moon_part);
        self.titles.say(&line);
    }

    fn cond_color_for(&self, h: &HourPoint) -> u8 {
        let points = weather::condition_points(
            h.cloud, h.humidity, h.temp, h.wind,
            self.cfg.cloud_limit, self.cfg.humidity_limit,
            self.cfg.temp_limit, self.cfg.wind_limit);
        weather::condition_color(points)
    }

    fn render_left(&mut self) {
        let mut lines = Vec::new();
        let mut prev_date = String::new();
        for (i, h) in self.hours.iter().enumerate() {
            let color = self.cond_color_for(h);
            let date_s = if h.date == prev_date { "          ".to_string() } else { h.date.clone() };
            prev_date = h.date.clone();

            let row_base = format!(
                "{}  ",
                date_s,
            );
            let mut row = style::fg(&row_base, color);

            let core = format!(
                "{}  {:>4}  {:>5}  {:>6}  {:>8}",
                h.hour_str,
                format!("{}%", h.cloud),
                format!("{}%", h.humidity as i64),
                format!("{:.1}\u{00B0}C", h.temp),
                format!("{:.1}({})", h.wind, h.wind_dir_name),
            );
            let core = if i == self.index {
                style::fg(&style::underline(&core), color)
            } else {
                style::fg(&core, color)
            };
            row.push_str(&core);

            // Event marker: shown on the row that owns the event's time slot
            // (latest hour ≤ event time, or first row of that date).
            let marked = self.event_marked.contains(&i);
            row.push_str(&style::fg(
                if marked { "  !" } else { "   " },
                color,
            ));

            // Visibility bars for each body
            if let Some(pd) = self.planets.get(&h.date) {
                for (j, body) in orbit::BODY_ORDER.iter().enumerate() {
                    let block_char = if j < 2 { "\u{2588}" } else { "\u{2503}" };
                    let above = pd.bodies.iter()
                        .find(|b| b.name == *body)
                        .map(|b| orbit::is_above(b.rise_h, b.set_h, b.always_up, b.never_up, h.hour as f64))
                        .unwrap_or(false);
                    if above {
                        let color_hex = if *body == "moon" {
                            orbit::moon_phase_gray(pd.mphase)
                        } else {
                            orbit::body_color_hex(body).to_string()
                        };
                        let c = orbit::hex_to_256(&color_hex);
                        row.push(' ');
                        row.push_str(&style::fg(block_char, c));
                    } else {
                        row.push_str("  ");
                    }
                }
            }

            lines.push(row);
        }

        // Center the selection
        let total = lines.len();
        let height = self.left.h as usize;
        let top = if total <= height { 0 } else {
            let half = height / 2;
            if self.index < half { 0 }
            else if self.index + half >= total { total - height }
            else { self.index - half }
        };

        self.left.set_text(&lines.join("\n"));
        self.left.ix = top;
        self.left.full_refresh();
    }

    fn render_main(&mut self) {
        let Some(h) = self.hours.get(self.index).cloned() else {
            self.main_p.set_text("");
            self.main_p.full_refresh();
            return;
        };

        let fog_s = if h.fog <= 0.0 { "-".into() } else { format!("{}%", h.fog as i64) };
        let uv_s = if h.uv == 0.0 { "-".into() } else { format!("{:.1}", h.uv) };
        // Compress the six weather lines into a 3x2 table so the
        // (optional) starchart / APOD image doesn't crash into the text.
        let left = [
            format!("Clouds:    {}% (low/high {}/{})", h.cloud, h.cloud_low, h.cloud_high),
            format!("Humidity:  {}% (fog {})", h.humidity as i64, fog_s),
            format!("Wind:      {} m/s dir {} gusts {}", h.wind, h.wind_dir_name, h.gust),
        ];
        let right = [
            format!("Temp:      {}\u{00B0}C (dew {}\u{00B0}C)", h.temp, h.dew_point),
            format!("Pressure:  {} hPa", h.pressure as i64),
            format!("UV index:  {}", uv_s),
        ];
        let col_w = left.iter().map(|s| s.chars().count()).max().unwrap_or(0) + 4;
        let mut info = String::new();
        for (l, r) in left.iter().zip(right.iter()) {
            let pad = col_w.saturating_sub(l.chars().count());
            info.push_str(l);
            info.push_str(&" ".repeat(pad));
            info.push_str(r);
            info.push('\n');
        }

        let mut buf = String::new();
        buf.push('\n'); // one row of padding above
        buf.push_str(&style::fg(&info, 230));
        buf.push('\n');

        if let Some(pd) = self.planets.get(&h.date) {
            buf.push_str(&pd.table);
        } else {
            buf.push_str(&format!("No ephemeris data for {}\n", h.date));
        }

        // Event for this date - always colored with the row's condition color
        // (matches astropanel behavior so events stand out visually).
        if let Some(ev) = self.events.get(&h.date) {
            buf.push('\n');
            let col = self.cond_color_for(&h);
            buf.push_str(&style::fg(&format!("@ {}: {}", ev.time, ev.event), col));
            buf.push('\n');
            buf.push_str(&style::fg(&ev.link, col));
            buf.push('\n');
        } else {
            // No notable astronomical event for this date — fall back to a
            // locally-computed "tonight" summary so the user always sees
            // something sky-relevant. Cheap (no network).
            let (y, m, d) = parse_date(&h.date);
            let summary = orbit::tonight_summary(
                y, m, d, self.cfg.lat, self.cfg.lon, self.cfg.tz, self.cfg.bortle,
            );
            buf.push('\n');
            buf.push_str(&style::fg(&summary, 245));
        }

        self.main_p.set_text(&buf);
        self.main_p.ix = 0;
        self.main_p.full_refresh();
    }

    fn render_footer(&mut self) {
        let cmds = "?=Help g=Gear l=Loc a=Lat o=Lon c=Cloud h=Hum t=Temp w=Wind b=Bortle e=Events s=Starchart S=Open A=APOD r=Redraw R=Refetch W=Write q=Quit";
        self.footer.say(cmds);
    }

    fn footer_say(&mut self, msg: &str, color: u8) {
        self.footer.say(&style::fg(msg, color));
    }

    /// Fetch and display the Stelvision starchart for the selected hour.
    /// Non-blocking: kicks off a background fetch; the image appears when
    /// the download completes (polled each render tick).
    fn show_starchart(&mut self) {
        let Some(h) = self.hours.get(self.index).cloned() else { return };
        let (y, m, d) = parse_date(&h.date);
        if let Some(path) = images::starchart_cached(y, m, d, h.hour as u32,
            self.cfg.lat, self.cfg.lon, self.cfg.tz) {
            self.show_image_path(path);
            return;
        }
        self.footer_say(" Fetching starchart...", 226);
        self.render_footer();
        self.spawn_starchart_fetch(y, m, d, h.hour as u32);
    }

    /// Open the last-fetched starchart in the system image viewer.
    fn open_starchart_external(&mut self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let path = std::path::PathBuf::from(home).join(".astro/images/starchart.jpg");
        if !path.exists() {
            self.footer_say(" No starchart yet (press s first)", 196);
            return;
        }
        let _ = std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn();
    }

    /// Fetch and display the NASA APOD (cached per day).
    fn show_apod(&mut self) {
        if let Some(path) = images::apod_cached() {
            self.show_image_path(path);
            return;
        }
        self.footer_say(" Fetching APOD...", 226);
        self.render_footer();
        self.spawn_apod_fetch();
    }

    /// Redraw the currently displayed image (astropanel ENTER behavior).
    fn refresh_image(&mut self) {
        if let Some(path) = self.current_image.clone() {
            self.show_image_path(path);
        }
    }

    fn show_image_path(&mut self, path: std::path::PathBuf) {
        self.clear_image();

        if !path.exists() {
            self.footer_say(" Image file missing", 196);
            return;
        }

        let display = glow::Display::new();
        if !display.supported() {
            self.footer_say(" Image display not supported in this terminal", 196);
            return;
        }

        // Place the image in the lower portion of the main pane, leaving
        // the top for the weather info + ephemeris table and a 2-row gap
        // above the status bar at the bottom.
        let top_offset: u16 = 23;
        let bottom_gap: u16 = 1;
        let img_x = self.main_p.x;
        let img_y = self.main_p.y + top_offset;
        let img_w = self.main_p.w.saturating_sub(2);
        let img_h = self.main_p.h.saturating_sub(top_offset + bottom_gap);

        if img_h < 4 {
            self.footer_say(" Not enough room for image", 196);
            return;
        }

        self.image_display = Some(display);
        if let Some(ref mut disp) = self.image_display {
            disp.show(path.to_string_lossy().as_ref(), img_x, img_y, img_w, img_h);
        }
        self.current_image = Some(path);
        self.footer_say(" Press ENTER to refresh, any other key continues", 46);
    }

    fn clear_image(&mut self) {
        if let Some(ref mut disp) = self.image_display {
            disp.clear(self.main_p.x, self.main_p.y, self.main_p.w, self.main_p.h,
                self.cols, self.rows);
        }
        self.image_display = None;
    }

    fn show_help(&mut self) {
        let help = "\n \
            Astro - sky panel + telescope/eyepiece catalog.\n \
            Sky mode: weather, ephemeris, events, starchart, APOD.\n \
            Gear mode (press g): catalog telescopes/eyepieces, compute optics.\n\n \
            * Weather forecast with coloring based on cloud/humidity/temp/wind limits\n \
            * Visibility bars for Sun, Moon and planets per hour\n \
            * Moon phase shown via bar shade (new=dark, full=bright)\n \
            * Astronomical events from in-the-sky.org\n \
            * Ephemeris table (RA, Dec, distance, rise, transit, set)\n \
            * Starchart (lat > 23) and NASA APOD inline image display\n\n \
            SKY KEYS\n \
             ? = This help             ENTER = Refresh starchart/image\n \
             g = Switch to Gear mode\n \
             UP/DOWN = Move row        r = Redraw all panes\n \
             PgUP/PgDOWN = Page        R = Refetch weather + events\n \
             HOME/END = First/Last     e = Show all events\n \
             l = Location name         s = Get starchart for selected time\n \
             a = Latitude              S = Open starchart in image viewer\n \
             o = Longitude             A = Show Astronomy Picture Of the Day\n \
             c = Cloud limit           h = Humidity limit\n \
             t = Temp lower limit      w = Wind limit\n \
             b = Bortle scale (1-9)    W = Save config\n \
             q = Quit";
        self.main_p.set_text(help);
        self.main_p.ix = 0;
        self.main_p.full_refresh();
        let _ = Input::getchr(None);
        self.render_main();
    }

    fn show_all_events(&mut self) {
        let mut buf = String::from("Upcoming events:\n\n");
        let mut dates: Vec<&String> = self.events.keys().collect();
        dates.sort();
        for d in dates {
            let ev = &self.events[d];
            buf.push_str(&format!("{} {}  {}\n  {}\n\n", d, ev.time, ev.event, ev.link));
        }
        self.main_p.set_text(&buf);
        self.main_p.ix = 0;
        self.main_p.full_refresh();
        let _ = Input::getchr(None);
        self.render_main();
    }

    fn prompt_loc(&mut self) {
        let s = self.footer.ask("Loc? ", &self.cfg.location);
        if !s.trim().is_empty() { self.cfg.location = s.trim().into(); }
    }

    fn prompt_lat(&mut self) {
        let s = self.footer.ask("Lat? (-90..90) ", &self.cfg.lat.to_string());
        if let Ok(v) = s.trim().parse::<f64>() {
            if (-90.0..=90.0).contains(&v) { self.cfg.lat = v; self.fetch_all(); }
        }
    }
    fn prompt_lon(&mut self) {
        let s = self.footer.ask("Lon? (-180..180) ", &self.cfg.lon.to_string());
        if let Ok(v) = s.trim().parse::<f64>() {
            if (-180.0..=180.0).contains(&v) { self.cfg.lon = v; self.fetch_all(); }
        }
    }
    fn prompt_cloud(&mut self) {
        let s = self.footer.ask("Maximum Cloud coverage? ", &self.cfg.cloud_limit.to_string());
        if let Ok(v) = s.trim().parse::<i64>() { self.cfg.cloud_limit = v; }
    }
    fn prompt_humidity(&mut self) {
        let s = self.footer.ask("Maximum Humidity? ", &self.cfg.humidity_limit.to_string());
        if let Ok(v) = s.trim().parse::<f64>() { self.cfg.humidity_limit = v; }
    }
    fn prompt_temp(&mut self) {
        let s = self.footer.ask("Minimum Temperature? ", &self.cfg.temp_limit.to_string());
        if let Ok(v) = s.trim().parse::<f64>() { self.cfg.temp_limit = v; }
    }
    fn prompt_wind(&mut self) {
        let s = self.footer.ask("Maximum Wind? ", &self.cfg.wind_limit.to_string());
        if let Ok(v) = s.trim().parse::<f64>() { self.cfg.wind_limit = v; }
    }
    fn prompt_bortle(&mut self) {
        let s = self.footer.ask("Bortle? (1..9) ", &self.cfg.bortle.to_string());
        if let Ok(v) = s.trim().parse::<f64>() {
            if (1.0..=9.0).contains(&v) { self.cfg.bortle = v; }
        }
    }
}

fn parse_date(s: &str) -> (i32, u32, u32) {
    let parts: Vec<&str> = s.split('-').collect();
    let y: i32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(2000);
    let m: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1);
    let d: u32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(1);
    (y, m, d)
}

fn pad_right(s: &str, width: usize) -> String {
    let w = crust::display_width(s);
    if w >= width { return s.to_string(); }
    format!("{}{}", s, " ".repeat(width - w))
}

/// Interactive first-run setup. Prompts the user for location, lat/lon,
/// timezone, and observation limits before Crust takes over the screen.
fn first_run_setup(mut cfg: Config) -> Config {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    let mut stdout = io::stdout();

    println!("\nastro: first-run setup (press Enter to accept defaults)\n");

    let prompt = |label: &str, default: &str, out: &mut io::StdoutLock, rd: &mut io::StdinLock| -> String {
        print!("  {} [{}]: ", label, default);
        let _ = out.flush();
        let mut s = String::new();
        if rd.read_line(&mut s).is_err() { return default.to_string(); }
        let t = s.trim();
        if t.is_empty() { default.to_string() } else { t.to_string() }
    };

    let mut out = stdout.lock();
    cfg.location = prompt("Location name (display)", &cfg.location, &mut out, &mut lock);
    cfg.tz_name = prompt("Timezone (Cont/City, e.g. Europe/Oslo)", &cfg.tz_name, &mut out, &mut lock);
    let lat_s = prompt("Latitude (-90..90)", &cfg.lat.to_string(), &mut out, &mut lock);
    if let Ok(v) = lat_s.parse::<f64>() {
        if (-90.0..=90.0).contains(&v) { cfg.lat = v; }
    }
    let lon_s = prompt("Longitude (-180..180)", &cfg.lon.to_string(), &mut out, &mut lock);
    if let Ok(v) = lon_s.parse::<f64>() {
        if (-180.0..=180.0).contains(&v) { cfg.lon = v; }
    }
    let tz_s = prompt("Timezone offset hours (e.g. 1 for CET)", &cfg.tz.to_string(), &mut out, &mut lock);
    if let Ok(v) = tz_s.parse::<f64>() { cfg.tz = v; }
    let cloud_s = prompt("Cloud coverage limit %", &cfg.cloud_limit.to_string(), &mut out, &mut lock);
    if let Ok(v) = cloud_s.parse::<i64>() { cfg.cloud_limit = v; }
    let hum_s = prompt("Humidity limit %", &cfg.humidity_limit.to_string(), &mut out, &mut lock);
    if let Ok(v) = hum_s.parse::<f64>() { cfg.humidity_limit = v; }
    let temp_s = prompt("Minimum temperature °C", &cfg.temp_limit.to_string(), &mut out, &mut lock);
    if let Ok(v) = temp_s.parse::<f64>() { cfg.temp_limit = v; }
    let wind_s = prompt("Wind limit m/s", &cfg.wind_limit.to_string(), &mut out, &mut lock);
    if let Ok(v) = wind_s.parse::<f64>() { cfg.wind_limit = v; }
    let bortle_s = prompt("Bortle scale (1..9)", &cfg.bortle.to_string(), &mut out, &mut lock);
    if let Ok(v) = bortle_s.parse::<f64>() {
        if (1.0..=9.0).contains(&v) { cfg.bortle = v; }
    }

    println!("\n  Config saved to ~/.astro/config.yml");
    println!("  Starting astro...\n");
    cfg
}

fn now_hhmm() -> String {
    let local = date_util::now_secs() + date_util::local_tz_offset_secs();
    let (_, _, _, h, m, _) = date_util::ts_to_parts(local);
    format!("{:02}:{:02}", h, m)
}
