#[path = "files_ui.rs"]
mod files_ui;
#[path = "codes_ui.rs"]
mod codes_ui;
#[path = "charts.rs"]
mod charts;
#[path = "menu_ui.rs"]
mod menu_ui;
#[path = "colors_ui.rs"]
mod colors_ui;
#[path = "update_ui.rs"]
mod update_ui;
#[path = "share_ui.rs"]
mod share_ui;

use crate::config::*;
use crate::model::*;
use crate::parser::{self, Progress};
use crate::patterns::*;
use crate::theme::{rgb, Palette, RowColors};
use crate::timeline::{TimelineAction, TimelineInput, TimelineState};
use eframe::egui::{self, Align, Color32, Id, Key, Layout, RichText, Sense, Stroke};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

// ------------------------------------------------------------------ columns

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Col {
    Bookmark,
    Index,
    Category,
    Time,
    Description,
    EventId,
    SystemMode,
    LogCategory,
    InfoCategory,
    Unit,
    SubUnit,
    Machine,
    Process,
    Component,
    Package,
    Message,
}

impl Col {
    pub const ALL: [Col; 16] = [
        Col::Bookmark,
        Col::Index,
        Col::Category,
        Col::Time,
        Col::Description,
        Col::EventId,
        Col::SystemMode,
        Col::LogCategory,
        Col::InfoCategory,
        Col::Unit,
        Col::SubUnit,
        Col::Machine,
        Col::Process,
        Col::Component,
        Col::Package,
        Col::Message,
    ];
    fn title(self) -> &'static str {
        match self {
            Col::Bookmark => "🔖",
            Col::Index => "Index",
            Col::Category => "Event Category",
            Col::Time => "Time Stamp",
            Col::Description => "Description",
            Col::EventId => "Event ID",
            Col::SystemMode => "System Mode",
            Col::LogCategory => "Log Category",
            Col::InfoCategory => "Info Category",
            Col::Unit => "Unit",
            Col::SubUnit => "SubUnit",
            Col::Machine => "Machine",
            Col::Process => "Process",
            Col::Component => "Component",
            Col::Package => "Package",
            Col::Message => "Additional Info",
        }
    }
    fn width(self) -> f32 {
        match self {
            Col::Bookmark => 26.0,
            Col::Index => 62.0,
            Col::Category => 112.0,
            Col::Time => 196.0,
            Col::Description => 420.0,
            Col::EventId => 140.0,
            Col::SystemMode => 128.0,
            Col::LogCategory => 104.0,
            Col::InfoCategory => 112.0,
            Col::Unit => 64.0,
            Col::SubUnit => 130.0,
            Col::Machine => 170.0,
            Col::Process => 220.0,
            Col::Component => 200.0,
            Col::Package => 150.0,
            Col::Message => 420.0,
        }
    }
    pub fn default_visible(self) -> bool {
        !matches!(self, Col::SubUnit | Col::Machine | Col::Process | Col::Message)
    }
    fn categorical(self) -> bool {
        matches!(
            self,
            Col::Category | Col::EventId | Col::SystemMode | Col::LogCategory | Col::InfoCategory | Col::Unit | Col::SubUnit | Col::Machine | Col::Process | Col::Component | Col::Package
        )
    }
    fn sym(self, e: &Event) -> Option<Sym> {
        Some(match self {
            Col::Category => e.category,
            Col::Description => e.description,
            Col::EventId => e.event_id,
            Col::SystemMode => e.system_mode,
            Col::LogCategory => e.log_cat,
            Col::InfoCategory => e.info_cat,
            Col::Unit => e.unit,
            Col::SubUnit => e.subunit,
            Col::Machine => e.machine,
            Col::Process => e.process,
            Col::Component => e.component,
            Col::Package => e.package,
            _ => return None,
        })
    }
}

fn cell_text(log: &LogData, e: &Event, c: Col) -> String {
    match c {
        Col::Bookmark => String::new(),
        Col::Index => e.index.to_string(),
        Col::Time => fmt_ts(e.ts),
        Col::Message => e.message.clone(),
        _ => log.pool.get(c.sym(e).unwrap()).to_owned(),
    }
}

// ------------------------------------------------------------------ filters

struct TextFilter {
    any: Vec<String>,
    exact: Vec<String>,
    none: Vec<String>,
}

impl TextFilter {
    /// "a|b" = a OR b, "!x" = NOT x, "=x" = exactly x
    fn compile(s: &str) -> Option<Self> {
        let mut any = vec![];
        let mut exact = vec![];
        let mut none = vec![];
        for part in s.split('|') {
            let p = part.trim().to_lowercase();
            if p.is_empty() {
                continue;
            }
            if let Some(n) = p.strip_prefix('!') {
                if !n.trim().is_empty() {
                    none.push(n.trim().to_owned());
                }
            } else if let Some(x) = p.strip_prefix('=') {
                exact.push(x.trim().to_owned());
            } else {
                any.push(p);
            }
        }
        if any.is_empty() && none.is_empty() && exact.is_empty() {
            None
        } else {
            Some(TextFilter { any, exact, none })
        }
    }
    fn matches(&self, hay: &str) -> bool {
        let positive = self.any.is_empty() && self.exact.is_empty()
            || self.any.iter().any(|a| hay.contains(a.as_str()))
            || self.exact.iter().any(|x| hay.trim() == x);
        positive && !self.none.iter().any(|n| hay.contains(n.as_str()))
    }
}

// ------------------------------------------------------------------ app

struct Loading {
    rx: Receiver<Result<LogData, String>>,
    progress: Arc<Progress>,
    name: String,
}

struct PatternEditor {
    p: Pattern,
    is_new: bool,
    error: Option<String>,
}

enum RowAction {
    Click(usize, egui::Modifiers),
    Double(usize),
    Bookmark(usize),
    NewPattern,
    CopyRows,
    FilterBy(usize, Col),
    DescribeCode(usize),
}

pub struct App {
    pal: Palette,
    cfg: GlobalConfig,
    log: Option<LogData>,
    loading: Option<Loading>,
    error: Option<String>,
    // view state
    sort_col: Col,
    sort_asc: bool,
    sorted: Vec<u32>,
    view: Vec<u32>,
    view_pos: Vec<u32>,
    view_gen: u64,
    dirty: bool,
    value_lists: HashMap<Col, Vec<(Sym, usize)>>,
    /// columns with no data in the current log (not shown)
    empty_cols: HashSet<Col>,
    // filters
    sev_on: [bool; 5],
    text_filters: HashMap<Col, String>,
    value_filters: HashMap<Col, HashSet<Sym>>,
    time_range: Option<(i64, i64)>,
    bookmarks_only: bool,
    search: String,
    search_only: bool,
    search_hits: Vec<u32>,
    hit_mask: Vec<bool>,
    // selection
    selected: HashSet<u32>,
    focus: Option<u32>,
    anchor: Option<usize>,
    bookmarks: HashSet<u32>,
    scroll_to: Option<usize>,
    visible_rows: Option<(usize, usize)>,
    timeline: TimelineState,
    // patterns
    patterns: Vec<Pattern>,
    results: Vec<PatternResult>,
    pattern_marks: HashMap<u32, [u8; 3]>,
    pattern_spans: Vec<(i64, i64, Color32)>,
    show_patterns: bool,
    patterns_tab: u8,
    editor: Option<PatternEditor>,
    confirm_delete: Option<u64>,
    toast: Option<(String, f64)>,
    // per-log companion file (bookmarks, notes, view)
    bookmark_notes: HashMap<u32, String>,
    log_note: String,
    log_changed: bool,
    log_save_due: Option<f64>,
    show_bookmarks: bool,
    // other files of the ZIP
    show_files: bool,
    files_filter: String,
    viewer: Option<files_ui::Viewer>,
    viewer_rx: Option<Receiver<Result<files_ui::Viewer, String>>>,
    show_sysinfo: bool,
    show_about: bool,
    // code dictionary
    show_codes: bool,
    codes_filter: String,
    code_edit: Option<codes_ui::CodeEdit>,
    code_syms: HashMap<Sym, String>,
    // charts
    show_charts: bool,
    // menu / settings / merge
    show_settings: bool,
    settings_kind: String,
    merge: Option<menu_ui::MergeState>,
    settings_tab: u8,
    row_colors: RowColors,
    /// categories of the loaded log with row counts
    cat_counts: Vec<(String, usize)>,
    new_category: String,
    /// global config: save shortly after the last change (color pickers change every frame)
    cfg_save_due: Option<f64>,
    upd: update_ui::UpdateState,
    /// problem with the settings file found at startup
    cfg_warning: Option<String>,
    share: share_ui::ShareState,
    charts: Option<charts::ChartState>,
    charts_filter: String,
    pending_new_pattern: bool,
    details_resizing: Option<f32>,
    logo: Option<egui::TextureHandle>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, initial: Option<PathBuf>) -> Self {
        let mut cfg = load_config();
        let patterns = std::mem::take(&mut cfg.patterns);
        let pal = Palette::light();
        pal.apply(&cc.egui_ctx);
        let mut app = App {
            pal,
            cfg,
            log: None,
            loading: None,
            error: None,
            sort_col: Col::Time,
            sort_asc: true,
            sorted: vec![],
            view: vec![],
            view_pos: vec![],
            view_gen: 0,
            dirty: false,
            value_lists: HashMap::new(),
            empty_cols: HashSet::new(),
            sev_on: [true; 5],
            text_filters: HashMap::new(),
            value_filters: HashMap::new(),
            time_range: None,
            bookmarks_only: false,
            search: String::new(),
            search_only: false,
            search_hits: vec![],
            hit_mask: vec![],
            selected: HashSet::new(),
            focus: None,
            anchor: None,
            bookmarks: HashSet::new(),
            scroll_to: None,
            visible_rows: None,
            timeline: TimelineState::default(),
            patterns,
            results: vec![],
            pattern_marks: HashMap::new(),
            pattern_spans: vec![],
            show_patterns: false,
            patterns_tab: 0,
            editor: None,
            confirm_delete: None,
            toast: None,
            bookmark_notes: HashMap::new(),
            log_note: String::new(),
            log_changed: false,
            log_save_due: None,
            show_bookmarks: false,
            show_files: false,
            files_filter: String::new(),
            viewer: None,
            viewer_rx: None,
            show_sysinfo: false,
            show_about: false,
            show_codes: false,
            codes_filter: String::new(),
            code_edit: None,
            code_syms: HashMap::new(),
            show_charts: false,
            show_settings: false,
            settings_kind: String::new(),
            merge: None,
            settings_tab: 0,
            row_colors: RowColors::default(),
            cat_counts: vec![],
            new_category: String::new(),
            cfg_save_due: None,
            upd: Default::default(),
            cfg_warning: None,
            share: Default::default(),
            charts: None,
            charts_filter: String::new(),
            pending_new_pattern: false,
            details_resizing: None,
            logo: eframe::icon_data::from_png_bytes(crate::ICON_PNG).ok().map(|ic| {
                let img = egui::ColorImage::from_rgba_unmultiplied([ic.width as usize, ic.height as usize], &ic.rgba);
                cc.egui_ctx.load_texture("logo", img, egui::TextureOptions::LINEAR)
            }),
        };
        app.cfg_warning = app.cfg.load_warning.take();
        app.apply_colors();
        crate::update::cleanup();
        if app.cfg.check_updates {
            app.start_update_check(false);
        }
        if app.ensure_uids() {
            let _ = app.save_cfg();
        }
        if app.cfg.share_sync {
            app.start_share_sync(false);
        }
        // first start of a new version: show what's new
        if app.cfg.app_version != crate::version::VERSION {
            app.show_about = true;
            app.cfg.app_version = crate::version::VERSION.to_owned();
            app.save_settings();
        }
        if let Some(p) = initial {
            app.open(&cc.egui_ctx, p);
        }
        app
    }

    fn save_cfg(&mut self) -> Result<(), String> {
        self.cfg.patterns = self.patterns.clone();
        let r = save_config(&self.cfg);
        self.cfg.patterns.clear();
        r
    }

    fn save_settings(&mut self) {
        let _ = self.save_cfg();
    }

    // ------------------------------------------------------------ per-log file

    fn log_file_data(&self) -> Option<LogFileData> {
        let log = self.log.as_ref()?;
        let mut bms: Vec<u32> = self.bookmarks.iter().copied().collect();
        bms.sort_by_key(|&i| (log.events[i as usize].ts, i));
        let bookmarks = bms
            .iter()
            .map(|&i| {
                let e = &log.events[i as usize];
                BookmarkRec {
                    index: e.index,
                    time: fmt_ts(e.ts),
                    event_id: log.pool.get(e.event_id).to_owned(),
                    description: log.pool.get(e.description).to_owned(),
                    comment: self.bookmark_notes.get(&i).cloned().unwrap_or_default(),
                }
            })
            .collect();
        let mut text_filters = std::collections::BTreeMap::new();
        for (c, t) in &self.text_filters {
            if !t.trim().is_empty() {
                text_filters.insert(format!("{:?}", c), t.clone());
            }
        }
        let mut value_filters = std::collections::BTreeMap::new();
        for (c, set) in &self.value_filters {
            if !set.is_empty() {
                let mut v: Vec<String> = set.iter().map(|&s| log.pool.get(s).to_owned()).collect();
                v.sort();
                value_filters.insert(format!("{:?}", c), v);
            }
        }
        let hidden_categories = [Severity::Info, Severity::Warning, Severity::Error, Severity::Fatal, Severity::Other]
            .iter()
            .filter(|s| !self.sev_on[**s as usize])
            .map(|s| s.label().to_owned())
            .collect();
        Some(LogFileData {
            app_version: crate::version::VERSION.to_owned(),
            log_file: log.file_name.clone(),
            saved: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            note: self.log_note.clone(),
            bookmarks,
            view: ViewState {
                sort_column: Some(self.sort_col),
                sort_ascending: self.sort_asc,
                hidden_categories,
                text_filters,
                value_filters,
                time_range: self.time_range.map(|(a, b)| [fmt_ts(a), fmt_ts(b)]),
                search: self.search.clone(),
                search_only: self.search_only,
                bookmarks_only: self.bookmarks_only,
            },
        })
    }

    fn save_log_file(&mut self) {
        self.log_save_due = None;
        let Some(d) = self.log_file_data() else { return };
        let path = self.log.as_ref().unwrap().data_file();
        if let Err(e) = save_log_data(&path, &d) {
            self.error = Some(format!("Cannot save {}: {}", path.display(), e));
        }
    }

    fn apply_log_file(&mut self, d: LogFileData) {
        let Some(log) = &self.log else { return };
        let mut by_index: HashMap<i64, Vec<u32>> = HashMap::new();
        for (i, e) in log.events.iter().enumerate() {
            by_index.entry(e.index).or_default().push(i as u32);
        }
        for b in &d.bookmarks {
            let Some(cands) = by_index.get(&b.index) else { continue };
            let ei = cands.iter().copied().find(|&i| fmt_ts(log.events[i as usize].ts) == b.time).unwrap_or(cands[0]);
            self.bookmarks.insert(ei);
            if !b.comment.is_empty() {
                self.bookmark_notes.insert(ei, b.comment.clone());
            }
        }
        self.log_note = d.note;
        let v = d.view;
        if let Some(c) = v.sort_column {
            self.sort_col = c;
        }
        self.sort_asc = v.sort_ascending;
        for s in [Severity::Info, Severity::Warning, Severity::Error, Severity::Fatal, Severity::Other] {
            self.sev_on[s as usize] = !v.hidden_categories.iter().any(|h| h == s.label());
        }
        let col_by_key = |k: &str| Col::ALL.iter().copied().find(|c| format!("{:?}", c) == k);
        for (k, t) in v.text_filters {
            if let Some(c) = col_by_key(&k) {
                self.text_filters.insert(c, t);
            }
        }
        for (k, vals) in v.value_filters {
            if let Some(c) = col_by_key(&k) {
                let set: HashSet<Sym> = vals.iter().filter_map(|x| log.pool.find(x)).collect();
                if !set.is_empty() {
                    self.value_filters.insert(c, set);
                }
            }
        }
        self.time_range = v.time_range.map(|[a, b]| (parse_ts_ms(&a), parse_ts_ms(&b)));
        self.search = v.search;
        self.search_only = v.search_only;
        self.bookmarks_only = v.bookmarks_only;
    }

    fn toggle_bookmark(&mut self, ei: u32) {
        if !self.bookmarks.insert(ei) {
            self.bookmarks.remove(&ei);
            self.bookmark_notes.remove(&ei);
        }
        self.log_changed = true;
        if self.bookmarks_only {
            self.dirty = true;
        }
    }

    fn toast(&mut self, ctx: &egui::Context, s: impl Into<String>) {
        self.toast = Some((s.into(), ctx.input(|i| i.time) + 3.5));
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.log_save_due.is_some() {
            self.save_log_file();
        }
        let (tx, rx) = channel();
        let progress = Progress::new();
        let p2 = progress.clone();
        let ctx2 = ctx.clone();
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(d) = path.parent() {
            self.cfg.last_dir = Some(d.to_path_buf());
            self.save_settings();
        }
        std::thread::spawn(move || {
            let r = parser::load(&path, p2);
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.error = None;
        self.loading = Some(Loading { rx, progress, name });
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut d = rfd::FileDialog::new().add_filter("Log (XML, ZIP, MDB)", &["xml", "zip", "mdb", "accdb"]).add_filter("All files", &["*"]);
        if let Some(dir) = &self.cfg.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_file() {
            self.open(ctx, p);
        }
    }

    fn on_loaded(&mut self, log: LogData) {
        let n = log.events.len();
        let mut lists: HashMap<Col, Vec<(Sym, usize)>> = HashMap::new();
        for c in Col::ALL.iter().copied().filter(|c| c.categorical()) {
            let mut m: HashMap<Sym, usize> = HashMap::new();
            for e in &log.events {
                *m.entry(c.sym(e).unwrap()).or_default() += 1;
            }
            let mut v: Vec<(Sym, usize)> = m.into_iter().collect();
            v.sort_by(|a, b| log.pool.lower(a.0).cmp(log.pool.lower(b.0)));
            lists.insert(c, v);
        }
        self.empty_cols = lists
            .iter()
            .filter(|(_, v)| v.iter().all(|(s, _)| log.pool.get(*s).trim().is_empty()))
            .map(|(c, _)| *c)
            .collect();
        if log.events.iter().all(|e| e.message.is_empty()) {
            self.empty_cols.insert(Col::Message);
        }
        self.value_lists = lists;
        self.log = Some(log);
        self.selected.clear();
        self.bookmarks.clear();
        self.bookmark_notes.clear();
        self.log_note.clear();
        self.focus = None;
        self.anchor = None;
        self.value_filters.clear();
        self.text_filters.clear();
        self.time_range = None;
        self.sev_on = [true; 5];
        self.sort_col = Col::Time;
        self.sort_asc = true;
        self.search.clear();
        self.search_only = false;
        self.bookmarks_only = false;
        self.log_save_due = None;
        let path = self.log.as_ref().unwrap().data_file();
        if let Some(d) = load_log_data(&path) {
            self.apply_log_file(d);
        }
        self.timeline.reset();
        self.hit_mask = vec![false; n];
        self.resort();
        self.rebuild();
        self.refresh_code_syms();
        self.discover_categories();
        self.charts = None;
        self.run_patterns();
        self.scroll_to = Some(0);
        self.log_changed = false;
    }

    fn resort(&mut self) {
        let Some(log) = &self.log else { return };
        let mut v = log.by_time.clone();
        let ev = &log.events;
        match self.sort_col {
            Col::Time => {}
            Col::Index => v.sort_by_key(|&i| ev[i as usize].index),
            Col::Bookmark => v.sort_by_key(|i| !self.bookmarks.contains(i)),
            Col::Message => v.sort_by(|&a, &b| ev[a as usize].message.cmp(&ev[b as usize].message)),
            Col::Category => v.sort_by_key(|&i| ev[i as usize].severity),
            c => v.sort_by(|&a, &b| log.pool.lower(c.sym(&ev[a as usize]).unwrap()).cmp(log.pool.lower(c.sym(&ev[b as usize]).unwrap()))),
        }
        if !self.sort_asc {
            v.reverse();
        }
        self.sorted = v;
        self.dirty = true;
    }

    fn rebuild(&mut self) {
        self.dirty = false;
        self.log_changed = true;
        let Some(log) = &self.log else { return };
        let n = log.events.len();
        let tf: Vec<(Col, TextFilter)> = self
            .text_filters
            .iter()
            .filter_map(|(c, s)| TextFilter::compile(s).map(|f| (*c, f)))
            .collect();
        let vf: Vec<(Col, &HashSet<Sym>)> = self.value_filters.iter().filter(|(_, s)| !s.is_empty()).map(|(c, s)| (*c, s)).collect();
        let q = self.search.trim().to_lowercase();
        if self.hit_mask.len() != n {
            self.hit_mask = vec![false; n];
        } else {
            self.hit_mask.iter_mut().for_each(|x| *x = false);
        }
        // codes whose dictionary description matches the search
        let note_hits: HashSet<Sym> = if q.is_empty() {
            HashSet::new()
        } else {
            self.code_syms.iter().filter(|(_, d)| d.to_lowercase().contains(q.as_str())).map(|(s, _)| *s).collect()
        };
        let mut view = Vec::with_capacity(n);
        let mut hits = Vec::new();
        let mut buf = String::new();
        'outer: for &ei in &self.sorted {
            let e = &log.events[ei as usize];
            if !self.sev_on[e.severity as usize] {
                continue;
            }
            if let Some((a, b)) = self.time_range {
                if e.ts < a || e.ts > b {
                    continue;
                }
            }
            if self.bookmarks_only && !self.bookmarks.contains(&ei) {
                continue;
            }
            for (c, set) in &vf {
                if !set.contains(&c.sym(e).unwrap()) {
                    continue 'outer;
                }
            }
            for (c, f) in &tf {
                let hay: &str = match c {
                    Col::Index => {
                        buf.clear();
                        buf.push_str(&e.index.to_string());
                        &buf
                    }
                    Col::Time => {
                        buf = fmt_ts(e.ts);
                        &buf
                    }
                    Col::Message => {
                        buf = e.message.to_lowercase();
                        &buf
                    }
                    Col::Bookmark => continue,
                    c => log.pool.lower(c.sym(e).unwrap()),
                };
                if !f.matches(hay) {
                    continue 'outer;
                }
            }
            let hit = !q.is_empty() && (e.hay.contains(q.as_str()) || note_hits.contains(&e.event_id));
            if hit {
                self.hit_mask[ei as usize] = true;
            } else if self.search_only && !q.is_empty() {
                continue;
            }
            if hit {
                hits.push(view.len() as u32);
            }
            view.push(ei);
        }
        let mut pos = vec![u32::MAX; n];
        for (i, &ei) in view.iter().enumerate() {
            pos[ei as usize] = i as u32;
        }
        self.view = view;
        self.view_pos = pos;
        self.search_hits = hits;
        self.view_gen += 1;
        if let Some(f) = self.focus {
            let p = self.view_pos[f as usize];
            if p != u32::MAX {
                self.scroll_to = Some(p as usize);
            }
        }
    }

    fn run_patterns(&mut self) {
        let Some(log) = &self.log else { return };
        self.results = match_all(&self.patterns, log);
        self.pattern_marks.clear();
        self.pattern_spans.clear();
        for r in &self.results {
            let Some(p) = self.patterns.iter().find(|p| p.id == r.pattern_id) else { continue };
            for o in &r.occurrences {
                let mut a = i64::MAX;
                let mut b = i64::MIN;
                for &ei in o.events.iter().flatten() {
                    self.pattern_marks.entry(ei).or_insert(p.color);
                    let t = log.events[ei as usize].ts;
                    a = a.min(t);
                    b = b.max(t);
                }
                self.pattern_spans.push((a, b, rgb(p.color)));
            }
        }
    }

    fn total_occurrences(&self) -> usize {
        self.results.iter().map(|r| r.occurrences.len()).sum()
    }

    fn clear_filters(&mut self) {
        self.sev_on = [true; 5];
        self.text_filters.clear();
        self.value_filters.clear();
        self.time_range = None;
        self.bookmarks_only = false;
        self.search_only = false;
        self.dirty = true;
    }

    fn any_filter(&self) -> bool {
        self.sev_on.iter().any(|x| !x)
            || self.text_filters.values().any(|s| !s.trim().is_empty())
            || self.value_filters.values().any(|s| !s.is_empty())
            || self.time_range.is_some()
            || self.bookmarks_only
            || (self.search_only && !self.search.trim().is_empty())
    }

    fn focus_row(&mut self, row: usize, extend: bool) {
        if self.view.is_empty() {
            return;
        }
        let row = row.min(self.view.len() - 1);
        let ei = self.view[row];
        if extend {
            let a = self.anchor.unwrap_or(row);
            self.selected.clear();
            for r in a.min(row)..=a.max(row) {
                self.selected.insert(self.view[r]);
            }
        } else {
            self.selected.clear();
            self.selected.insert(ei);
            self.anchor = Some(row);
        }
        self.focus = Some(ei);
        self.scroll_to = Some(row);
    }

    fn focus_view_row(&self) -> Option<usize> {
        self.focus.and_then(|f| {
            let p = *self.view_pos.get(f as usize)?;
            (p != u32::MAX).then_some(p as usize)
        })
    }

    fn jump_time(&mut self, t: i64) {
        let Some(log) = &self.log else { return };
        if self.view.is_empty() {
            return;
        }
        let ev = &log.events;
        let row = if self.sort_col == Col::Time {
            let p = if self.sort_asc {
                self.view.partition_point(|&i| ev[i as usize].ts < t)
            } else {
                self.view.partition_point(|&i| ev[i as usize].ts > t)
            };
            let p = p.min(self.view.len() - 1);
            if p > 0 && (ev[self.view[p - 1] as usize].ts - t).abs() < (ev[self.view[p] as usize].ts - t).abs() {
                p - 1
            } else {
                p
            }
        } else {
            (0..self.view.len()).min_by_key(|&r| (ev[self.view[r] as usize].ts - t).abs()).unwrap_or(0)
        };
        self.focus_row(row, false);
    }

    fn next_matching(&mut self, forward: bool, pred: impl Fn(&App, u32) -> bool) -> bool {
        match self.find_next(forward, pred) {
            Some(r) => {
                self.focus_row(r, false);
                true
            }
            None => false,
        }
    }

    fn find_next(&self, forward: bool, pred: impl Fn(&App, u32) -> bool) -> Option<usize> {
        let n = self.view.len();
        if n == 0 {
            return None;
        }
        let cur = self.focus_view_row();
        for k in 1..=n {
            let r = match (cur, forward) {
                (Some(c), true) => (c + k) % n,
                (Some(c), false) => (c + n - k % n) % n,
                (None, true) => k - 1,
                (None, false) => n - k,
            };
            if pred(self, self.view[r]) {
                return Some(r);
            }
        }
        None
    }

    fn next_hit(&mut self, forward: bool) {
        if self.search_hits.is_empty() {
            return;
        }
        let cur = self.focus_view_row();
        let hits = &self.search_hits;
        let r = match cur {
            None => hits[if forward { 0 } else { hits.len() - 1 }],
            Some(c) => {
                if forward {
                    let i = hits.partition_point(|&h| (h as usize) <= c);
                    hits[if i >= hits.len() { 0 } else { i }]
                } else {
                    let i = hits.partition_point(|&h| (h as usize) < c);
                    hits[if i == 0 { hits.len() - 1 } else { i - 1 }]
                }
            }
        };
        self.focus_row(r as usize, false);
    }

    fn copy_selected(&self, ctx: &egui::Context) {
        let Some(log) = &self.log else { return };
        let cols: Vec<Col> = self.active_columns().into_iter().filter(|c| *c != Col::Bookmark).collect();
        let mut sel: Vec<u32> = self.selected.iter().copied().collect();
        sel.sort_by_key(|&i| self.view_pos[i as usize]);
        let mut out = cols.iter().map(|c| c.title()).collect::<Vec<_>>().join("\t");
        out.push('\n');
        for ei in sel {
            let e = &log.events[ei as usize];
            let line: Vec<String> = cols.iter().map(|&c| cell_text(log, e, c)).collect();
            out.push_str(&line.join("\t"));
            if !cols.contains(&Col::Message) && !e.message.is_empty() {
                out.push('\t');
                out.push_str(&e.message);
            }
            out.push('\n');
        }
        ctx.copy_text(out);
    }

    fn new_pattern_from_selection(&mut self, ctx: &egui::Context) {
        let Some(log) = &self.log else { return };
        if self.selected.is_empty() {
            self.toast(ctx, "First select one or more rows (Ctrl/Shift + click)");
            return;
        }
        let sel: Vec<u32> = self.selected.iter().copied().collect();
        let steps = steps_from_selection(log, &sel);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let created = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let mut p = Pattern::defaults_from(steps, log.file_name.clone(), created, now as u64);
        let palette = [[255, 153, 51], [0, 200, 180], [90, 160, 255], [255, 90, 160], [160, 220, 60], [200, 120, 255]];
        p.color = palette[self.patterns.len() % palette.len()];
        p.author = self.author();
        self.editor = Some(PatternEditor { p, is_new: true, error: None });
    }

    fn select_occurrence(&mut self, ctx: &egui::Context, events: &[u32]) {
        self.selected = events.iter().copied().collect();
        let mut rows: Vec<usize> = events.iter().filter_map(|&e| {
            let p = self.view_pos[e as usize];
            (p != u32::MAX).then_some(p as usize)
        }).collect();
        rows.sort();
        if rows.len() < events.len() {
            self.toast(ctx, format!("{} of {} rows are hidden by filters", events.len() - rows.len(), events.len()));
        }
        if let Some(&r) = rows.first() {
            self.focus = Some(self.view[r]);
            self.anchor = Some(r);
            self.scroll_to = Some(r);
        } else {
            self.focus = events.first().copied();
        }
    }

    // ------------------------------------------------------------ input

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.log.is_none() || self.editor.is_some() || self.code_edit.is_some() || self.merge.is_some() || !self.cfg.author_asked {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        let (m, keys) = ctx.input(|i| {
            let ks = [
                Key::ArrowDown, Key::ArrowUp, Key::PageDown, Key::PageUp, Key::Home, Key::End, Key::C, Key::A, Key::B, Key::F2, Key::F3, Key::F8,
                Key::F, Key::O, Key::Escape, Key::P,
            ];
            (i.modifiers, ks.iter().copied().filter(|k| i.key_pressed(*k)).collect::<Vec<_>>())
        });
        for k in keys {
            match k {
                Key::F => {
                    if m.command {
                        ctx.memory_mut(|mem| mem.request_focus(Id::new("search_box")));
                    }
                }
                Key::O if m.command => self.open_dialog(ctx),
                Key::F3 => self.next_hit(!m.shift),
                Key::F8 => {
                    if !self.next_matching(!m.shift, is_problem) {
                        self.toast(ctx, "No warnings/errors in the current view");
                    }
                }
                Key::F2 => {
                    if !self.next_matching(!m.shift, |a, ei| a.bookmarks.contains(&ei)) {
                        self.toast(ctx, "No bookmarked rows (Ctrl+B)");
                    }
                }
                _ if typing => {}
                Key::ArrowDown | Key::ArrowUp | Key::PageDown | Key::PageUp | Key::Home | Key::End => {
                    let n = self.view.len();
                    if n == 0 {
                        continue;
                    }
                    let cur = self.focus_view_row().unwrap_or(0) as i64;
                    let r = match k {
                        Key::ArrowDown => cur + 1,
                        Key::ArrowUp => cur - 1,
                        Key::PageDown => cur + 30,
                        Key::PageUp => cur - 30,
                        Key::Home => 0,
                        _ => n as i64 - 1,
                    }
                    .clamp(0, n as i64 - 1) as usize;
                    let anchor = self.anchor;
                    self.focus_row(r, m.shift);
                    if m.shift {
                        self.anchor = anchor.or(Some(cur as usize));
                    }
                }
                Key::C if m.command => {
                    if !self.selected.is_empty() {
                        self.copy_selected(ctx);
                        let n = self.selected.len();
                        self.toast(ctx, format!("Copied {} rows", n));
                    }
                }
                Key::A if m.command => {
                    self.selected = self.view.iter().copied().collect();
                }
                Key::B if m.command => {
                    let sel: Vec<u32> = self.selected.iter().copied().collect();
                    let add = sel.iter().any(|e| !self.bookmarks.contains(e));
                    for e in sel {
                        if add {
                            self.bookmarks.insert(e);
                        } else {
                            self.bookmarks.remove(&e);
                            self.bookmark_notes.remove(&e);
                        }
                    }
                    self.log_changed = true;
                    if self.bookmarks_only {
                        self.dirty = true;
                    }
                }
                Key::P if m.command => self.new_pattern_from_selection(ctx),
                Key::Escape => {
                    self.selected.clear();
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ UI parts

    fn top_bar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(10, 8)).stroke(Stroke::new(1.0_f32, pal.border)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.main_menu(ui, ctx);
                    ui.add_space(4.0);
                    if let Some(t) = &self.logo {
                        ui.add(egui::Image::new((t.id(), egui::vec2(28.0, 28.0))));
                    }
                    let r = ui.add(egui::Label::new(RichText::new("Philips Log Viewer").strong().size(15.0)).sense(Sense::click()));
                    let v = ui.add(egui::Label::new(RichText::new(format!("v{}", crate::version::VERSION)).small().color(pal.text_weak)).sense(Sense::click()));
                    if r.on_hover_text("About · What's new").clicked() || v.on_hover_text("About · What's new").clicked() {
                        self.show_about = !self.show_about;
                    }
                    ui.add_space(8.0);
                    if let Some(log) = &self.log {
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if log.entry.is_some() {
                                let zipname = log.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                ui.label(RichText::new(format!("{zipname}  ›  {}", log.file_name)).strong());
                            } else {
                                ui.label(RichText::new(&log.file_name).strong());
                            }
                            let i = &log.info;
                            let product = format!("{} {}", i.family, if i.model != i.family { i.model.as_str() } else { "" }).trim().to_owned();
                            let mut parts: Vec<String> = vec![];
                            if !product.is_empty() {
                                parts.push(product);
                            }
                            if !i.version.is_empty() {
                                parts.push(format!("v{}", i.version));
                            }
                            if !i.serial.is_empty() {
                                parts.push(format!("SN {}", i.serial));
                            }
                            if !i.modality.is_empty() && !i.family.contains(&i.modality) {
                                parts.push(i.modality.clone());
                            }
                            let mut sub = parts.join(" · ");
                            let place: Vec<&str> = [i.hospital.as_str(), i.room.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
                            if !place.is_empty() {
                                if !sub.is_empty() {
                                    sub.push_str(" · ");
                                }
                                sub.push_str(&place.join(", "));
                            }
                            ui.label(RichText::new(sub).small().color(pal.text_weak));
                        });
                    }
                    let mut back = false;
                    if let Some(log) = &self.log {
                        if log.entry.is_some() && ui.button("↩ Main log").on_hover_text("Back to the system event log of this ZIP").clicked() {
                            back = true;
                        }
                    }
                    if back {
                        self.back_to_main(ctx);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.update_badge(ui);
                        // patterns button: changes color when something is recognized
                        let occ = self.total_occurrences();
                        let recognized = self.results.len();
                        let btn = if occ > 0 {
                            egui::Button::new(
                                RichText::new(format!("🎯  Patterns  ·  {} recognized ({}×)", recognized, occ)).color(Color32::BLACK).strong(),
                            )
                            .fill(pal.pattern)
                            .stroke(Stroke::new(1.0_f32, pal.pattern))
                        } else {
                            egui::Button::new(format!("🎯  Patterns ({})", self.patterns.len()))
                        };
                        let r = ui.add(btn).on_hover_text("Pattern library and patterns recognized in this log");
                        if r.clicked() {
                            self.show_patterns = !self.show_patterns;
                            self.patterns_tab = if occ > 0 { 0 } else { 1 };
                        }
                        let ns = self.selected.len();
                        let b = ui.add_enabled(ns > 0 && self.log.is_some(), egui::Button::new("➕ Add pattern"));
                        if b.on_hover_text("Select rows (Ctrl/Shift + click) and save them as a pattern (Ctrl+P)").clicked() {
                            self.new_pattern_from_selection(ctx);
                        }
                        ui.add_space(10.0);

                        // search
                        if self.log.is_some() {
                            let mut only = self.search_only;
                            if ui.toggle_value(&mut only, "⛃ Matches only").on_hover_text("Show only rows containing the search text").changed() {
                                self.search_only = only;
                                self.dirty = true;
                            }
                            if ui.small_button("▼").on_hover_text("Next match (Enter / F3)").clicked() {
                                self.next_hit(true);
                            }
                            if ui.small_button("▲").on_hover_text("Previous match (Shift+F3)").clicked() {
                                self.next_hit(false);
                            }
                            if !self.search.trim().is_empty() {
                                let txt = if self.search_hits.is_empty() {
                                    RichText::new("0 matches").color(pal.sev_err)
                                } else {
                                    let cur = self.focus_view_row().and_then(|c| self.search_hits.binary_search(&(c as u32)).ok()).map(|i| format!("{}/", i + 1)).unwrap_or_default();
                                    RichText::new(format!("{}{}", cur, fmt_num(self.search_hits.len()))).color(pal.text_weak)
                                };
                                ui.label(txt);
                            }
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .id(Id::new("search_box"))
                                    .hint_text("🔍  Search… (Ctrl+F)")
                                    .desired_width(260.0),
                            );
                            if r.changed() {
                                self.dirty = true;
                            }
                            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                if self.dirty {
                                    self.rebuild();
                                }
                                let back = ui.input(|i| i.modifiers.shift);
                                self.next_hit(!back);
                                r.request_focus();
                            }
                        }
                    });
                });
            });
    }

    fn filter_bar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let counts = log.counts;
        let total = log.events.len();
        let any_filter = self.any_filter();
        let mut nav: Option<bool> = None;
        egui::TopBottomPanel::top("filters")
            .frame(egui::Frame::new().fill(pal.bg).inner_margin(egui::Margin { left: 10, right: 10, top: 8, bottom: 4 }))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for s in [Severity::Info, Severity::Warning, Severity::Error, Severity::Fatal, Severity::Other] {
                        let c = counts[s as usize];
                        if s == Severity::Other && c == 0 {
                            continue;
                        }
                        let on = self.sev_on[s as usize];
                        let col = pal.sev_color(s);
                        let text = RichText::new(format!("●  {}  {}", s.label(), fmt_num(c))).color(if on { pal.text } else { pal.text_weak });
                        let fill = if on {
                            Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 60)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let b = egui::Button::new(text).fill(fill).stroke(Stroke::new(1.0_f32, if on { col } else { pal.border })).corner_radius(12.0);
                        let r = ui.add(b).on_hover_text("Click = show/hide  ·  right-click = only this category");
                        if r.clicked() {
                            self.sev_on[s as usize] = !on;
                            self.dirty = true;
                        } else if r.secondary_clicked() {
                            self.sev_on = [false; 5];
                            self.sev_on[s as usize] = true;
                            self.dirty = true;
                        }
                    }
                    ui.add_space(6.0);
                    if ui.button("⏮").on_hover_text("Previous warning/error (Shift+F8)").clicked() {
                        nav = Some(false);
                    }
                    if ui.button("⏭").on_hover_text("Next warning/error (F8)").clicked() {
                        nav = Some(true);
                    }
                    ui.separator();
                    let mut bo = self.bookmarks_only;
                    if ui.toggle_value(&mut bo, format!("🔖 Bookmarked ({})", self.bookmarks.len())).on_hover_text("Ctrl+B bookmark · F2 next").changed() {
                        self.bookmarks_only = bo;
                        self.dirty = true;
                    }
                    if ui.button("📋 List & notes").on_hover_text("Bookmarks with comments and notes for this log").clicked() {
                        self.show_bookmarks = !self.show_bookmarks;
                    }
                    if let Some((a, b)) = self.time_range {
                        let t = format!("⏱ {} → {}   ×", fmt_ts_short(a), fmt_ts_short(b));
                        if ui.add(egui::Button::new(RichText::new(t).color(pal.accent)).stroke(Stroke::new(1.0_f32, pal.accent))).on_hover_text("Remove time range").clicked() {
                            self.time_range = None;
                            self.dirty = true;
                        }
                    }
                    if any_filter && ui.button("× Clear filters").clicked() {
                        self.clear_filters();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("showing {} / {}", fmt_num(self.view.len()), fmt_num(total))).color(pal.text_weak));
                    });
                });
            });
        if let Some(fwd) = nav {
            if !self.next_matching(fwd, is_problem) {
                self.toast(ctx, "No warnings/errors in the current view");
            }
        }
    }

    fn timeline_bar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let mut action = TimelineAction::None;
        egui::TopBottomPanel::top("timeline")
            .frame(egui::Frame::new().fill(pal.bg).inner_margin(egui::Margin { left: 10, right: 10, top: 2, bottom: 6 }))
            .show(ctx, |ui| {
                let visible = self.visible_rows.and_then(|(a, b)| {
                    if self.view.is_empty() {
                        return None;
                    }
                    let mut lo = i64::MAX;
                    let mut hi = i64::MIN;
                    for r in a..=b.min(self.view.len() - 1) {
                        let t = log.events[self.view[r] as usize].ts;
                        lo = lo.min(t);
                        hi = hi.max(t);
                    }
                    (lo <= hi).then_some((lo, hi))
                });
                action = self.timeline.ui(
                    ui,
                    TimelineInput {
                        log,
                        view: &self.view,
                        view_gen: self.view_gen,
                        visible,
                        focus_ts: self.focus.map(|f| log.events[f as usize].ts),
                        range: self.time_range,
                        pattern_marks: &self.pattern_spans,
                        pal: &pal,
                    },
                );
            });
        match action {
            TimelineAction::None => {}
            TimelineAction::Jump(t) => self.jump_time(t),
            TimelineAction::JumpEvent(ei) => {
                let p = self.view_pos[ei as usize];
                if p != u32::MAX {
                    self.focus_row(p as usize, false);
                } else {
                    let t = self.log.as_ref().unwrap().events[ei as usize].ts;
                    self.jump_time(t);
                }
            }
            TimelineAction::Range(a, b) => {
                self.time_range = Some((a, b));
                self.dirty = true;
            }
        }
    }

    fn details_panel(&mut self, ctx: &egui::Context) {
        if self.log.is_none() {
            return;
        }
        let pal = self.pal;
        let frame = egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(12, 8)).stroke(Stroke::new(1.0_f32, pal.border));
        let size = self.cfg.details_size;
        let mut new_size: Option<f32> = None;
        let panel_id = match self.cfg.details_dock {
            DetailsDock::Bottom => "details_bottom",
            DetailsDock::Left => "details_left",
            DetailsDock::Right => "details_right",
            DetailsDock::Window => "details_window",
        };
        let resize_dragged = ctx.read_response(Id::new(panel_id).with("__resize")).is_some_and(|r| r.dragged());
        match self.cfg.details_dock {
            DetailsDock::Bottom => {
                let r = egui::TopBottomPanel::bottom("details_bottom")
                    .resizable(true)
                    .default_height(size.clamp(80.0, 2000.0))
                    .min_height(80.0)
                    .frame(frame)
                    .show(ctx, |ui| self.details_ui(ui));
                new_size = Some(r.response.rect.height());
            }
            DetailsDock::Left | DetailsDock::Right => {
                let p = if self.cfg.details_dock == DetailsDock::Left {
                    egui::SidePanel::left("details_left")
                } else {
                    egui::SidePanel::right("details_right")
                };
                let r = p
                    .resizable(true)
                    .default_width(self.cfg.details_width.clamp(260.0, 3000.0))
                    .min_width(260.0)
                    .frame(frame)
                    .show(ctx, |ui| self.details_ui(ui));
                new_size = Some(r.response.rect.width());
            }
            DetailsDock::Window => {
                let vid = egui::ViewportId::from_hash_of("details_window");
                let builder = egui::ViewportBuilder::default()
                    .with_title("Event details – Philips Log Viewer")
                    .with_inner_size([760.0, 560.0])
                    .with_min_inner_size([320.0, 200.0])
                    .with_icon(std::sync::Arc::new(crate::icon()));
                let mut closed = false;
                ctx.show_viewport_immediate(vid, builder, |vctx, _class| {
                    egui::CentralPanel::default().frame(frame).show(vctx, |ui| self.details_ui(ui));
                    if vctx.input(|i| i.viewport().close_requested()) {
                        closed = true;
                    }
                });
                if closed {
                    self.cfg.details_dock = DetailsDock::Bottom;
                    self.save_settings();
                }
            }
        }
        // remember the size the user dragged the panel to
        if let Some(sz) = new_size {
            if resize_dragged {
                self.details_resizing = Some(sz);
            } else {
                if let Some(sz) = self.details_resizing.take() {
                    if self.cfg.details_dock == DetailsDock::Bottom {
                        self.cfg.details_size = sz;
                    } else {
                        self.cfg.details_width = sz;
                    }
                    self.save_settings();
                }
            }
        }
    }

    /// Content of the details panel (same for every dock position).
    fn details_ui(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let mut make_pattern = false;
        let mut edit_code: Option<String> = None;
        let mut bm_toggle: Option<u32> = None;
        let mut note_changed = false;
        let mut dock: Option<DetailsDock> = None;
        let mut raw = self.cfg.details_raw;
        // always fill the size the user gave the panel (egui panels otherwise shrink to their content)
        ui.set_min_size(ui.available_size());
        let Some(log) = &self.log else { return };

        // toolbar: view mode + dock position
        ui.horizontal(|ui| {
            ui.label(RichText::new("Details").strong());
            ui.separator();
            ui.selectable_value(&mut raw, false, "Formatted").on_hover_text("Line breaks, key = value tables, exception analysis");
            ui.selectable_value(&mut raw, true, "Raw").on_hover_text("Text exactly as written in the log");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let cur = self.cfg.details_dock;
                for (d, icon, tip) in [
                    (DetailsDock::Window, "🗖", "Separate window (can be moved outside the main window)"),
                    (DetailsDock::Right, "▶", "Dock right"),
                    (DetailsDock::Bottom, "▼", "Dock bottom"),
                    (DetailsDock::Left, "◀", "Dock left"),
                ] {
                    if ui.selectable_label(cur == d, icon).on_hover_text(tip).clicked() && cur != d {
                        dock = Some(d);
                    }
                }
            });
        });
        ui.separator();

        let Some(fe) = self.focus else {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Click a row to see details  ·  Ctrl/Shift + click for multiple selection").color(pal.text_weak));
            });
            self.apply_details_actions(raw, dock, None, false, false);
            return;
        };
        let e = &log.events[fe as usize];
        let desc = log.pool.get(e.description);
        let first_line = desc.lines().next().unwrap_or("");
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // header: one line only, full text below
            ui.horizontal(|ui| {
                let c = pal.sev_color(e.severity);
                ui.label(RichText::new(format!("● {}", log.pool.get(e.category))).color(c).strong());
                ui.label(RichText::new(fmt_ts(e.ts)).monospace());
                ui.label(RichText::new(log.pool.get(e.event_id)).monospace().color(pal.text_weak));
            });
            if let Some(c) = App::code_note_ui(&self.cfg.codes, pal, ui, e, log) {
                edit_code = Some(c);
            }
            if raw {
                ui.add(egui::Label::new(RichText::new(first_line).strong().size(14.5)).wrap().selectable(true));
            } else {
                let mut job = crate::formatter::emph_job(ui, first_line, pal.text, pal.accent);
                for sec in job.sections.iter_mut() {
                    sec.format.font_id.size = 14.5;
                }
                ui.add(egui::Label::new(job).wrap().selectable(true));
            }
            if self.selected.len() > 1 {
                ui.horizontal(|ui| {
                    let ts: Vec<i64> = self.selected.iter().map(|&i| log.events[i as usize].ts).collect();
                    let span = ts.iter().max().unwrap() - ts.iter().min().unwrap();
                    ui.label(RichText::new(format!("{} rows selected · span {}", self.selected.len(), fmt_duration(span))).color(pal.accent));
                    if ui.button("➕ Create pattern").clicked() {
                        make_pattern = true;
                    }
                });
            }
            ui.horizontal(|ui| {
                if self.bookmarks.contains(&fe) {
                    if ui.button(RichText::new("🔖").color(pal.accent)).on_hover_text("Remove bookmark").clicked() {
                        bm_toggle = Some(fe);
                    }
                    let note = self.bookmark_notes.entry(fe).or_default();
                    if ui.add(egui::TextEdit::singleline(note).hint_text("Bookmark comment…").desired_width(f32::INFINITY)).changed() {
                        note_changed = true;
                    }
                } else if ui.small_button("🔖 Bookmark (Ctrl+B / double-click)").clicked() {
                    bm_toggle = Some(fe);
                }
            });
            ui.add_space(4.0);

            // description in full when it has more than the header shows
            let multi = desc.len() > first_line.len() + 1;
            let escaped = desc.contains("\\n");
            let rich = !raw && crate::formatter::analyze(desc).iter().any(|b| !matches!(b, crate::formatter::Block::Para(_)));
            if multi || escaped || rich || (raw && desc.len() > 80) {
                ui.add_space(6.0);
                ui.label(RichText::new("Description").color(pal.text_weak));
                crate::formatter::show(ui, Id::new(("desc", fe)), desc, raw, &pal);
            }
            if !e.message.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Additional Info").color(pal.text_weak));
                crate::formatter::show(ui, Id::new(("msg", fe)), &e.message, raw, &pal);
            }
            ui.add_space(6.0);
            // fields: 4 pairs per row when wide, otherwise 1
            let mut fields: Vec<(String, String)> = vec![
                ("Index".into(), e.index.to_string()),
                ("System Mode".into(), log.pool.get(e.system_mode).into()),
                ("Log Category".into(), log.pool.get(e.log_cat).into()),
                ("Info Category".into(), log.pool.get(e.info_cat).into()),
                ("Machine".into(), log.pool.get(e.machine).into()),
                ("Unit / SubUnit".into(), format!("{} / {}", log.pool.get(e.unit), log.pool.get(e.subunit))),
                ("Component".into(), log.pool.get(e.component).into()),
                ("Package".into(), log.pool.get(e.package).into()),
                ("Process".into(), log.pool.get(e.process).into()),
                ("PID / TID".into(), format!("{} / {}", e.pid, e.tid)),
            ];
            for (k, v) in &e.extras {
                fields.push((log.pool.get(*k).to_owned(), v.clone()));
            }
            fields.retain(|(_, v)| !v.trim().is_empty() && v.trim() != "/" && v.trim() != "0 / 0");
            let per_row = if ui.available_width() > 1100.0 { 4 } else if ui.available_width() > 640.0 { 2 } else { 1 };
            egui::Grid::new("det_fields").num_columns(per_row * 2).spacing([14.0, 3.0]).show(ui, |ui| {
                for (i, (k, v)) in fields.iter().enumerate() {
                    ui.label(RichText::new(k).color(pal.text_weak));
                    if v.len() > 60 {
                        ui.add(egui::Label::new(v.as_str()).wrap().selectable(true));
                    } else {
                        ui.add(egui::Label::new(v.as_str()).extend().selectable(true));
                    }
                    if (i + 1) % per_row == 0 {
                        ui.end_row();
                    }
                }
                ui.end_row();
            });

        });
        let toggle = bm_toggle;
        self.apply_details_actions(raw, dock, toggle, note_changed, make_pattern);
        if let Some(c) = edit_code {
            self.open_code_editor(&c);
        }
    }

    fn apply_details_actions(&mut self, raw: bool, dock: Option<DetailsDock>, bm_toggle: Option<u32>, note_changed: bool, make_pattern: bool) {
        if raw != self.cfg.details_raw {
            self.cfg.details_raw = raw;
            self.save_settings();
        }
        if let Some(d) = dock {
            self.cfg.details_dock = d;
            self.save_settings();
        }
        if let Some(e) = bm_toggle {
            self.toggle_bookmark(e);
        }
        if note_changed {
            self.log_changed = true;
        }
        if make_pattern {
            self.pending_new_pattern = true;
        }
    }

    fn bookmarks_window(&mut self, ctx: &egui::Context) {
        if !self.show_bookmarks {
            return;
        }
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let mut open = true;
        let mut jump: Option<u32> = None;
        let mut remove: Option<u32> = None;
        let mut changed = false;
        let mut bms: Vec<u32> = self.bookmarks.iter().copied().collect();
        bms.sort_by_key(|&i| (log.events[i as usize].ts, i));
        let file = log.data_file();
        egui::Window::new(RichText::new("🔖  Bookmarks & notes").strong())
            .open(&mut open)
            .default_size([640.0, 500.0])
            .min_width(420.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new("Notes for this log").strong());
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.log_note)
                            .hint_text("Problem description, what was done, conclusion…")
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Bookmarks ({})", bms.len())).strong());
                if bms.is_empty() {
                    ui.label(RichText::new("No bookmarks. Double-click a row or press Ctrl+B.").color(pal.text_weak));
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 24.0).show(ui, |ui| {
                    for &ei in &bms {
                        let e = &log.events[ei as usize];
                        egui::Frame::new().fill(pal.panel2).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(fmt_ts(e.ts)).monospace());
                                let mut t = RichText::new(log.pool.get(e.description)).strong();
                                if e.severity != Severity::Info {
                                    t = t.color(pal.sev_color(e.severity));
                                }
                                ui.add(egui::Label::new(t).truncate().selectable(false));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("🗑").on_hover_text("Remove bookmark").clicked() {
                                        remove = Some(ei);
                                    }
                                    if ui.small_button("👁 Go").clicked() {
                                        jump = Some(ei);
                                    }
                                });
                            });
                            let note = self.bookmark_notes.entry(ei).or_default();
                            if ui.add(egui::TextEdit::singleline(note).hint_text("comment…").desired_width(f32::INFINITY)).changed() {
                                changed = true;
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
                ui.label(RichText::new(format!("Saved automatically to: {}", file.display())).small().color(pal.text_weak));
            });
        if !open {
            self.show_bookmarks = false;
        }
        if changed {
            self.log_changed = true;
        }
        if let Some(e) = remove {
            self.toggle_bookmark(e);
        }
        if let Some(e) = jump {
            self.select_occurrence(ctx, &[e]);
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        egui::Window::new(RichText::new("About Philips Log Viewer").strong())
            .open(&mut open)
            .default_size([560.0, 520.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(t) = &self.logo {
                        ui.add(egui::Image::new((t.id(), egui::vec2(56.0, 56.0))));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Philips Log Viewer").strong().size(20.0));
                        ui.label(RichText::new(format!("Version {}  ·  built {}", crate::version::VERSION, crate::version::BUILD_DATE)).color(pal.text_weak));
                        ui.label(RichText::new(format!("Settings & patterns: {}", config_path().display())).small().color(pal.text_weak));
                    });
                });
                ui.separator();
                ui.label(RichText::new("What's new").strong());
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (i, r) in crate::version::CHANGELOG.iter().enumerate() {
                        egui::CollapsingHeader::new(RichText::new(format!("{}   ·   {}", r.version, r.date)).strong().color(if i == 0 { pal.accent } else { pal.text }))
                            .id_salt(("rel", r.version))
                            .default_open(i == 0)
                            .show(ui, |ui| {
                                for c in r.changes {
                                    ui.horizontal_top(|ui| {
                                        ui.label(RichText::new("•").color(pal.accent));
                                        ui.add(egui::Label::new(*c).wrap());
                                    });
                                }
                            });
                    }
                });
            });
        if !open {
            self.show_about = false;
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::new().fill(pal.bg).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let small = |t: String| RichText::new(t).small().color(pal.text_weak);
                    if let Some(log) = &self.log {
                        ui.label(small(format!("{} events loaded in {} ms", fmt_num(log.events.len()), log.load_ms)));
                        ui.label(small("·".into()));
                        ui.label(small(format!("{} → {}  ({})", fmt_ts_short(log.t_min), fmt_ts_short(log.t_max), fmt_duration(log.t_max - log.t_min))));
                        if !self.selected.is_empty() {
                            ui.label(small("·".into()));
                            ui.label(RichText::new(format!("selected: {}", self.selected.len())).small().color(pal.accent));
                        }
                    } else {
                        ui.label(small("Ready".into()));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(small("F3 search · F8 errors · F2 bookmarks · Ctrl+B bookmark · Ctrl+C copy · Ctrl+P pattern".into()));
                    });
                });
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pal = self.pal;
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.25);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(460.0, 230.0), Sense::click());
                let stroke_col = if hovering || resp.hovered() { pal.accent } else { pal.border };
                ui.painter().rect_filled(r, 14.0, pal.panel);
                ui.painter().rect_stroke(r, 14.0, Stroke::new(if hovering { 2.5_f32 } else { 1.5 }, stroke_col), egui::StrokeKind::Inside);
                let p = ui.painter();
                p.text(r.center() - egui::vec2(0.0, 52.0), egui::Align2::CENTER_CENTER, "📄", egui::FontId::proportional(44.0), pal.accent);
                p.text(r.center() + egui::vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, "Open a log file", egui::FontId::proportional(20.0), pal.text);
                p.text(
                    r.center() + egui::vec2(0.0, 36.0),
                    egui::Align2::CENTER_CENTER,
                    "Drop an .xml or .zip here, or click",
                    egui::FontId::proportional(13.5),
                    pal.text_weak,
                );
                p.text(
                    r.center() + egui::vec2(0.0, 76.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{} saved patterns", self.patterns.len()),
                    egui::FontId::proportional(11.5),
                    pal.text_weak,
                );
                if resp.clicked() {
                    self.open_dialog(ctx);
                }
                if let Some(err) = &self.error {
                    ui.add_space(14.0);
                    ui.label(RichText::new(format!("⚠ {}", err)).color(pal.sev_err));
                }
            });
        });
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let all_cols = self.active_columns();
        let cols: Vec<Col> = all_cols.iter().copied().filter(|c| !self.empty_cols.contains(c)).collect();
        let empty_cols = &self.empty_cols;
        let mut col_change: Option<(Col, bool)> = None;
        let mut open_settings = false;
        if cols.is_empty() {
            return;
        }
        let row_h = 21.0;
        let mut actions: Vec<RowAction> = Vec::new();
        let mut sort_click: Option<Col> = None;
        let mut filter_changed = false;
        let mut visible: Option<(usize, usize)> = None;
        let q = self.search.trim().to_lowercase();
        let modifiers = ui.input(|i| i.modifiers);
        let n_sel = self.selected.len();

        let mut tb = TableBuilder::new(ui)
            .id_salt(format!("tbl{:?}", cols))
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .min_scrolled_height(0.0)
            .max_scroll_height(f32::INFINITY);
        for (i, c) in cols.iter().enumerate() {
            let col = if i == cols.len() - 1 {
                Column::remainder().at_least(80.0).clip(true)
            } else if *c == Col::Bookmark {
                Column::exact(c.width())
            } else {
                Column::initial(c.width()).at_least(36.0).clip(true)
            };
            tb = tb.column(col);
        }
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, Some(Align::Center));
        }

        let text_filters = &mut self.text_filters;
        let value_filters = &mut self.value_filters;
        let value_lists = &self.value_lists;
        let sort_col = self.sort_col;
        let sort_asc = self.sort_asc;
        let view = &self.view;
        let selected = &self.selected;
        let bookmarks = &self.bookmarks;
        let notes = &self.bookmark_notes;
        let code_syms = &self.code_syms;
        let hit_mask = &self.hit_mask;
        let marks = &self.pattern_marks;
        let focus = self.focus;
        let row_colors = &self.row_colors;

        tb.header(50.0, |mut header| {
            for &c in &cols {
                header.col(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        let arrow = if sort_col == c { if sort_asc { " ⏶" } else { " ⏷" } } else { "" };
                        let title = RichText::new(format!("{}{}", c.title(), arrow)).strong().size(12.5).color(if sort_col == c { pal.accent } else { pal.text });
                        let hr = ui.add(egui::Label::new(title).sense(Sense::click()).truncate().selectable(false)).on_hover_text("Click = sort · right-click = choose columns");
                        if hr.clicked() {
                            sort_click = Some(c);
                        }
                        hr.context_menu(|ui| {
                            ui.label(RichText::new("Columns shown").strong());
                            for cc in Col::ALL {
                                let mut on = all_cols.contains(&cc);
                                let name = if cc == Col::Bookmark { "🔖 Bookmark" } else { cc.title() };
                                let t = if empty_cols.contains(&cc) {
                                    RichText::new(format!("{name}   (empty in this log)")).color(pal.text_weak)
                                } else {
                                    RichText::new(name)
                                };
                                if ui.checkbox(&mut on, t).changed() {
                                    col_change = Some((cc, on));
                                }
                            }
                            ui.separator();
                            if ui.button("⚙ More in Settings…").clicked() {
                                open_settings = true;
                                ui.close_menu();
                            }
                        });
                        if c == Col::Bookmark {
                            return;
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let has_values = c.categorical();
                            let active_vals = value_filters.get(&c).map_or(0, |s| s.len());
                            let w = ui.available_width() - if has_values { 30.0 } else { 0.0 };
                            let txt = text_filters.entry(c).or_default();
                            let active = !txt.trim().is_empty();
                            let te = egui::TextEdit::singleline(txt)
                                .hint_text("filter…")
                                .desired_width(w.max(20.0))
                                .font(egui::TextStyle::Small)
                                .margin(egui::Margin::symmetric(4, 2))
                                .background_color(if active {
                                    Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 40)
                                } else {
                                    ui.visuals().extreme_bg_color
                                });
                            if ui.add(te).on_hover_text("Text filter:  a|b = or,  !x = not,  =x = exactly x").changed() {
                                filter_changed = true;
                            }
                            if has_values {
                                let lbl = if active_vals > 0 { RichText::new("⏷").color(pal.accent).strong() } else { RichText::new("⏷") };
                                ui.menu_button(lbl, |ui| {
                                    ui.set_min_width(260.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(c.title()).strong());
                                        if ui.small_button("All").clicked() {
                                            value_filters.remove(&c);
                                            filter_changed = true;
                                        }
                                    });
                                    ui.separator();
                                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                        if let Some(list) = value_lists.get(&c) {
                                            let set = value_filters.entry(c).or_default();
                                            for &(sym, cnt) in list {
                                                let mut on = set.contains(&sym);
                                                let name = log.pool.get(sym);
                                                let label = format!("{}   ({})", if name.is_empty() { "(empty)" } else { name }, fmt_num(cnt));
                                                if ui.checkbox(&mut on, label).changed() {
                                                    if on {
                                                        set.insert(sym);
                                                    } else {
                                                        set.remove(&sym);
                                                    }
                                                    filter_changed = true;
                                                }
                                            }
                                        }
                                    });
                                });
                            }
                        });
                    });
                });
            }
        })
        .body(|body| {
            body.rows(row_h, view.len(), |mut row| {
                let vi = row.index();
                let ei = view[vi];
                let e = &log.events[ei as usize];
                visible = Some(match visible {
                    None => (vi, vi),
                    Some((a, b)) => (a.min(vi), b.max(vi)),
                });
                let is_sel = selected.contains(&ei);
                row.set_selected(is_sel);
                let hit = hit_mask.get(ei as usize).copied().unwrap_or(false);
                let bg = if is_sel { Some(pal.row_sel) } else { row_colors.row_bg(e, &log.pool) };
                let hit_bg = !is_sel && hit;
                for (ci, &c) in cols.iter().enumerate() {
                    row.col(|ui| {
                        let r = ui.max_rect().expand2(egui::vec2(ui.spacing().item_spacing.x * 0.5 + 0.5, 0.0));
                        if let Some(bg) = bg {
                            ui.painter().rect_filled(r, 0.0, bg);
                        }
                        if hit_bg {
                            ui.painter().rect_filled(r, 0.0, pal.search_hit);
                        }
                        if ci == 0 {
                            if let Some(pc) = marks.get(&ei) {
                                let s = egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height()));
                                ui.painter().rect_filled(s, 0.0, rgb(*pc));
                            }
                            if Some(ei) == focus {
                                let s = egui::Rect::from_min_size(r.min + egui::vec2(3.0, 0.0), egui::vec2(2.0, r.height()));
                                ui.painter().rect_filled(s, 0.0, pal.accent);
                            }
                        }
                        match c {
                            Col::Bookmark => {
                                if bookmarks.contains(&ei) {
                                    let r = ui.add(egui::Label::new(RichText::new("🔖").color(pal.accent)).selectable(false));
                                    if let Some(n) = notes.get(&ei).filter(|n| !n.is_empty()) {
                                        r.on_hover_text(n.as_str());
                                    }
                                }
                            }
                            Col::Category => {
                                let col = pal.sev_color(e.severity);
                                let strong = e.severity != Severity::Info;
                                let mut t = RichText::new(format!("● {}", log.pool.get(e.category))).color(if strong { col } else { pal.text });
                                if strong {
                                    t = t.strong();
                                }
                                ui.add(egui::Label::new(t).truncate().selectable(false));
                            }
                            Col::Time => {
                                ui.add(egui::Label::new(RichText::new(fmt_ts(e.ts)).monospace()).truncate().selectable(false));
                            }
                            Col::Index => {
                                ui.add(egui::Label::new(RichText::new(e.index.to_string()).color(pal.text_weak)).truncate().selectable(false));
                            }
                            Col::Description | Col::Message => {
                                let s: &str = if c == Col::Description { log.pool.get(e.description) } else { &e.message };
                                let note = if c == Col::Description { code_syms.get(&e.event_id) } else { None };
                                let mut job = if hit && !q.is_empty() {
                                    highlight(ui, s, &q, &pal)
                                } else {
                                    let mut j = egui::text::LayoutJob::default();
                                    j.append(s, 0.0, egui::TextFormat { font_id: egui::TextStyle::Body.resolve(ui.style()), color: pal.text, ..Default::default() });
                                    j
                                };
                                if let Some(n) = note {
                                    job.append(&format!("  📘 {n}"), 0.0, egui::TextFormat { font_id: egui::TextStyle::Body.resolve(ui.style()), color: pal.accent, ..Default::default() });
                                }
                                ui.add(egui::Label::new(job).truncate().selectable(false));
                            }
                            c => {
                                let s = log.pool.get(c.sym(e).unwrap());
                                let note = if c == Col::EventId { code_syms.get(&e.event_id) } else { None };
                                let t = match note {
                                    Some(_) => RichText::new(format!("📘 {s}")).color(pal.accent),
                                    None => RichText::new(s).color(if c == Col::EventId { pal.text_weak } else { pal.text }),
                                };
                                let r = ui.add(egui::Label::new(t).truncate().selectable(false));
                                if let Some(n) = note {
                                    r.on_hover_text(n.as_str());
                                }
                            }
                        }
                    });
                }
                let resp = row.response();
                if resp.double_clicked() {
                    actions.push(RowAction::Double(vi));
                } else if resp.clicked() {
                    actions.push(RowAction::Click(vi, modifiers));
                }
                if resp.secondary_clicked() && !is_sel {
                    actions.push(RowAction::Click(vi, egui::Modifiers::NONE));
                }
                resp.context_menu(|ui| {
                    if ui.button(format!("➕ Create pattern from selection ({})", n_sel.max(1))).clicked() {
                        actions.push(RowAction::NewPattern);
                        ui.close_menu();
                    }
                    if ui.button("📋 Copy (Ctrl+C)").clicked() {
                        actions.push(RowAction::CopyRows);
                        ui.close_menu();
                    }
                    if ui.button(if bookmarks.contains(&ei) { "🔖 Remove bookmark" } else { "🔖 Bookmark (Ctrl+B)" }).clicked() {
                        actions.push(RowAction::Bookmark(vi));
                        ui.close_menu();
                    }
                    if ui.button(format!("📘 Describe code {}…", log.pool.get(e.event_id))).clicked() {
                        actions.push(RowAction::DescribeCode(vi));
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(RichText::new("Filter by value").small().color(pal.text_weak));
                    for fc in [Col::EventId, Col::Unit, Col::SystemMode, Col::InfoCategory] {
                        let v = log.pool.get(fc.sym(e).unwrap());
                        if ui.button(format!("{} = {}", fc.title(), v)).clicked() {
                            actions.push(RowAction::FilterBy(vi, fc));
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        self.visible_rows = visible;
        if filter_changed {
            self.dirty = true;
        }
        if let Some((c, on)) = col_change {
            let kind = self.log_kind();
            self.set_column(&kind, c, on);
        }
        if open_settings {
            self.settings_tab = 0;
            self.settings_kind = self.log_kind();
            self.show_settings = true;
        }
        if let Some(c) = sort_click {
            if self.sort_col == c {
                self.sort_asc = !self.sort_asc;
            } else {
                self.sort_col = c;
                self.sort_asc = true;
            }
            self.resort();
        }
        let ctx = ui.ctx().clone();
        for a in actions {
            match a {
                RowAction::Click(vi, m) => {
                    let ei = self.view[vi];
                    if m.shift {
                        let a = self.anchor.unwrap_or(vi);
                        if !m.command {
                            self.selected.clear();
                        }
                        for r in a.min(vi)..=a.max(vi) {
                            self.selected.insert(self.view[r]);
                        }
                    } else if m.command {
                        if !self.selected.insert(ei) {
                            self.selected.remove(&ei);
                        }
                        self.anchor = Some(vi);
                    } else {
                        self.selected.clear();
                        self.selected.insert(ei);
                        self.anchor = Some(vi);
                    }
                    self.focus = Some(ei);
                }
                RowAction::Double(vi) | RowAction::Bookmark(vi) => {
                    let ei = self.view[vi];
                    self.toggle_bookmark(ei);
                }
                RowAction::NewPattern => self.new_pattern_from_selection(&ctx),
                RowAction::DescribeCode(vi) => {
                    let code = self.log.as_ref().map(|l| l.pool.get(l.events[self.view[vi] as usize].event_id).to_owned()).unwrap_or_default();
                    if !code.trim().is_empty() {
                        self.open_code_editor(&code);
                    }
                }
                RowAction::CopyRows => self.copy_selected(&ctx),
                RowAction::FilterBy(vi, c) => {
                    let log = self.log.as_ref().unwrap();
                    let sym = c.sym(&log.events[self.view[vi] as usize]).unwrap();
                    let set = self.value_filters.entry(c).or_default();
                    set.clear();
                    set.insert(sym);
                    self.dirty = true;
                }
            }
        }
    }

    // ------------------------------------------------------------ patterns UI

    fn patterns_window(&mut self, ctx: &egui::Context) {
        if !self.show_patterns {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        let mut select: Option<Vec<u32>> = None;
        let mut edit: Option<u64> = None;
        let mut delete: Option<u64> = None;
        let mut rerun = false;
        egui::Window::new(RichText::new("🎯  Patterns").strong())
            .open(&mut open)
            .default_size([680.0, 540.0])
            .min_width(480.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let occ: usize = self.results.iter().map(|r| r.occurrences.len()).sum();
                    ui.selectable_value(&mut self.patterns_tab, 0, format!("Recognized in log ({} · {}×)", self.results.len(), occ));
                    ui.selectable_value(&mut self.patterns_tab, 1, format!("Library ({})", self.patterns.len()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("⟳ Re-run recognition").clicked() {
                            rerun = true;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.patterns_tab == 0 {
                        if self.log.is_none() {
                            ui.label("No log loaded.");
                        } else if self.results.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("No patterns recognized in this log.").color(pal.text_weak));
                                ui.label(RichText::new("Select rows in the table and click “➕ Pattern from selection”.").small().color(pal.text_weak));
                            });
                        }
                        let log = self.log.as_ref();
                        for r in &self.results {
                            let Some(p) = self.patterns.iter().find(|p| p.id == r.pattern_id) else { continue };
                            let Some(log) = log else { continue };
                            egui::Frame::new().fill(pal.panel2).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let (rr, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                                    ui.painter().rect_filled(rr, 3.0, rgb(p.color));
                                    ui.label(RichText::new(&p.name).strong().size(15.0));
                                    ui.label(RichText::new(format!("{}×", r.occurrences.len())).color(pal.pattern).strong());
                                    ui.label(RichText::new(format!("{} steps · duration ~{}", p.steps.len(), fmt_duration(p.span_ms()))).small().color(pal.text_weak));
                                });
                                if !p.description.is_empty() {
                                    ui.label(&p.description);
                                }
                                if !p.comment.is_empty() {
                                    ui.label(RichText::new(format!("💬 {}", p.comment)).italics().color(pal.text_weak));
                                }
                                ui.add_space(4.0);
                                for (k, o) in r.occurrences.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("#{}", k + 1)).color(pal.text_weak));
                                        ui.label(RichText::new(fmt_ts(o.start_ts)).monospace());
                                        let col = if o.similarity >= 95.0 {
                                            Color32::from_rgb(0x3d, 0xc4, 0x6e)
                                        } else if o.similarity >= 85.0 {
                                            pal.sev_warn
                                        } else {
                                            pal.pattern
                                        };
                                        ui.label(RichText::new(format!("{:.0}% similarity", o.similarity)).color(col).strong());
                                        ui.label(RichText::new(format!("{}/{} steps", o.found, p.steps.len())).small().color(pal.text_weak));
                                        let ev: Vec<u32> = o.events.iter().flatten().copied().collect();
                                        if let (Some(a), Some(b)) = (ev.iter().map(|&e| log.events[e as usize].ts).min(), ev.iter().map(|&e| log.events[e as usize].ts).max()) {
                                            ui.label(RichText::new(fmt_duration(b - a)).small().color(pal.text_weak));
                                        }
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.button("👁 Show").clicked() {
                                                select = Some(ev.clone());
                                            }
                                        });
                                    });
                                }
                            });
                            ui.add_space(6.0);
                        }
                    } else {
                        if self.patterns.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("The library is empty.").color(pal.text_weak));
                                ui.label(RichText::new("Select one or more rows in the table → right-click → “Create pattern”.").small().color(pal.text_weak));
                            });
                        }
                        for p in &self.patterns {
                            let occ = self.results.iter().find(|r| r.pattern_id == p.id).map_or(0, |r| r.occurrences.len());
                            egui::Frame::new().fill(pal.panel2).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let (rr, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                                    ui.painter().rect_filled(rr, 3.0, rgb(p.color));
                                    ui.label(RichText::new(&p.name).strong().size(15.0));
                                    if occ > 0 {
                                        ui.label(RichText::new(format!("recognized {}×", occ)).color(pal.pattern).strong());
                                    }
                                    if let Some((t, c)) = share_ui::share_label(p) {
                                        ui.label(RichText::new(t).small().color(match c {
                                            1 => pal.accent,
                                            2 => pal.sev_warn,
                                            _ => pal.text_weak,
                                        }));
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if self.confirm_delete == Some(p.id) {
                                            let txt = if p.shared && !p.foreign { "Confirm delete (also from the shared library)" } else { "Confirm delete" };
                                            if ui.button(RichText::new(txt).color(pal.sev_err)).clicked() {
                                                delete = Some(p.id);
                                            }
                                        } else if ui.button("🗑").on_hover_text("Delete").clicked() {
                                            self.confirm_delete = Some(p.id);
                                        }
                                        if ui.button("✏ Edit").clicked() {
                                            edit = Some(p.id);
                                        }
                                    });
                                });
                                if !p.description.is_empty() {
                                    ui.label(&p.description);
                                }
                                if !p.comment.is_empty() {
                                    ui.label(RichText::new(format!("💬 {}", p.comment)).italics().color(pal.text_weak));
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "{} steps · ~{} · tolerance ±{:.0}% (min {}) · threshold {:.0}% · {} · from: {} · {}",
                                        p.steps.len(),
                                        fmt_duration(p.span_ms()),
                                        p.time_tolerance_pct,
                                        fmt_duration(p.min_slack_ms),
                                        p.min_similarity,
                                        p.match_mode.label(),
                                        p.source_file,
                                        p.created
                                    ))
                                    .small()
                                    .color(pal.text_weak),
                                );
                                egui::CollapsingHeader::new(RichText::new("Steps").small()).id_salt(("steps", p.id)).show(ui, |ui| {
                                    steps_grid(ui, &p.steps, &pal, None);
                                });
                            });
                            ui.add_space(6.0);
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Patterns and settings are stored in: {}", config_path().display())).small().color(pal.text_weak));
                    }
                });
            });
        if !open {
            self.show_patterns = false;
        }
        if let Some(ev) = select {
            self.select_occurrence(ctx, &ev);
        }
        if let Some(id) = edit {
            if let Some(p) = self.patterns.iter().find(|p| p.id == id) {
                self.editor = Some(PatternEditor { p: p.clone(), is_new: false, error: None });
            }
        }
        if let Some(id) = delete {
            if let Some(p) = self.patterns.iter().find(|p| p.id == id).cloned() {
                self.withdraw_later(&p);
                self.schedule_share_sync(ctx.input(|i| i.time));
            }
            self.patterns.retain(|p| p.id != id);
            self.confirm_delete = None;
            if let Err(e) = self.save_cfg() {
                self.toast(ctx, format!("Error saving: {e}"));
            }
            rerun = true;
        }
        if rerun {
            self.run_patterns();
            let n = self.total_occurrences();
            self.toast(ctx, format!("Recognition finished: {} occurrences", n));
        }
    }

    fn pattern_editor(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(ed) = &mut self.editor else { return };
        let mut save = false;
        let mut cancel = false;
        let modal = egui::Modal::new(Id::new("pattern_editor")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.heading(if ed.is_new { "➕  New pattern" } else { "✏  Edit pattern" });
            ui.add_space(6.0);
            egui::Grid::new("pe").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Name *");
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut ed.p.name).hint_text("e.g. Collision sensor failure at startup").desired_width(480.0));
                    if ed.is_new && ed.p.name.is_empty() {
                        r.request_focus();
                    }
                    ui.color_edit_button_srgb(&mut ed.p.color);
                });
                ui.end_row();
                ui.label("Description");
                ui.add(egui::TextEdit::singleline(&mut ed.p.description).hint_text("What this pattern means").desired_width(520.0));
                ui.end_row();
                ui.label("Comment");
                ui.add(egui::TextEdit::multiline(&mut ed.p.comment).hint_text("Cause, solution, service notes…").desired_rows(3).desired_width(520.0));
                ui.end_row();
            });
            ui.add_space(6.0);
            egui::CollapsingHeader::new("⚙ Recognition settings").default_open(true).show(ui, |ui| {
                egui::Grid::new("pe2").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Row matching");
                    egui::ComboBox::from_id_salt("mm").selected_text(ed.p.match_mode.label()).show_ui(ui, |ui| {
                        for m in [MatchMode::IdAndDescription, MatchMode::IdOnly, MatchMode::DescriptionOnly] {
                            ui.selectable_value(&mut ed.p.match_mode, m, m.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Time tolerance");
                    ui.add(egui::Slider::new(&mut ed.p.time_tolerance_pct, 0.0..=300.0).suffix(" %").text("of the gap between rows"));
                    ui.end_row();
                    ui.label("Minimum window");
                    let mut secs = ed.p.min_slack_ms as f64 / 1000.0;
                    if ui.add(egui::DragValue::new(&mut secs).range(0.0..=3600.0).speed(0.1).suffix(" s")).changed() {
                        ed.p.min_slack_ms = (secs * 1000.0) as i64;
                    }
                    ui.end_row();
                    ui.label("Similarity threshold");
                    ui.add(egui::Slider::new(&mut ed.p.min_similarity, 40.0..=100.0).suffix(" %"));
                    ui.end_row();
                });
                ui.label(
                    RichText::new("Every step must match by key; gaps may deviate by the given tolerance and the order may differ slightly (this lowers the similarity). Numbers in descriptions are ignored.")
                        .small()
                        .color(pal.text_weak),
                );
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Steps ({})", ed.p.steps.len())).strong());
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                let mut remove = None;
                steps_grid(ui, &ed.p.steps, &pal, Some(&mut remove));
                if let Some(i) = remove {
                    ed.p.steps.remove(i);
                    if let Some(first) = ed.p.steps.first().map(|s| s.offset_ms) {
                        for s in &mut ed.p.steps {
                            s.offset_ms -= first;
                        }
                    }
                }
            });
            ui.add_space(6.0);
            if ed.p.foreign {
                ui.label(
                    RichText::new(format!(
                        "Shared by {}. Saving changes makes your own copy – the shared original stays unchanged.",
                        if ed.p.author.is_empty() { "a colleague" } else { &ed.p.author }
                    ))
                    .color(pal.sev_warn),
                );
            } else {
                ui.checkbox(&mut ed.p.shared, "Share with colleagues (shared pattern library)")
                    .on_hover_text("The pattern is sent to the shared library; later changes are sent automatically");
            }
            if let Some(err) = &ed.error {
                ui.label(RichText::new(err).color(pal.sev_err));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("💾  Save").strong().color(Color32::WHITE)).fill(pal.accent)).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if save {
            if ed.p.name.trim().is_empty() {
                ed.error = Some("Enter a pattern name.".into());
                return;
            }
            if ed.p.steps.is_empty() {
                ed.error = Some("A pattern needs at least one step.".into());
                return;
            }
            let mut p = ed.p.clone();
            let is_new = ed.is_new;
            self.editor = None;
            let old = self.patterns.iter().find(|x| x.id == p.id).cloned();
            let mut copied = false;
            if let Some(o) = old.as_ref().filter(|o| o.foreign) {
                if o.checksum() != p.checksum() {
                    // someone else's pattern: the change becomes an own copy, the original stays
                    if p.name.trim() == o.name.trim() {
                        p.name = format!("{} (my copy)", p.name.trim());
                    }
                    p.id = self.patterns.iter().map(|x| x.id + 1).max().unwrap_or(1);
                    p.uid = new_uid();
                    p.foreign = false;
                    p.shared = false;
                    p.author = self.author();
                    p.synced_checksum.clear();
                    p.remote_checksum.clear();
                    copied = true;
                } else {
                    p = o.clone();
                }
            }
            if let Some(o) = old.as_ref().filter(|o| !o.foreign && o.shared && !p.shared) {
                // unshared in the editor
                self.withdraw_later(o);
                p.synced_checksum.clear();
                p.remote_checksum.clear();
            }
            if p.shared {
                self.cfg.share_pending_withdraw.retain(|u| *u != p.uid);
            }
            let sync = p.shared || old.as_ref().is_some_and(|o| o.shared);
            if is_new || copied {
                self.patterns.push(p);
            } else if let Some(x) = self.patterns.iter_mut().find(|x| x.id == p.id) {
                *x = p;
            }
            if sync {
                self.schedule_share_sync(ctx.input(|i| i.time));
            }
            if copied {
                self.toast(ctx, "Saved as your own copy – the shared original stays unchanged");
            }
            if let Err(e) = self.save_cfg() {
                self.toast(ctx, format!("Error saving patterns: {e}"));
            }
            self.run_patterns();
            let n = self.total_occurrences();
            self.toast(ctx, format!("Pattern saved · {} occurrences recognized in this log", n));
        } else if cancel {
            self.editor = None;
        }
    }
}

fn is_problem(a: &App, ei: u32) -> bool {
    a.log.as_ref().is_some_and(|l| matches!(l.events[ei as usize].severity, Severity::Warning | Severity::Error | Severity::Fatal))
}

fn steps_grid(ui: &mut egui::Ui, steps: &[PatternStep], pal: &Palette, mut remove: Option<&mut Option<usize>>) {
    egui::Grid::new(ui.next_auto_id()).striped(true).num_columns(5).spacing([10.0, 3.0]).show(ui, |ui| {
        ui.label(RichText::new("#").small().color(pal.text_weak));
        ui.label(RichText::new("Δt").small().color(pal.text_weak));
        ui.label(RichText::new("Event ID").small().color(pal.text_weak));
        ui.label(RichText::new("Description").small().color(pal.text_weak));
        ui.label("");
        ui.end_row();
        let mut prev = 0;
        for (i, s) in steps.iter().enumerate() {
            ui.label(RichText::new(format!("{}", i + 1)).color(pal.text_weak));
            let d = if i == 0 { "start".to_owned() } else { format!("+{}", fmt_duration(s.offset_ms - prev)) };
            prev = s.offset_ms;
            ui.label(RichText::new(d).monospace().small());
            ui.label(RichText::new(&s.event_id).monospace().small().color(pal.text_weak));
            let sev = Severity::parse(&s.category);
            let mut t = RichText::new(&s.description);
            if sev != Severity::Info {
                t = t.color(pal.sev_color(sev));
            }
            ui.label(t);
            if let Some(rm) = remove.as_deref_mut() {
                if steps.len() > 1 && ui.small_button("×").on_hover_text("Remove step").clicked() {
                    *rm = Some(i);
                }
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
}

fn highlight(ui: &egui::Ui, s: &str, q: &str, pal: &Palette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let normal = egui::TextFormat { font_id: font.clone(), color: pal.text, ..Default::default() };
    let hl = egui::TextFormat {
        font_id: font,
        color: pal.text,
        background: Color32::from_rgb(0xff, 0xd3, 0x4d),
        ..Default::default()
    };
    let lower = s.to_lowercase();
    if lower.len() != s.len() {
        job.append(s, 0.0, normal);
        return job;
    }
    let mut i = 0;
    while let Some(p) = lower[i..].find(q) {
        let a = i + p;
        let b = a + q.len();
        if a > i {
            job.append(&s[i..a], 0.0, normal.clone());
        }
        job.append(&s[a..b], 0.0, hl.clone());
        i = b;
    }
    if i < s.len() {
        job.append(&s[i..], 0.0, normal);
    }
    job
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.log_save_due.is_some() || self.log_changed {
            self.save_log_file();
        }
        if self.cfg_save_due.is_some() {
            self.save_settings();
        }
        self.update_on_exit();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // finished loading?
        if let Some(l) = &self.loading {
            match l.rx.try_recv() {
                Ok(Ok(log)) => {
                    self.loading = None;
                    self.on_loaded(log);
                    let n = self.total_occurrences();
                    if n > 0 {
                        let k = self.results.len();
                        self.toast(ctx, format!("🎯 Recognized {} patterns ({} occurrences)", k, n));
                    }
                }
                Ok(Err(e)) => {
                    self.loading = None;
                    self.error = Some(e);
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(40)),
            }
        }
        // drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.open(ctx, p);
        }

        self.handle_keys(ctx);
        if self.dirty {
            self.rebuild();
        }

        self.top_bar(ctx);
        if self.log.is_some() {
            self.filter_bar(ctx);
            self.timeline_bar(ctx);
        }
        self.status_bar(ctx);
        if self.log.is_some() {
            self.details_panel(ctx);
        }
        let pal = self.pal;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 0 }))
            .show(ctx, |ui| {
                if let Some(l) = &self.loading {
                    let f = l.progress.fraction();
                    let name = l.name.clone();
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.35);
                        ui.label(RichText::new(format!("Loading {}", name)).size(16.0));
                        ui.add_space(8.0);
                        ui.add(egui::ProgressBar::new(f).desired_width(420.0).show_percentage().animate(true));
                    });
                } else if self.log.is_none() {
                    self.empty_state(ui, ctx);
                } else {
                    self.table(ui);
                }
            });

        self.patterns_window(ctx);
        self.bookmarks_window(ctx);
        self.poll_viewer(ctx);
        self.files_window(ctx);
        self.viewer_window(ctx);
        self.sysinfo_window(ctx);
        self.about_window(ctx);
        self.codes_window(ctx);
        self.code_editor(ctx);
        self.charts_window(ctx);
        self.settings_window(ctx);
        self.merge_dialog(ctx);
        self.poll_update(ctx);
        self.update_dialog(ctx);
        self.poll_share(ctx);
        self.share_windows(ctx);
        if let Some(w) = self.cfg_warning.clone() {
            let pal = self.pal;
            let mut close = false;
            egui::Modal::new(Id::new("cfg_warning")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading("⚠  Settings file");
                ui.add_space(4.0);
                ui.label(RichText::new(w).color(pal.text));
                ui.add_space(8.0);
                if ui.button("OK").clicked() {
                    close = true;
                }
            });
            if close {
                self.cfg_warning = None;
            }
        }
        if std::mem::take(&mut self.pending_new_pattern) {
            self.new_pattern_from_selection(ctx);
        }

        // per-log file: save shortly after the last change
        let now = ctx.input(|i| i.time);
        if self.log_changed {
            self.log_changed = false;
            if self.log.is_some() {
                self.log_save_due = Some(now + 1.0);
            }
        }
        if let Some(due) = self.cfg_save_due {
            if now >= due {
                self.cfg_save_due = None;
                self.save_settings();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now + 0.05));
            }
        }
        if let Some(due) = self.log_save_due {
            if now >= due {
                self.save_log_file();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now + 0.05));
            }
        }
        self.pattern_editor(ctx);

        // toast
        if let Some((msg, until)) = &self.toast {
            let now = ctx.input(|i| i.time);
            if now > *until {
                self.toast = None;
            } else {
                let msg = msg.clone();
                egui::Area::new(Id::new("toast"))
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(0x22, 0x28, 0x33))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(16, 10))
                            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(Color32::WHITE));
                            });
                    });
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }
}
