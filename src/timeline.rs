use crate::model::*;
use crate::theme::Palette;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

#[derive(Default)]
pub struct TimelineState {
    /// visible time window (zoom); None = whole log
    pub window: Option<(i64, i64)>,
    drag_from: Option<f32>,
    pan_from: Option<(f32, i64, i64)>,
    /// moving / resizing the selected time range: (mode, start x, range at start)
    range_drag: Option<(RangeDrag, f32, i64, i64)>,
    hist_key: (u64, i32, i64, i64),
    hist: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum RangeDrag {
    Move,
    Left,
    Right,
}

pub enum TimelineAction {
    None,
    Jump(i64),
    JumpEvent(u32),
    Range(i64, i64),
}

pub struct TimelineInput<'a> {
    pub log: &'a LogData,
    pub view: &'a [u32],
    pub view_gen: u64,
    pub visible: Option<(i64, i64)>,
    pub focus_ts: Option<i64>,
    pub range: Option<(i64, i64)>,
    pub pattern_marks: &'a [(i64, i64, Color32)],
    pub pal: &'a Palette,
}

const TICKS: [i64; 19] = [
    1_000, 2_000, 5_000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000, 600_000, 900_000, 1_800_000,
    3_600_000, 7_200_000, 10_800_000, 21_600_000, 43_200_000, 86_400_000, 172_800_000,
];

impl TimelineState {
    pub fn reset(&mut self) {
        self.window = None;
        self.hist_key = (u64::MAX, 0, 0, 0);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, inp: TimelineInput) -> TimelineAction {
        let pal = inp.pal;
        let log = inp.log;
        let h = 78.0;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let (mut t0, mut t1) = self.window.unwrap_or((log.t_min, log.t_max));
        if t1 <= t0 {
            t1 = t0 + 1000;
        }
        let w = rect.width().max(1.0);
        let to_x = |t: i64, t0: i64, t1: i64| rect.left() + ((t - t0) as f64 / (t1 - t0) as f64 * w as f64) as f32;
        let to_t = |x: f32, t0: i64, t1: i64| t0 + (((x - rect.left()) / w) as f64 * (t1 - t0) as f64) as i64;

        // ---- interaction: zoom / pan ----
        let mut action = TimelineAction::None;
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.raw_scroll_delta.y + i.raw_scroll_delta.x, i.zoom_delta()));
            let factor = if zoom != 1.0 { 1.0 / zoom as f64 } else if scroll != 0.0 { (-scroll as f64 / 400.0).exp() } else { 1.0 };
            if factor != 1.0 {
                if let Some(p) = resp.hover_pos() {
                    let tc = to_t(p.x, t0, t1);
                    let full = (log.t_max - log.t_min).max(1000) as f64;
                    let span = ((t1 - t0) as f64 * factor).clamp(2000.0, full * 1.02);
                    let frac = ((p.x - rect.left()) / w) as f64;
                    let mut n0 = tc - (span * frac) as i64;
                    let mut n1 = n0 + span as i64;
                    if n0 < log.t_min {
                        n1 += log.t_min - n0;
                        n0 = log.t_min;
                    }
                    if n1 > log.t_max {
                        n0 -= n1 - log.t_max;
                        n1 = log.t_max;
                    }
                    n0 = n0.max(log.t_min);
                    if span >= full {
                        self.window = None;
                    } else {
                        self.window = Some((n0, n1));
                    }
                    (t0, t1) = self.window.unwrap_or((log.t_min, log.t_max));
                }
            }
        }
        if resp.drag_started_by(egui::PointerButton::Secondary) || resp.drag_started_by(egui::PointerButton::Middle) {
            if let Some(p) = resp.interact_pointer_pos() {
                self.pan_from = Some((p.x, t0, t1));
            }
        }
        if let (Some((x0, a, b)), Some(p)) = (self.pan_from, resp.interact_pointer_pos()) {
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                let dt = ((p.x - x0) / w) as f64 * (b - a) as f64;
                let mut n0 = a - dt as i64;
                let span = b - a;
                n0 = n0.clamp(log.t_min, (log.t_max - span).max(log.t_min));
                self.window = Some((n0, n0 + span));
                (t0, t1) = (n0, n0 + span);
            }
        }
        if resp.drag_stopped() {
            self.pan_from = None;
        }
        // selected range on screen (for moving / resizing it)
        let range_x = inp.range.map(|(a, b)| (to_x(a, t0, t1), to_x(b, t0, t1)));
        let range_hit = |x: f32| -> Option<RangeDrag> {
            let (x0, x1) = range_x?;
            if (x - x0).abs() <= 5.0 {
                Some(RangeDrag::Left)
            } else if (x - x1).abs() <= 5.0 {
                Some(RangeDrag::Right)
            } else if x > x0 && x < x1 {
                Some(RangeDrag::Move)
            } else {
                None
            }
        };
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            match (origin, inp.range) {
                (Some(o), Some((a, b))) if o.y > rect.top() + 12.0 && range_hit(o.x).is_some() => {
                    self.range_drag = Some((range_hit(o.x).unwrap(), o.x, a, b));
                }
                _ => self.drag_from = origin.map(|p| p.x),
            }
        }
        if let (Some((mode, x0, a0, b0)), Some(p)) = (self.range_drag, resp.interact_pointer_pos()) {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let dt = (((p.x - x0) / w) as f64 * (t1 - t0) as f64) as i64;
                let (mut a, mut b) = match mode {
                    RangeDrag::Move => (a0 + dt, b0 + dt),
                    RangeDrag::Left => ((a0 + dt).min(b0 - 1000), b0),
                    RangeDrag::Right => (a0, (b0 + dt).max(a0 + 1000)),
                };
                if mode == RangeDrag::Move {
                    let width = b0 - a0;
                    if a < log.t_min {
                        a = log.t_min;
                        b = a + width;
                    }
                    if b > log.t_max {
                        b = log.t_max;
                        a = b - width;
                    }
                }
                if Some((a, b)) != inp.range {
                    action = TimelineAction::Range(a, b);
                }
                ui.ctx().set_cursor_icon(if mode == RangeDrag::Move { egui::CursorIcon::Grabbing } else { egui::CursorIcon::ResizeHorizontal });
            }
        }
        if resp.drag_stopped() {
            self.range_drag = None;
        }
        if self.range_drag.is_none() && self.drag_from.is_none() {
            if let Some(hp) = resp.hover_pos() {
                if hp.y > rect.top() + 12.0 {
                    match range_hit(hp.x) {
                        Some(RangeDrag::Move) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                        Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                        None => {}
                    }
                }
            }
        }
        if resp.double_clicked() {
            self.window = None;
            (t0, t1) = (log.t_min, log.t_max);
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                action = TimelineAction::Jump(to_t(p.x, t0, t1));
            }
        }

        // ---- layout ----
        let band = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 12.0));
        let main = Rect::from_min_max(Pos2::new(rect.left(), band.bottom() + 2.0), Pos2::new(rect.right(), rect.bottom() - 20.0));
        let axis_y = main.bottom();
        painter.rect_filled(rect, 6.0, pal.panel);
        painter.rect_filled(main, 0.0, Color32::from_rgb(0xf6, 0xf8, 0xfb));

        // system mode band
        let mut hover_mode: Option<Sym> = None;
        let hover = resp.hover_pos();
        for &(a, b, m) in &log.mode_segments {
            if b < t0 || a > t1 {
                continue;
            }
            let x0 = to_x(a, t0, t1).max(rect.left());
            let x1 = to_x(b, t0, t1).min(rect.right()).max(x0 + 1.0);
            let r = Rect::from_min_max(Pos2::new(x0, band.top() + 2.0), Pos2::new(x1, band.bottom()));
            painter.rect_filled(r, 0.0, pal.mode_color(log.pool.get(m)));
            if let Some(hp) = hover {
                if hp.y <= band.bottom() + 1.0 && hp.x >= x0 && hp.x <= x1 {
                    hover_mode = Some(m);
                }
            }
        }

        // density histogram of the filtered view
        let bins = (w as usize / 2).max(1);
        let key = (inp.view_gen, bins as i32, t0, t1);
        if key != self.hist_key {
            self.hist_key = key;
            self.hist = vec![0; bins];
            let span = (t1 - t0).max(1) as f64;
            for &ei in inp.view {
                let t = log.events[ei as usize].ts;
                if t < t0 || t > t1 {
                    continue;
                }
                let b = (((t - t0) as f64 / span) * bins as f64) as usize;
                self.hist[b.min(bins - 1)] += 1;
            }
        }
        let maxc = *self.hist.iter().max().unwrap_or(&1) as f32;
        let hist_col = Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 60);
        let bw = w / bins as f32;
        for (i, &c) in self.hist.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let hh = ((c as f32).sqrt() / maxc.sqrt()) * (main.height() - 4.0);
            let x = rect.left() + i as f32 * bw;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x, main.bottom() - hh.max(1.5)), Pos2::new(x + bw.max(1.0), main.bottom())),
                0.0,
                hist_col,
            );
        }

        // time range filter overlay
        if let Some((a, b)) = inp.range {
            let x0 = to_x(a, t0, t1).max(rect.left());
            let x1 = to_x(b, t0, t1).min(rect.right());
            if x1 > x0 {
                let r = Rect::from_min_max(Pos2::new(x0, main.top()), Pos2::new(x1, main.bottom()));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 25));
                painter.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0_f32, pal.accent));
                painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0_f32, pal.accent));
            }
        }

        // pattern occurrences
        for &(a, b, c) in inp.pattern_marks {
            if b < t0 || a > t1 {
                continue;
            }
            let x0 = to_x(a, t0, t1);
            let x1 = to_x(b, t0, t1).max(x0 + 3.0);
            let r = Rect::from_min_max(Pos2::new(x0, main.bottom() - 5.0), Pos2::new(x1, main.bottom() - 1.0));
            painter.rect_filled(r, 1.0, c);
        }

        // warnings / errors / fatal markers
        let mut nearest: Option<(f32, u32)> = None;
        for sev in [Severity::Warning, Severity::Error, Severity::Fatal] {
            let col = pal.sev_color(sev);
            let (top, width) = match sev {
                Severity::Warning => (main.top() + 10.0, 2.0_f32),
                Severity::Error => (main.top() + 3.0, 2.0),
                _ => (main.top(), 3.0),
            };
            for &ei in &log.notable {
                let e = &log.events[ei as usize];
                if e.severity != sev || e.ts < t0 || e.ts > t1 {
                    continue;
                }
                let x = to_x(e.ts, t0, t1);
                painter.line_segment([Pos2::new(x, top), Pos2::new(x, main.bottom() - 6.0)], Stroke::new(width, col));
                if let Some(hp) = hover {
                    let d = (hp.x - x).abs();
                    if hp.y > band.bottom() && d < 4.0 && nearest.map_or(true, |(nd, _)| d < nd) {
                        nearest = Some((d, ei));
                    }
                }
            }
        }

        // visible rows window
        if let Some((a, b)) = inp.visible {
            let x0 = to_x(a, t0, t1).clamp(rect.left(), rect.right());
            let x1 = to_x(b, t0, t1).clamp(rect.left(), rect.right()).max(x0 + 2.0);
            let r = Rect::from_min_max(Pos2::new(x0, main.top()), Pos2::new(x1, main.bottom()));
            painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 50));
            painter.rect_stroke(r, 2.0, Stroke::new(1.5_f32, pal.text_weak), StrokeKind::Inside);
        }
        if let Some(t) = inp.focus_ts {
            if t >= t0 && t <= t1 {
                let x = to_x(t, t0, t1);
                painter.line_segment([Pos2::new(x, band.top()), Pos2::new(x, axis_y)], Stroke::new(1.5_f32, pal.text));
            }
        }

        // axis
        painter.line_segment([Pos2::new(rect.left(), axis_y), Pos2::new(rect.right(), axis_y)], Stroke::new(1.0_f32, pal.border));
        let span = t1 - t0;
        let step = TICKS.iter().copied().find(|&s| (s as f64 / span as f64) * w as f64 >= 120.0).unwrap_or(7 * 86_400_000);
        let mut t = (t0.div_euclid(step) + 1) * step;
        let font = FontId::proportional(11.0);
        let mut last_day = i64::MIN;
        while t <= t1 {
            let x = to_x(t, t0, t1);
            painter.line_segment([Pos2::new(x, axis_y), Pos2::new(x, axis_y + 4.0)], Stroke::new(1.0_f32, pal.text_weak));
            painter.line_segment([Pos2::new(x, main.top()), Pos2::new(x, axis_y)], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(128, 128, 128, 22)));
            let d = split_ts(t);
            let day = t.div_euclid(86_400_000);
            let label = if step >= 86_400_000 {
                format!("{} {}", d.d, month_name(d.mo))
            } else if day != last_day || (d.h == 0 && d.mi == 0 && d.s == 0) {
                if step < 60_000 {
                    format!("{} {} {:02}:{:02}:{:02}", d.d, month_name(d.mo), d.h, d.mi, d.s)
                } else {
                    format!("{} {} {:02}:{:02}", d.d, month_name(d.mo), d.h, d.mi)
                }
            } else if step < 60_000 {
                format!("{:02}:{:02}:{:02}", d.h, d.mi, d.s)
            } else {
                format!("{:02}:{:02}", d.h, d.mi)
            };
            last_day = day;
            painter.text(Pos2::new(x, axis_y + 5.0), Align2::CENTER_TOP, label, font.clone(), pal.text_weak);
            t += step;
        }
        if self.window.is_some() {
            painter.text(
                Pos2::new(rect.right() - 6.0, main.top() + 2.0),
                Align2::RIGHT_TOP,
                format!("zoom {}  · double-click = whole log", fmt_duration(span)),
                FontId::proportional(10.5),
                pal.text_weak,
            );
        }

        // drag-to-select range
        if let (Some(x0), Some(p)) = (self.drag_from, resp.interact_pointer_pos()) {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let r = Rect::from_x_y_ranges(x0.min(p.x)..=x0.max(p.x), main.top()..=main.bottom());
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 50));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, pal.accent), StrokeKind::Inside);
            }
            if resp.drag_stopped() {
                if (p.x - x0).abs() > 4.0 {
                    let a = to_t(x0.min(p.x), t0, t1);
                    let b = to_t(x0.max(p.x), t0, t1);
                    action = TimelineAction::Range(a, b);
                }
                self.drag_from = None;
            }
        }

        // hover cursor + tooltip
        if let Some(hp) = hover {
            if self.drag_from.is_none() {
                painter.line_segment([Pos2::new(hp.x, main.top()), Pos2::new(hp.x, axis_y)], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(128, 128, 128, 120)));
            }
            let ht = to_t(hp.x, t0, t1);
            resp.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(egui::RichText::new(fmt_ts(ht)).strong());
                if let Some(m) = hover_mode {
                    ui.label(format!("Mode: {}", log.pool.get(m)));
                }
                if let Some((_, ei)) = nearest {
                    let e = &log.events[ei as usize];
                    ui.label(egui::RichText::new(format!("{}  {}", e.severity.label(), fmt_ts(e.ts))).color(pal.sev_color(e.severity)).strong());
                    ui.label(log.pool.get(e.description));
                }
                ui.label(egui::RichText::new("click = jump · drag = range · wheel = zoom · right-drag = pan").small().color(pal.text_weak));
                if inp.range.is_some() {
                    ui.label(egui::RichText::new("drag inside the selected range = move it · drag its edge = resize").small().color(pal.text_weak));
                }
            });
        }
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, pal.border), StrokeKind::Inside);
        if let (TimelineAction::Jump(_), Some((_, ei))) = (&action, nearest) {
            action = TimelineAction::JumpEvent(ei);
        }
        action
    }
}
