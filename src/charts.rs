//! Charts of numeric values found in the key = value details of events
//! (tube heat / temperatures, kV, mA, DLP, detector temperatures...). Child module of `app`.

use super::*;
use crate::formatter::{self, Block};
use egui_plot::{GridMark, Line, Plot, PlotPoints, Points, VLine};

pub struct Series {
    /// event description with numbers replaced by '#'
    pub event: String,
    pub key: String,
    /// (time ms, value, event index)
    pub points: Vec<(i64, f64, u32)>,
}

impl Series {
    pub fn id(&self) -> String {
        format!("{} › {}", self.event, self.key)
    }
}

pub struct ChartData {
    pub log_key: (PathBuf, Option<String>, usize),
    pub series: Vec<Series>,
    pub by_id: HashMap<String, usize>,
}

pub enum ChartState {
    Building(Receiver<ChartData>),
    Ready(ChartData),
}

const COLORS: [[u8; 3]; 8] = [
    [0x1f, 0x6f, 0xeb],
    [0xe0, 0x4f, 0x2a],
    [0x2a, 0xa1, 0x5a],
    [0x9b, 0x4d, 0xd6],
    [0xd9, 0x8f, 0x00],
    [0x00, 0x99, 0xa8],
    [0xc2, 0x2f, 0x7a],
    [0x5a, 0x6b, 0x80],
];

/// "40", "-3.5", "1%", "77.00 kV" -> number; UIDs, dates, "2*0.625" -> None
fn number(v: &str) -> Option<f64> {
    let v = v.trim();
    let b = v.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start_digits {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') && b.get(i + 1).is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+') {
        i += 2;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let rest = v[i..].trim();
    if rest.len() > 6 || rest.chars().any(|c| c.is_ascii_digit() || c == '.' || c == '/' || c == ':' || c == '*') {
        return None;
    }
    v[..i].parse::<f64>().ok().filter(|x| x.is_finite())
}

fn norm_event(s: &str) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let mut out = String::with_capacity(first.len());
    let mut in_d = false;
    for c in first.chars().take(90) {
        if c.is_ascii_digit() {
            if !in_d {
                out.push('#');
            }
            in_d = true;
        } else {
            in_d = false;
            out.push(c);
        }
    }
    out.replace('*', "")
}

fn build(items: Vec<(u32, i64, String, String)>, log_key: (PathBuf, Option<String>, usize)) -> ChartData {
    let mut map: HashMap<(String, String), Vec<(i64, f64, u32)>> = HashMap::new();
    for (ei, ts, desc, msg) in items {
        let ev = norm_event(&desc);
        for b in formatter::analyze(&msg) {
            if let Block::Table(rows) = b {
                for r in rows {
                    if r.header || r.key.trim().is_empty() {
                        continue;
                    }
                    if let Some(x) = number(&r.value) {
                        map.entry((ev.clone(), r.key.trim().to_owned())).or_default().push((ts, x, ei));
                    }
                }
            }
        }
    }
    let mut series: Vec<Series> = map
        .into_iter()
        .filter(|(_, p)| p.len() >= 3 && p.iter().any(|q| q.1 != p[0].1))
        .map(|((event, key), mut points)| {
            points.sort_by_key(|p| p.0);
            Series { event, key, points }
        })
        .collect();
    series.sort_by(|a, b| a.event.to_lowercase().cmp(&b.event.to_lowercase()).then(a.key.to_lowercase().cmp(&b.key.to_lowercase())));
    let by_id = series.iter().enumerate().map(|(i, s)| (s.id(), i)).collect();
    ChartData { log_key, series, by_id }
}

fn fmt_x(t0: i64, x: f64, with_date: bool) -> String {
    let t = split_ts(t0 + (x * 1000.0) as i64);
    if with_date {
        format!("{} {} {:02}:{:02}:{:02}", t.d, month_name(t.mo), t.h, t.mi, t.s)
    } else {
        format!("{:02}:{:02}:{:02}", t.h, t.mi, t.s)
    }
}

impl App {
    fn chart_log_key(&self) -> Option<(PathBuf, Option<String>, usize)> {
        self.log.as_ref().map(|l| (l.path.clone(), l.entry.clone(), l.events.len()))
    }

    fn start_chart_build(&mut self, ctx: &egui::Context) {
        let Some(log) = &self.log else { return };
        let key = (log.path.clone(), log.entry.clone(), log.events.len());
        let items: Vec<(u32, i64, String, String)> = log
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.message.is_empty())
            .map(|(i, e)| (i as u32, e.ts, log.pool.get(e.description).to_owned(), e.message.clone()))
            .collect();
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(build(items, key));
            ctx2.request_repaint();
        });
        self.charts = Some(ChartState::Building(rx));
    }

    pub(super) fn charts_window(&mut self, ctx: &egui::Context) {
        if !self.show_charts {
            return;
        }
        let pal = self.pal;
        // (re)build when needed
        let key = self.chart_log_key();
        let stale = match &self.charts {
            None => true,
            Some(ChartState::Ready(d)) => Some(&d.log_key) != key.as_ref(),
            Some(ChartState::Building(_)) => false,
        };
        if stale {
            self.start_chart_build(ctx);
        }
        if let Some(ChartState::Building(rx)) = &self.charts {
            if let Ok(d) = rx.try_recv() {
                self.charts = Some(ChartState::Ready(d));
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
        let mut open = true;
        let mut toggle: Option<String> = None;
        let mut jump: Option<u32> = None;
        let mut clear = false;
        let Some(log) = &self.log else { return };
        let t0 = log.t_min;
        let multi_day = log.t_max - log.t_min > 86_400_000;
        let focus_x = self.focus.map(|f| (log.events[f as usize].ts - t0) as f64 / 1000.0);
        let selected = self.cfg.chart_series.clone();
        let q = self.charts_filter.trim().to_lowercase();
        egui::Window::new(RichText::new("📈  Charts").strong())
            .open(&mut open)
            .default_size([1180.0, 740.0])
            .min_width(640.0)
            .min_height(320.0)
            .collapsible(false)
            .show(ctx, |ui| {
                let data = match &self.charts {
                    Some(ChartState::Ready(d)) => d,
                    _ => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Collecting numeric values from the event details…");
                        });
                        return;
                    }
                };
                if data.series.is_empty() {
                    ui.label(RichText::new("No numeric key = value data found in this log.").color(pal.text_weak));
                    return;
                }
                let h = ui.available_height();
                ui.horizontal_top(|ui| {
                    // ---- series list
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.add(egui::TextEdit::singleline(&mut self.charts_filter).hint_text("🔍  filter values…").desired_width(320.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} value series", data.series.len())).small().color(pal.text_weak));
                            if !selected.is_empty() && ui.small_button("× clear").clicked() {
                                clear = true;
                            }
                        });
                        egui::ScrollArea::vertical().id_salt("chart_list").max_height(h - 50.0).auto_shrink([false, false]).show(ui, |ui| {
                            let mut i = 0;
                            while i < data.series.len() {
                                let ev = &data.series[i].event;
                                let mut j = i;
                                while j < data.series.len() && &data.series[j].event == ev {
                                    j += 1;
                                }
                                let group = &data.series[i..j];
                                let ev_match = ev.to_lowercase().contains(&q);
                                let visible: Vec<&Series> = group.iter().filter(|s| q.is_empty() || ev_match || s.key.to_lowercase().contains(&q)).collect();
                                if !visible.is_empty() {
                                    let any_sel = visible.iter().any(|s| selected.contains(&s.id()));
                                    egui::CollapsingHeader::new(RichText::new(format!("{}  ({})", ev, visible.len())).strong())
                                        .id_salt(("chart_ev", ev))
                                        .default_open(any_sel)
                                        .open(if q.is_empty() { None } else { Some(true) })
                                        .show(ui, |ui| {
                                            for s in visible {
                                                let id = s.id();
                                                let mut on = selected.contains(&id);
                                                if ui.checkbox(&mut on, format!("{}   ({})", s.key, s.points.len())).changed() {
                                                    toggle = Some(id);
                                                }
                                            }
                                        });
                                }
                                i = j;
                            }
                        });
                    });
                    ui.separator();
                    // ---- plots
                    ui.vertical(|ui| {
                        let shown: Vec<(usize, &Series)> = selected.iter().filter_map(|id| data.by_id.get(id).map(|&k| (k, &data.series[k]))).collect();
                        if shown.is_empty() {
                            ui.add_space(30.0);
                            ui.label(RichText::new("Tick values on the left to show them over time.").color(pal.text_weak));
                            ui.label(
                                RichText::new("All charts share the time axis · wheel = zoom · drag = pan · double-click = reset · click a point = show that event in the table")
                                    .small()
                                    .color(pal.text_weak),
                            );
                            return;
                        }
                        ui.label(
                            RichText::new("wheel = zoom · drag = pan · double-click = reset · click a point = show that event in the table").small().color(pal.text_weak),
                        );
                        let plot_h = ((h - 40.0) / shown.len() as f32 - 30.0).clamp(130.0, 260.0);
                        egui::ScrollArea::vertical().id_salt("chart_plots").auto_shrink([false, false]).show(ui, |ui| {
                            for (n, (_, s)) in shown.iter().enumerate() {
                                let c = COLORS[n % COLORS.len()];
                                let col = Color32::from_rgb(c[0], c[1], c[2]);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&s.key).strong().color(col));
                                    ui.label(RichText::new(&s.event).small().color(pal.text_weak));
                                    let (mn, mx) = s.points.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
                                    ui.label(RichText::new(format!("min {mn} · max {mx} · {} points", s.points.len())).small().color(pal.text_weak));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.small_button("×").on_hover_text("Remove this chart").clicked() {
                                            toggle = Some(s.id());
                                        }
                                    });
                                });
                                let pts: Vec<[f64; 2]> = s.points.iter().map(|p| [(p.0 - t0) as f64 / 1000.0, p.1]).collect();
                                let key = s.key.clone();
                                let resp = Plot::new(("plot", s.id()))
                                    .height(plot_h)
                                    .link_axis("charts_x", [true, false])
                                    .link_cursor("charts_x", [true, false])
                                    .x_axis_formatter(move |m: GridMark, _r: &std::ops::RangeInclusive<f64>| fmt_x(t0, m.value, multi_day))
                                    .label_formatter(move |_name, p| format!("{}\n{} = {}", fmt_x(t0, p.x, true), key, p.y))
                                    .y_axis_min_width(48.0)
                                    .allow_boxed_zoom(false)
                                    .show(ui, |pui| {
                                        pui.line(Line::new(PlotPoints::from(pts.clone())).color(col).width(1.5_f32));
                                        pui.points(Points::new(PlotPoints::from(pts.clone())).color(col).radius(2.2_f32));
                                        if let Some(fx) = focus_x {
                                            pui.vline(VLine::new(fx).color(pal.text_weak).width(1.0_f32));
                                        }
                                        pui.pointer_coordinate()
                                    });
                                if resp.response.clicked() {
                                    if let Some(pc) = resp.inner {
                                        let tx = t0 + (pc.x * 1000.0) as i64;
                                        if let Some(p) = s.points.iter().min_by_key(|p| (p.0 - tx).abs()) {
                                            jump = Some(p.2);
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                            }
                        });
                    });
                });
            });
        if !open {
            self.show_charts = false;
        }
        if clear {
            self.cfg.chart_series.clear();
            self.save_settings();
        }
        if let Some(id) = toggle {
            if let Some(p) = self.cfg.chart_series.iter().position(|x| *x == id) {
                self.cfg.chart_series.remove(p);
            } else {
                self.cfg.chart_series.push(id);
            }
            self.save_settings();
        }
        if let Some(ei) = jump {
            self.select_occurrence(ctx, &[ei]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number("40"), Some(40.0));
        assert_eq!(number("6.900863"), Some(6.900863));
        assert_eq!(number("1%"), Some(1.0));
        assert_eq!(number("-18.95364"), Some(-18.95364));
        assert_eq!(number("1.3.46.670589.61"), None);
        assert_eq!(number("2*0.625"), None);
        assert_eq!(number("02/05/2024 09:34:40"), None);
        assert_eq!(number("Clinical"), None);
    }
}
