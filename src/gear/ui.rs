use super::data;
use super::optics;

use crust::{Crust, Input, Pane};
use crust::style;
use data::{Config, Eyepiece, Store, Telescope};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run the Gear (telescope/eyepiece) sub-mode. Takes over the alt
/// screen, runs its own input loop, returns when the user presses
/// `g` (flip back to Sky mode) or `q`/`Q` (quit astro entirely).
/// Returns true if astro should exit; false if it should resume Sky.
pub fn run(env: super::SkyEnv) -> bool {
    let cfg = Config::load();
    let mut store = Store::load();

    Crust::clear_screen();
    let mut app = App::new(cfg, store.clone());
    app.env = env;
    app.render_all();

    let mut quit_astro = false;

    loop {
        let Some(key) = Input::getchr(Some(5)) else { continue };
        match key.as_str() {
            "g" | "G" => {
                if app.cfg.auto_backup { data::backup(&app.store, app.cfg.backup_count); }
                let _ = app.store.save();
                break;
            }
            "q" => {
                if app.cfg.auto_backup { data::backup(&app.store, app.cfg.backup_count); }
                let _ = app.store.save();
                quit_astro = true;
                break;
            }
            "Q" => { quit_astro = true; break; }
            "ESC" => {
                // Clear any lingering status message; restore the
                // default key-hint footer.
                app.status = None;
                app.render_footer();
            }
            "?" => app.show_help(),
            "t" => { app.add_telescope(); app.render_all(); }
            "e" => { app.add_eyepiece(); app.render_all(); }
            "ENTER" => { app.edit_selected(); app.render_all(); }
            "TAB" => { app.toggle_focus(); app.render_all(); }
            "j" | "DOWN" => { app.move_down(); app.render_all(); }
            "k" | "UP" => { app.move_up(); app.render_all(); }
            "S-UP" => { app.shift_up(); app.render_all(); }
            "S-DOWN" => { app.shift_down(); app.render_all(); }
            "HOME" => { app.go_first(); app.render_all(); }
            "END" => { app.go_last(); app.render_all(); }
            " " | "SPACE" => { app.toggle_tag(); app.render_all(); }
            "u" => { app.untag_all(); app.render_all(); }
            "A" => { app.tag_all(); app.render_all(); }
            "o" => { app.toggle_sort(); app.render_all(); }
            "D" => { app.delete_selected(); app.render_all(); }
            "C-O" => { app.create_observation_log(); }
            "x" => { app.export_csv(); }
            "X" => { app.export_json(); }
            "r" => { app.render_all(); }
            "v" => { app.show_version(); }
            _ => {}
        }
        store = app.store.clone();
        let _ = store.save();
    }

    Crust::clear_screen();
    quit_astro
}

#[derive(Clone, Copy, PartialEq)]
enum Focus { Ts, Ep }

struct App {
    cfg: Config,
    store: Store,
    cols: u16,
    rows: u16,
    header: Pane,
    ts_head: Pane,
    ts: Pane,
    ep_head: Pane,
    ep: Pane,
    combo_head: Pane,
    combo: Pane,
    footer: Pane,
    focus: Focus,
    ts_idx: usize,
    ep_idx: usize,
    ts_tagged: Vec<bool>,
    ep_tagged: Vec<bool>,
    sort_on: bool,
    status: Option<(String, u8)>,
    /// Snapshot of Sky-mode state at mode-switch time. Drives the
    /// Bortle-aware mag-limit column and observation-log auto-fill.
    env: super::SkyEnv,
}

impl App {
    fn new(cfg: Config, store: Store) -> Self {
        let (cols, rows) = Crust::terminal_size();
        let ts_tagged = vec![false; store.telescopes.len()];
        let ep_tagged = vec![false; store.eyepieces.len()];
        let panes = Self::build_panes(cols, rows, &cfg);
        Self {
            cfg, store, cols, rows,
            header: panes.0, ts_head: panes.1, ts: panes.2,
            ep_head: panes.3, ep: panes.4,
            combo_head: panes.5, combo: panes.6,
            footer: panes.7,
            focus: Focus::Ts,
            ts_idx: 0, ep_idx: 0, ts_tagged, ep_tagged,
            sort_on: false,
            status: None,
            env: super::SkyEnv::default(),
        }
    }

    fn build_panes(cols: u16, rows: u16, cfg: &Config)
        -> (Pane, Pane, Pane, Pane, Pane, Pane, Pane, Pane)
    {
        let half = rows / 2;
        let ts_bg = hex_to_256(&cfg.ts_header_bg);
        let ep_bg = hex_to_256(&cfg.ep_header_bg);

        // Bottom half splits horizontally: EP list on the left, scope×EP
        // combo details on the right. EP rows are 80 cols wide (2 cursor
        // + 36 name + 7+6+7+6+6+8 numerics + a 2-col gutter), so the
        // combo pane gets everything past col 80. Below 130 cols total
        // we collapse the combo pane and let the EP list sprawl.
        let ep_w: u16 = if cols >= 130 { 80 } else { cols };
        let combo_w: u16 = cols.saturating_sub(ep_w);

        let mut header = Pane::new(1, 1, cols, 1, 255, 236);
        header.wrap = false;
        let mut ts_head = Pane::new(1, 2, cols, 1, 255, ts_bg);
        ts_head.wrap = false;
        let mut ts = Pane::new(1, 3, cols, half.saturating_sub(3), cfg.text_color, 0);
        ts.wrap = false;
        let mut ep_head = Pane::new(1, half, ep_w, 1, 255, ep_bg);
        ep_head.wrap = false;
        let mut ep = Pane::new(1, half + 1, ep_w, rows.saturating_sub(half + 1), cfg.text_color, 0);
        ep.wrap = false;
        let mut combo_head = Pane::new(ep_w + 1, half, combo_w.max(1), 1, 255, ep_bg);
        combo_head.wrap = false;
        let mut combo = Pane::new(ep_w + 1, half + 1, combo_w.max(1),
                                  rows.saturating_sub(half + 1), cfg.text_color, 0);
        combo.wrap = true;
        let mut footer = Pane::new(1, rows, cols, 1, 255, 236);
        footer.wrap = false;
        (header, ts_head, ts, ep_head, ep, combo_head, combo, footer)
    }

    fn render_all(&mut self) {
        self.render_header();
        self.render_ts_head();
        self.render_ts();
        self.render_ep_head();
        self.render_ep();
        self.render_combo_head();
        self.render_combo();
        self.render_footer();
    }

    fn render_header(&mut self) {
        let focus = if self.focus == Focus::Ts { "Telescopes" } else { "Eyepieces" };
        let ts_tag = self.ts_tagged.iter().filter(|b| **b).count();
        let ep_tag = self.ep_tagged.iter().filter(|b| **b).count();
        let bortle_note = if self.env.bortle > 0.0 {
            format!("   <MGN @ Bortle {:.0}", self.env.bortle)
        } else { String::new() };
        let left = format!(" astro v{} [Gear]   [{}]   TS tagged: {}   EP tagged: {}   sort: {}{}",
            VERSION, focus, ts_tag, ep_tag,
            if self.sort_on { "on" } else { "off" }, bortle_note);
        self.header.say(&style::bold(&left));
    }

    fn render_ts_head(&mut self) {
        // Column widths must mirror the data row format in render_ts.
        // SEP-R and SEP-D widths include their trailing quote (8 / 7).
        let line = format!(
            "  {:<28}{:>7}{:>8}{:>6}{:>6}{:>6}{:>10}{:>9}{:>8}{:>7}{:>7}{:>6}{:>6}{:>6}{:>6}{:>7}{:>6}",
            "Telescope",
            "APP", "TFL", "F/?", "<MGN", "xEYE", "MINx", "MAXx",
            "SEP-R", "SEP-D",
            "*FLD", "GLXY", "PLNT", "DBL*", ">2*<", "MOON", "SUN",
        );
        self.ts_head.say(&style::bold(&line));
    }

    fn render_ts(&mut self) {
        let ordered: Vec<usize> = self.ordered_ts_indices();
        let mut lines = Vec::new();
        for (vis_i, &i) in ordered.iter().enumerate() {
            let t = &self.store.telescopes[i];
            let tagged = *self.ts_tagged.get(i).unwrap_or(&false);
            let (app, tfl) = (t.app, t.tfl);
            let focused = self.focus == Focus::Ts && vis_i == self.ts_idx;
            // Build cursor + tag markers explicitly (1 cell each) so we
            // never have to byte-slice ANSI sequences.
            let cursor = if focused { "\u{2192}" } else { " " };
            let tag = if tagged {
                style::bold(&style::fg("\u{2590}", self.cfg.tag_color as u8))
            } else {
                " ".to_string()
            };
            let body = format!(
                "{:<28}{:>7.0}{:>8.0}{:>6.1}{:>6.1}{:>6.0}{:>10}{:>9}{:>7.2}\"{:>6.2}\"{:>7.0}{:>6.0}{:>6.0}{:>6.0}{:>6.0}{:>7.0}{:>6.0}",
                truncate_or_pad(&t.name, 28),
                app,
                tfl,
                optics::tfr(app, tfl),
                optics::mlim_bortle(app, self.env.bortle),
                optics::xeye(app),
                format!("{:.0}({:.0}x)", optics::mine(app, tfl), optics::minx(app, tfl)),
                format!("{:.0}({:.0}x)", optics::maxe(app, tfl), optics::maxx(app)),
                optics::sepr(app),
                optics::sepd(app),
                optics::e_st(app, tfl),
                optics::e_gx(app, tfl),
                optics::e_pl(app, tfl),
                optics::e_2s(app, tfl),
                optics::e_t2(app, tfl),
                optics::moon(tfl),
                optics::sun(tfl),
            );
            let row = format!("{}{}{}", cursor, tag, body);
            let line = if focused {
                style::bg(&row, self.cfg.cursor_bg as u8)
            } else {
                row
            };
            lines.push(line);
        }
        if lines.is_empty() {
            lines.push(style::fg("  (no telescopes - press 't' to add)", 245));
        }
        self.ts.set_text(&lines.join("\n"));
        self.ts.ix = scroll_offset(self.ts_idx, lines.len(), self.ts.h as usize);
        self.ts.full_refresh();
    }

    fn render_ep_head(&mut self) {
        // Match the data row: name(36) + FL(7) + AFOV(6) + MAGX(7) + TFOV(6) + PPL(6) + 2BLW(8)
        let line = format!(
            "  {:<36}{:>7}{:>6}{:>7}{:>6}{:>6}{:>8}",
            "Eyepiece", "FL", "AFOV", "MAGX", "TFOV", "PPL", "2BLW",
        );
        self.ep_head.say(&style::bold(&line));
    }

    fn render_ep(&mut self) {
        // Use the currently-selected telescope for per-EP combination figures.
        let ts = self.current_ts().cloned();
        let ordered: Vec<usize> = self.ordered_ep_indices();
        let mut lines = Vec::new();
        for (vis_i, &i) in ordered.iter().enumerate() {
            let e = &self.store.eyepieces[i];
            let tagged = *self.ep_tagged.get(i).unwrap_or(&false);
            let (magx, tfov, ppl) = match ts.as_ref() {
                Some(t) => (
                    optics::magx(t.tfl, e.fl),
                    optics::tfov(t.tfl, e.fl, e.afov),
                    optics::pupl(t.app, t.tfl, e.fl),
                ),
                None => (0.0, 0.0, 0.0),
            };
            let barlow = match ts.as_ref() {
                Some(t) => format!("{:.0}x", 2.0 * optics::magx(t.tfl, e.fl)),
                None => "-".into(),
            };
            let focused = self.focus == Focus::Ep && vis_i == self.ep_idx;
            let cursor = if focused { "\u{2192}" } else { " " };
            let tag = if tagged {
                style::bold(&style::fg("\u{2590}", self.cfg.tag_color as u8))
            } else {
                " ".to_string()
            };
            let body = format!(
                "{:<36}{:>7.1}{:>6.0}{:>7.0}{:>6.2}{:>6.1}{:>8}",
                truncate_or_pad(&e.name, 36), e.fl, e.afov,
                magx, tfov, ppl, barlow,
            );
            let row = format!("{}{}{}", cursor, tag, body);
            let line = if focused {
                style::bg(&row, self.cfg.cursor_bg as u8)
            } else {
                row
            };
            lines.push(line);
        }
        if lines.is_empty() {
            lines.push(style::fg("  (no eyepieces - press 'e' to add)", 245));
        }
        self.ep.set_text(&lines.join("\n"));
        self.ep.ix = scroll_offset(self.ep_idx, lines.len(), self.ep.h as usize);
        self.ep.full_refresh();
    }

    fn render_combo_head(&mut self) {
        if self.combo.w <= 1 { return; }
        self.combo_head.say(&style::bold("  Scope × EP combo"));
    }

    fn render_combo(&mut self) {
        if self.combo.w <= 1 { return; }
        let ts = self.current_ts().cloned();
        let ep = self.current_ep().cloned();
        let mut lines: Vec<String> = Vec::new();

        match (ts, ep) {
            (Some(t), Some(e)) => {
                let m = optics::magx(t.tfl, e.fl);
                let fov = optics::tfov(t.tfl, e.fl, e.afov);
                let pupil = optics::pupl(t.app, t.tfl, e.fl);
                let max_useful = optics::maxx(t.app);
                let min_useful = optics::minx(t.app, t.tfl);

                lines.push(style::bold(&format!(" {}", t.name)));
                lines.push(format!(" × {}", e.name));
                lines.push(String::new());

                // Magnification — colour the status by zone.
                let m_pct_max = if max_useful > 0.0 { m / max_useful * 100.0 } else { 0.0 };
                let (m_status, m_color) = if m > max_useful {
                    ("over max-useful", 196u8)
                } else if m < min_useful {
                    ("below min-useful", 208u8)
                } else if m_pct_max > 80.0 {
                    ("near max", 220u8)
                } else if m_pct_max < 30.0 {
                    ("low power", 117u8)
                } else {
                    ("good", 46u8)
                };
                lines.push(format!(" Magnification:  {:.0}×", m));
                lines.push(format!("   {:.0}% of max-useful ({:.0}×)   {}",
                    m_pct_max, max_useful, style::fg(m_status, m_color)));
                lines.push(String::new());

                // True FOV plus framing.
                let fov_arcmin = fov * 60.0;
                lines.push(format!(" True FOV:       {:.2}°  ({:.0}')", fov, fov_arcmin));
                let moons = fov / 0.5;
                if moons >= 1.5 {
                    lines.push(format!("   ~{:.1} full Moons across", moons));
                } else if moons >= 0.6 {
                    lines.push(format!("   {:.0}% of full Moon", moons * 100.0));
                } else {
                    lines.push(format!("   tight: {:.0}% of full Moon", moons * 100.0));
                }
                lines.push(framing_hint(fov_arcmin));
                lines.push(String::new());

                // Exit pupil with eye-physiology assessment.
                let (pq_label, pq_color) = pupil_quality(pupil);
                lines.push(format!(" Exit pupil:     {:.2} mm", pupil));
                lines.push(format!("   {}", style::fg(pq_label, pq_color)));
                lines.push(String::new());

                // Best target use, matched against e_st/e_gx/e_pl/e_2s/e_t2.
                lines.push(format!(" Best target:    {}", classify_target(t.app, t.tfl, e.fl)));
                lines.push(String::new());

                // 2× Barlow yields.
                let m_b = m * 2.0;
                let pupil_b = pupil / 2.0;
                let barlow_q = if m_b > max_useful { " (overshoots max)" } else { "" };
                lines.push(style::fg(" With 2× Barlow", 245));
                lines.push(format!("   {:.0}× / pupil {:.2} mm{}", m_b, pupil_b, barlow_q));
                lines.push(String::new());

                // Notes pulled straight from the equipment record so the
                // user can leave reminders ("doesn't grip ETX-90 focuser",
                // etc.) without leaving the gear list.
                if !t.notes.is_empty() || !e.notes.is_empty() {
                    lines.push(style::fg(" Notes", 245));
                    if !t.notes.is_empty() {
                        lines.push(format!("   • {}", t.notes));
                    }
                    if !e.notes.is_empty() {
                        lines.push(format!("   • {}", e.notes));
                    }
                }
            }
            _ => {
                lines.push(style::fg(" (select telescope and eyepiece)", 245));
            }
        }

        self.combo.set_text(&lines.join("\n"));
        self.combo.full_refresh();
    }

    fn render_footer(&mut self) {
        if let Some((ref msg, color)) = self.status {
            self.footer.say(&style::fg(msg, color));
        } else {
            let hint = " t:+TS  e:+EP  ENTER:Edit  TAB:Focus  SPACE:Tag  o:Sort  C-o:Log  x/X:Export  D:Del  r:Redraw  ?:Help  q:Quit";
            self.footer.say(&style::fg(hint, 245));
        }
    }

    fn status_say(&mut self, msg: &str, c: u8) {
        self.status = Some((msg.to_string(), c));
        self.render_footer();
    }

    // --- Selection / ordering ---

    fn ordered_ts_indices(&self) -> Vec<usize> {
        let mut idxs: Vec<usize> = (0..self.store.telescopes.len()).collect();
        if self.sort_on {
            idxs.sort_by(|&a, &b| self.store.telescopes[a].app.partial_cmp(&self.store.telescopes[b].app)
                .unwrap_or(std::cmp::Ordering::Equal));
        }
        idxs
    }
    fn ordered_ep_indices(&self) -> Vec<usize> {
        let mut idxs: Vec<usize> = (0..self.store.eyepieces.len()).collect();
        if self.sort_on {
            idxs.sort_by(|&a, &b| self.store.eyepieces[a].fl.partial_cmp(&self.store.eyepieces[b].fl)
                .unwrap_or(std::cmp::Ordering::Equal));
        }
        idxs
    }

    /// Index (in the unsorted Vec) of the currently-selected telescope.
    fn current_ts_orig_idx(&self) -> Option<usize> {
        self.ordered_ts_indices().get(self.ts_idx).copied()
    }
    fn current_ep_orig_idx(&self) -> Option<usize> {
        self.ordered_ep_indices().get(self.ep_idx).copied()
    }
    fn current_ts(&self) -> Option<&Telescope> {
        self.current_ts_orig_idx().and_then(|i| self.store.telescopes.get(i))
    }
    fn current_ep(&self) -> Option<&Eyepiece> {
        self.current_ep_orig_idx().and_then(|i| self.store.eyepieces.get(i))
    }

    fn toggle_focus(&mut self) {
        self.focus = if self.focus == Focus::Ts { Focus::Ep } else { Focus::Ts };
    }
    fn move_down(&mut self) {
        match self.focus {
            Focus::Ts => {
                if self.ts_idx + 1 < self.store.telescopes.len() { self.ts_idx += 1; }
            }
            Focus::Ep => {
                if self.ep_idx + 1 < self.store.eyepieces.len() { self.ep_idx += 1; }
            }
        }
    }
    fn move_up(&mut self) {
        match self.focus {
            Focus::Ts => { if self.ts_idx > 0 { self.ts_idx -= 1; } }
            Focus::Ep => { if self.ep_idx > 0 { self.ep_idx -= 1; } }
        }
    }
    fn shift_up(&mut self) {
        // Reorders the underlying Vec when sort is off.
        if self.sort_on { return; }
        match self.focus {
            Focus::Ts => {
                if self.ts_idx > 0 {
                    self.store.telescopes.swap(self.ts_idx, self.ts_idx - 1);
                    self.ts_tagged.swap(self.ts_idx, self.ts_idx - 1);
                    self.ts_idx -= 1;
                }
            }
            Focus::Ep => {
                if self.ep_idx > 0 {
                    self.store.eyepieces.swap(self.ep_idx, self.ep_idx - 1);
                    self.ep_tagged.swap(self.ep_idx, self.ep_idx - 1);
                    self.ep_idx -= 1;
                }
            }
        }
    }
    fn shift_down(&mut self) {
        if self.sort_on { return; }
        match self.focus {
            Focus::Ts => {
                if self.ts_idx + 1 < self.store.telescopes.len() {
                    self.store.telescopes.swap(self.ts_idx, self.ts_idx + 1);
                    self.ts_tagged.swap(self.ts_idx, self.ts_idx + 1);
                    self.ts_idx += 1;
                }
            }
            Focus::Ep => {
                if self.ep_idx + 1 < self.store.eyepieces.len() {
                    self.store.eyepieces.swap(self.ep_idx, self.ep_idx + 1);
                    self.ep_tagged.swap(self.ep_idx, self.ep_idx + 1);
                    self.ep_idx += 1;
                }
            }
        }
    }
    fn go_first(&mut self) {
        match self.focus { Focus::Ts => self.ts_idx = 0, Focus::Ep => self.ep_idx = 0 }
    }
    fn go_last(&mut self) {
        match self.focus {
            Focus::Ts => self.ts_idx = self.store.telescopes.len().saturating_sub(1),
            Focus::Ep => self.ep_idx = self.store.eyepieces.len().saturating_sub(1),
        }
    }
    fn toggle_tag(&mut self) {
        match self.focus {
            Focus::Ts => {
                if let Some(i) = self.current_ts_orig_idx() {
                    if i < self.ts_tagged.len() { self.ts_tagged[i] = !self.ts_tagged[i]; }
                }
            }
            Focus::Ep => {
                if let Some(i) = self.current_ep_orig_idx() {
                    if i < self.ep_tagged.len() { self.ep_tagged[i] = !self.ep_tagged[i]; }
                }
            }
        }
    }
    fn tag_all(&mut self) {
        match self.focus {
            Focus::Ts => self.ts_tagged.iter_mut().for_each(|b| *b = true),
            Focus::Ep => self.ep_tagged.iter_mut().for_each(|b| *b = true),
        }
    }
    fn untag_all(&mut self) {
        self.ts_tagged.iter_mut().for_each(|b| *b = false);
        self.ep_tagged.iter_mut().for_each(|b| *b = false);
    }
    fn toggle_sort(&mut self) {
        self.sort_on = !self.sort_on;
    }

    // --- Edit/add ---

    fn add_telescope(&mut self) {
        let input = self.footer.ask(" Telescope (name,app,fl[,notes]): ", "");
        let parts: Vec<&str> = input.splitn(4, ',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            self.status_say(" Need: name,aperture,focal_length", 196);
            return;
        }
        let name = parts[0].to_string();
        if name.is_empty() { self.status_say(" Name required", 196); return; }
        let app: f64 = match parts[1].parse() { Ok(v) if v > 0.0 => v, _ => { self.status_say(" Bad aperture", 196); return; } };
        let tfl: f64 = match parts[2].parse() { Ok(v) if v > 0.0 => v, _ => { self.status_say(" Bad focal length", 196); return; } };
        let notes = parts.get(3).unwrap_or(&"").to_string();
        self.store.telescopes.push(Telescope { name, app, tfl, notes });
        self.ts_tagged.push(false);
        self.status_say(" Telescope added", 46);
    }

    fn add_eyepiece(&mut self) {
        let input = self.footer.ask(" Eyepiece (name,fl,afov[,notes]): ", "");
        let parts: Vec<&str> = input.splitn(4, ',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            self.status_say(" Need: name,focal_length,afov", 196);
            return;
        }
        let name = parts[0].to_string();
        if name.is_empty() { self.status_say(" Name required", 196); return; }
        let fl: f64 = match parts[1].parse() { Ok(v) if v > 0.0 => v, _ => { self.status_say(" Bad focal length", 196); return; } };
        let afov: f64 = match parts[2].parse() { Ok(v) if v > 0.0 => v, _ => { self.status_say(" Bad AFOV", 196); return; } };
        let notes = parts.get(3).unwrap_or(&"").to_string();
        self.store.eyepieces.push(Eyepiece { name, fl, afov, notes });
        self.ep_tagged.push(false);
        self.status_say(" Eyepiece added", 46);
    }

    fn edit_selected(&mut self) {
        match self.focus {
            Focus::Ts => {
                let Some(i) = self.current_ts_orig_idx() else { return };
                let t = self.store.telescopes[i].clone();
                let initial = format!("{},{},{}{}",
                    t.name, t.app, t.tfl,
                    if t.notes.is_empty() { String::new() } else { format!(",{}", t.notes) });
                let input = self.footer.ask(" Edit telescope (name,app,fl[,notes]): ", &initial);
                let parts: Vec<&str> = input.splitn(4, ',').map(|s| s.trim()).collect();
                if parts.len() < 3 { return; }
                if let (Ok(a), Ok(f)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                    self.store.telescopes[i] = Telescope {
                        name: parts[0].to_string(), app: a, tfl: f,
                        notes: parts.get(3).unwrap_or(&"").to_string(),
                    };
                }
            }
            Focus::Ep => {
                let Some(i) = self.current_ep_orig_idx() else { return };
                let e = self.store.eyepieces[i].clone();
                let initial = format!("{},{},{}{}",
                    e.name, e.fl, e.afov,
                    if e.notes.is_empty() { String::new() } else { format!(",{}", e.notes) });
                let input = self.footer.ask(" Edit eyepiece (name,fl,afov[,notes]): ", &initial);
                let parts: Vec<&str> = input.splitn(4, ',').map(|s| s.trim()).collect();
                if parts.len() < 3 { return; }
                if let (Ok(f), Ok(a)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                    self.store.eyepieces[i] = Eyepiece {
                        name: parts[0].to_string(), fl: f, afov: a,
                        notes: parts.get(3).unwrap_or(&"").to_string(),
                    };
                }
            }
        }
    }

    fn delete_selected(&mut self) {
        let s = self.footer.ask(" Delete selected? (y/n): ", "");
        if s.trim() != "y" && s.trim() != "Y" { return; }
        match self.focus {
            Focus::Ts => {
                if let Some(i) = self.current_ts_orig_idx() {
                    self.store.telescopes.remove(i);
                    if i < self.ts_tagged.len() { self.ts_tagged.remove(i); }
                    if self.ts_idx >= self.store.telescopes.len() {
                        self.ts_idx = self.store.telescopes.len().saturating_sub(1);
                    }
                }
            }
            Focus::Ep => {
                if let Some(i) = self.current_ep_orig_idx() {
                    self.store.eyepieces.remove(i);
                    if i < self.ep_tagged.len() { self.ep_tagged.remove(i); }
                    if self.ep_idx >= self.store.eyepieces.len() {
                        self.ep_idx = self.store.eyepieces.len().saturating_sub(1);
                    }
                }
            }
        }
    }

    fn export_csv(&mut self) {
        let default = format!("{}/scope_export.csv", std::env::var("HOME").unwrap_or_default());
        let path = self.footer.ask(" Export tagged to CSV (path): ", &default);
        if path.trim().is_empty() { return; }
        let mut out = String::from("type,name,aperture_mm,focal_length_mm,afov_deg,notes\n");
        for (i, t) in self.store.telescopes.iter().enumerate() {
            if *self.ts_tagged.get(i).unwrap_or(&false) {
                out.push_str(&format!("telescope,{},{},{},,{}\n", csv_escape(&t.name), t.app, t.tfl, csv_escape(&t.notes)));
            }
        }
        for (i, e) in self.store.eyepieces.iter().enumerate() {
            if *self.ep_tagged.get(i).unwrap_or(&false) {
                out.push_str(&format!("eyepiece,{},,{},{},{}\n", csv_escape(&e.name), e.fl, e.afov, csv_escape(&e.notes)));
            }
        }
        match std::fs::write(path.trim(), out) {
            Ok(_) => self.status_say(&format!(" Exported to {}", path.trim()), 46),
            Err(e) => self.status_say(&format!(" Export failed: {}", e), 196),
        }
    }

    fn export_json(&mut self) {
        let default = format!("{}/scope_export.json", std::env::var("HOME").unwrap_or_default());
        let path = self.footer.ask(" Export all to JSON (path): ", &default);
        if path.trim().is_empty() { return; }
        match serde_json::to_string_pretty(&self.store) {
            Ok(s) => match std::fs::write(path.trim(), s) {
                Ok(_) => self.status_say(&format!(" Exported to {}", path.trim()), 46),
                Err(e) => self.status_say(&format!(" Export failed: {}", e), 196),
            },
            Err(e) => self.status_say(&format!(" Serialize failed: {}", e), 196),
        }
    }

    fn create_observation_log(&mut self) {
        let default = format!("{}/observation_{}.md", std::env::var("HOME").unwrap_or_default(),
            chrono_date());
        let path = self.footer.ask(" Observation log (path): ", &default);
        if path.trim().is_empty() { return; }
        let mut out = String::from("# Observation Log\n\n");
        out.push_str(&format!("Date: {}\n", chrono_date()));
        // Auto-fill conditions from the Sky-mode snapshot taken when the
        // user pressed `g`. Anything missing is just omitted.
        if !self.env.location.is_empty() {
            out.push_str(&format!("Location: {}\n", self.env.location));
        }
        if !self.env.hour_str.is_empty() {
            out.push_str(&format!("Time: {}:00\n", self.env.hour_str));
        }
        if !self.env.weather.is_empty() {
            out.push_str(&format!("Weather: {}\n", self.env.weather));
        }
        if !self.env.moon_summary.is_empty() {
            out.push_str(&format!("Moon: {}\n", self.env.moon_summary));
        }
        if !self.env.visible_bodies.is_empty() {
            out.push_str(&format!("Visible: {}\n", self.env.visible_bodies));
        }
        if self.env.bortle > 0.0 {
            out.push_str(&format!("Bortle: {:.0}\n", self.env.bortle));
        }
        out.push('\n');
        let tagged_ts: Vec<&Telescope> = self.store.telescopes.iter().enumerate()
            .filter(|(i, _)| *self.ts_tagged.get(*i).unwrap_or(&false))
            .map(|(_, t)| t).collect();
        let tagged_ep: Vec<&Eyepiece> = self.store.eyepieces.iter().enumerate()
            .filter(|(i, _)| *self.ep_tagged.get(*i).unwrap_or(&false))
            .map(|(_, e)| e).collect();
        if !tagged_ts.is_empty() {
            out.push_str("## Telescope(s)\n\n");
            for t in &tagged_ts {
                out.push_str(&format!("- **{}** (APP {} mm, FL {} mm, f/{:.1})\n",
                    t.name, t.app, t.tfl, optics::tfr(t.app, t.tfl)));
                if !t.notes.is_empty() { out.push_str(&format!("  {}\n", t.notes)); }
            }
            out.push('\n');
        }
        if !tagged_ep.is_empty() {
            out.push_str("## Eyepiece(s)\n\n");
            for e in &tagged_ep {
                out.push_str(&format!("- **{}** (FL {} mm, AFOV {}°)\n", e.name, e.fl, e.afov));
                if !e.notes.is_empty() { out.push_str(&format!("  {}\n", e.notes)); }
            }
            out.push('\n');
        }
        if !tagged_ts.is_empty() && !tagged_ep.is_empty() {
            out.push_str("## Combinations\n\n| Telescope | Eyepiece | MAGX | TFOV° | Pupil mm |\n|---|---|---|---|---|\n");
            for t in &tagged_ts {
                for e in &tagged_ep {
                    out.push_str(&format!("| {} | {} | {:.0}x | {:.2} | {:.1} |\n",
                        t.name, e.name,
                        optics::magx(t.tfl, e.fl),
                        optics::tfov(t.tfl, e.fl, e.afov),
                        optics::pupl(t.app, t.tfl, e.fl),
                    ));
                }
            }
            out.push('\n');
        }
        out.push_str("## Observations\n\n_Fill in your notes here._\n");
        match std::fs::write(path.trim(), out) {
            Ok(_) => self.status_say(&format!(" Log written to {}", path.trim()), 46),
            Err(e) => self.status_say(&format!(" Write failed: {}", e), 196),
        }
    }

    fn show_version(&mut self) {
        let msg = format!(" astro v{} (Gear mode) - port of telescope-term by Geir Isene", VERSION);
        self.footer.say(&style::fg(&msg, 117));
        let _ = Input::getchr(Some(4));
    }

    fn show_help(&mut self) {
        let help = format!("\n  \
            astro v{} — Gear mode\n  \
            Telescope and eyepiece catalog with optics calculations.\n  \
            Press g to flip back to Sky mode.\n\n  \
            CATALOG\n  \
              t            Add telescope (name, aperture mm, focal length mm[, notes])\n  \
              e            Add eyepiece  (name, focal length mm, AFOV°[, notes])\n  \
              ENTER        Edit selected\n  \
              D            Delete selected\n\n  \
            NAVIGATION\n  \
              UP / DOWN, k / j      Move cursor\n  \
              Shift-UP / Shift-DOWN Reorder\n  \
              HOME / END            Jump to start / end\n  \
              TAB                   Switch panel (telescope ↔ eyepiece)\n  \
              o                     Toggle sort (APP / FL)\n\n  \
            TAGS & EXPORT\n  \
              SPACE        Tag / untag\n  \
              A            Tag all\n  \
              u            Untag all\n  \
              Ctrl-O       Create observation log from tagged equipment\n  \
              x            Export tagged to CSV\n  \
              X            Export all to JSON\n\n  \
            OTHER\n  \
              g            Back to Sky mode\n  \
              r            Redraw\n  \
              v            Show version\n  \
              ?            This help\n  \
              q / Q        Quit astro (save / no save)\n\n  \
            Data: ~/.astro/gear.json   Config: ~/.astro/gear_config.json\n  \
            ESC or q closes this popup.", VERSION);
        let (cols, rows) = Crust::terminal_size();
        let w = cols.saturating_sub(8).min(80);
        let h = rows.saturating_sub(4).min(36);
        let mut popup = crust::Popup::centered(w, h, 252, 234);
        let _ = popup.modal(&help);
        // Wipe the screen so the popup border and any content sitting
        // in the gaps between panes is removed.
        Crust::clear_screen();
        self.header.full_refresh();
        self.ts_head.full_refresh();
        self.ts.full_refresh();
        self.ep_head.full_refresh();
        self.ep.full_refresh();
        self.combo_head.full_refresh();
        self.combo.full_refresh();
        self.footer.full_refresh();
        self.render_all();
    }
}

/// Truncate to char-count `n` (with an ellipsis when shortened) or
/// return the original. Format!'s `{:<N}` uses *byte* width, so a
/// 24-char telescope name with no multibyte chars is fine — but the
/// ellipsis-on-truncate is still nice when a future entry is wider
/// than the column.
fn truncate_or_pad(s: &str, n: usize) -> String {
    let cc = s.chars().count();
    if cc <= n { s.to_string() }
    else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

/// Match the user's eyepiece focal length against the five recommended
/// per-target focal lengths from `optics::e_*` and return the closest
/// label. Helps the user see "this combo is great for galaxies" at a
/// glance instead of mentally translating the mm.
fn classify_target(app: f64, tfl: f64, epfl: f64) -> &'static str {
    let candidates = [
        (super::optics::e_st(app, tfl), "wide star fields"),
        (super::optics::e_gx(app, tfl), "galaxies / nebulae"),
        (super::optics::e_pl(app, tfl), "planets"),
        (super::optics::e_2s(app, tfl), "double stars"),
        (super::optics::e_t2(app, tfl), "tight doubles"),
    ];
    let mut best: (f64, &'static str) = (f64::INFINITY, "—");
    for (target_fl, label) in candidates {
        let dist = (epfl - target_fl).abs();
        if dist < best.0 { best = (dist, label); }
    }
    best.1
}

/// Translate true-field arcminutes into something the brain has a
/// chance with — Andromeda, Pleiades, an Orion Nebula etc. The list
/// is deliberately short; framing dozens of targets becomes a
/// catalogue, not a hint.
fn framing_hint(fov_arcmin: f64) -> String {
    // (object, span in arcmin, label)
    let comps: &[(&str, f64)] = &[
        ("M31 (Andromeda) is 178'", 178.0),
        ("Pleiades cluster is ~110'", 110.0),
        ("M44 (Beehive) is ~95'", 95.0),
        ("Orion Nebula is ~85'", 85.0),
        ("Moon is 31'", 31.0),
        ("Jupiter is ~0.7'", 0.7),
        ("Mars max ~0.4'", 0.4),
    ];
    // Pick the largest object that still fits; otherwise mention the
    // smallest one that overflows the field as a "fits inside" comparison.
    for (label, span) in comps {
        if *span <= fov_arcmin { return format!("   fits {}", label); }
    }
    format!("   narrower than any common target")
}

/// Categorise an exit pupil. Boundary thresholds come from the standard
/// "max useful pupil = ~7mm dark adapted" / "loss of contrast above
/// pupil" / "image too dim below ~0.5mm" rules of thumb.
fn pupil_quality(p: f64) -> (&'static str, u8) {
    if p > 7.0       { ("wider than dark-adapted eye → wasted light", 208) }
    else if p > 5.0  { ("dark-sky deep-field range", 46) }
    else if p > 2.0  { ("comfortable bright-target pupil", 46) }
    else if p > 0.7  { ("high-power planetary / lunar", 117) }
    else if p > 0.4  { ("getting dim — stable atmosphere needed", 220) }
    else if p > 0.0  { ("too dim — image breaks down", 196) }
    else             { ("—", 245) }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn scroll_offset(idx: usize, total: usize, h: usize) -> usize {
    if total <= h { return 0; }
    let half = h / 2;
    if idx < half { 0 }
    else if idx + half >= total { total - h }
    else { idx - half }
}

fn hex_to_256(hex: &str) -> u16 {
    if hex.len() < 6 { return 234; }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as u32;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as u32;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as u32;
    let cv = |c: u32| -> u32 {
        if c < 48 { 0 } else if c < 115 { 1 } else { (c - 35) / 40 }
    };
    (16 + 36 * cv(r) + 6 * cv(g) + cv(b)) as u16
}

fn chrono_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
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
