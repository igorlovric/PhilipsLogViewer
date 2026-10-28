//! Main menu (☰), Settings (columns per kind of log) and merging a colleague's patterns.
//! Child module of `app`.

use super::*;

/// key for the default column set (all kinds of logs without their own set)
pub const DEFAULT_KIND: &str = "";

pub enum MergeChoice {
    Add,
    Skip,
    /// add with a new id and " (imported)" appended to the name
    KeepBoth,
    /// overwrite the local pattern with the same id / name
    Replace(u64),
}

pub enum MergeStatus {
    New,
    /// same steps as a local pattern (name of it)
    Same(String),
    /// same id or name as a local pattern, different content
    Conflict(u64, String),
}

pub struct MergeItem {
    pub p: Pattern,
    pub status: MergeStatus,
    pub choice: MergeChoice,
}

pub struct MergeState {
    pub file: PathBuf,
    pub items: Vec<MergeItem>,
}

fn steps_key(p: &Pattern) -> String {
    let mut k = format!("{:?}|", p.match_mode);
    for s in &p.steps {
        k.push_str(&format!("{}|{}|{};", s.event_id, crate::model::normalize_desc(&s.description), s.offset_ms));
    }
    k
}

/// Compares a colleague's patterns with the local ones and proposes what to do with each.
pub fn classify(local: &[Pattern], theirs: Vec<Pattern>) -> Vec<MergeItem> {
    let mut items = vec![];
    for p in theirs {
        let key = steps_key(&p);
        let status = if let Some(same) = local.iter().find(|m| steps_key(m) == key) {
            MergeStatus::Same(same.name.clone())
        } else if let Some(m) = local.iter().find(|m| m.id == p.id || m.name.trim().eq_ignore_ascii_case(p.name.trim())) {
            MergeStatus::Conflict(m.id, m.name.clone())
        } else {
            MergeStatus::New
        };
        let choice = match &status {
            MergeStatus::New => MergeChoice::Add,
            MergeStatus::Same(_) => MergeChoice::Skip,
            MergeStatus::Conflict(..) => MergeChoice::KeepBoth,
        };
        items.push(MergeItem { p, status, choice });
    }
    items
}

impl App {
    /// kind of the current log ("" when nothing is loaded)
    pub(super) fn log_kind(&self) -> String {
        self.log.as_ref().map(|l| l.kind.clone()).unwrap_or_default()
    }

    /// columns chosen for this kind of log (falls back to the default set)
    pub(super) fn columns_for(&self, kind: &str) -> Vec<Col> {
        self.cfg.column_sets.get(kind).cloned().unwrap_or_else(|| self.cfg.visible_columns.clone())
    }

    pub(super) fn active_columns(&self) -> Vec<Col> {
        self.columns_for(&self.log_kind())
    }

    pub(super) fn set_column(&mut self, kind: &str, c: Col, on: bool) {
        let mut cols = self.columns_for(kind);
        if on {
            if !cols.contains(&c) {
                cols.push(c);
            }
        } else {
            cols.retain(|x| *x != c);
        }
        let cols: Vec<Col> = Col::ALL.iter().copied().filter(|x| cols.contains(x)).collect();
        if kind == DEFAULT_KIND {
            self.cfg.visible_columns = cols;
        } else {
            self.cfg.column_sets.insert(kind.to_owned(), cols);
        }
        self.save_settings();
    }

    // ------------------------------------------------------------ main menu

    pub(super) fn main_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has_log = self.log.is_some();
        let archive = self.log.as_ref().map_or(0, |l| l.archive.len());
        let sysinfo = self.log.as_ref().is_some_and(|l| l.sysinfo.is_some());
        ui.menu_button(RichText::new("☰").size(18.0), |ui| {
            ui.set_min_width(250.0);
            let item = |ui: &mut egui::Ui, enabled: bool, text: String, hint: &str| -> bool {
                ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(hint)).clicked()
            };
            if item(ui, true, "📂  Open…".into(), "Ctrl+O") {
                ui.close_menu();
                self.open_dialog(ctx);
            }
            ui.separator();
            if item(ui, archive > 0, format!("📁  Files in ZIP ({archive})"), "") {
                ui.close_menu();
                self.show_files = true;
            }
            if item(ui, sysinfo, "ℹ  System".into(), "") {
                ui.close_menu();
                self.show_sysinfo = true;
            }
            if item(ui, has_log, "📈  Charts".into(), "") {
                ui.close_menu();
                self.show_charts = true;
            }
            if item(ui, true, format!("📘  Codes ({})", self.cfg.codes.len()), "") {
                ui.close_menu();
                self.show_codes = true;
            }
            ui.separator();
            if item(ui, true, "⚙  Settings…".into(), "") {
                ui.close_menu();
                self.settings_kind = self.log_kind();
                if !self.cfg.column_sets.contains_key(&self.settings_kind) && self.settings_kind.is_empty() {
                    self.settings_kind = DEFAULT_KIND.to_owned();
                }
                self.show_settings = true;
            }
            if item(ui, true, "🎯  Merge patterns…".into(), "") {
                ui.close_menu();
                self.start_merge(ctx);
            }
            ui.separator();
            if item(ui, true, "⏶  Check for updates…".into(), "") {
                ui.close_menu();
                self.start_update_check(true);
            }
            if item(ui, true, "About · What's new".into(), "") {
                ui.close_menu();
                self.show_about = true;
            }
        })
        .response
        .on_hover_text("Menu");
    }

    // ------------------------------------------------------------ settings

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        let current_kind = self.log_kind();
        let mut kinds: Vec<String> = self.cfg.column_sets.keys().cloned().collect();
        if !current_kind.is_empty() && !kinds.contains(&current_kind) {
            kinds.push(current_kind.clone());
        }
        kinds.sort();
        let label = |k: &str| if k.is_empty() { "Default (all other logs)".to_owned() } else { k.to_owned() };
        egui::Window::new(RichText::new("⚙  Settings").strong())
            .open(&mut open)
            .default_size([520.0, 560.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (t, name) in [(0u8, "Table columns"), (1u8, "Colors"), (2u8, "Updates")] {
                        let sel = self.settings_tab == t;
                        let txt = RichText::new(name).size(14.0).color(if sel { pal.accent } else { pal.text });
                        if ui.selectable_label(sel, if sel { txt.strong() } else { txt }).clicked() {
                            self.settings_tab = t;
                        }
                    }
                });
                ui.separator();
                if self.settings_tab == 2 {
                    self.updates_tab(ui);
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Settings are stored in {}", config_path().display())).small().color(pal.text_weak));
                    return;
                }
                if self.settings_tab == 1 {
                    self.colors_tab(ui);
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Settings are stored in {}", config_path().display())).small().color(pal.text_weak));
                    return;
                }
                    ui.label(
                        RichText::new("Each kind of log can have its own columns (e.g. Azurion, CombiDiagnost, CT database, trace files). Kinds without their own choice use the default set. Right-click the table header to change the columns of the current log quickly.")
                            .small()
                            .color(pal.text_weak),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Kind of log");
                        let mut sel = self.settings_kind.clone();
                        egui::ComboBox::from_id_salt("kind_sel").width(280.0).selected_text(label(&sel)).show_ui(ui, |ui| {
                            ui.selectable_value(&mut sel, DEFAULT_KIND.to_owned(), label(DEFAULT_KIND));
                            for k in &kinds {
                                let mut t = label(k);
                                if *k == current_kind {
                                    t.push_str("   (current log)");
                                }
                                ui.selectable_value(&mut sel, k.clone(), t);
                            }
                        });
                        self.settings_kind = sel;
                    });
                    let kind = self.settings_kind.clone();
                    if !kind.is_empty() {
                        ui.horizontal(|ui| {
                            if self.cfg.column_sets.contains_key(&kind) {
                                ui.label(RichText::new("Own column choice for this kind of log").small().color(pal.accent));
                                if ui.small_button("Use the default columns").clicked() {
                                    self.cfg.column_sets.remove(&kind);
                                    self.save_settings();
                                }
                            } else {
                                ui.label(RichText::new("Uses the default columns – a change below creates its own choice").small().color(pal.text_weak));
                            }
                        });
                    }
                    ui.separator();
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        egui::Grid::new("set_cols").num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
                            // two columns of checkboxes
                            let cols = self.columns_for(&kind);
                            let is_current = kind == current_kind || (kind.is_empty() && !self.cfg.column_sets.contains_key(&current_kind));
                            let mut change: Option<(Col, bool)> = None;
                            let half = Col::ALL.len().div_ceil(2);
                            for row in 0..half {
                                for c in [Col::ALL.get(row), Col::ALL.get(row + half)].into_iter().flatten() {
                                    let mut on = cols.contains(c);
                                    let name = if *c == Col::Bookmark { "🔖 Bookmark" } else { c.title() };
                                    let mut t = RichText::new(name);
                                    if is_current && self.empty_cols.contains(c) {
                                        t = RichText::new(format!("{name}   (empty in this log)")).color(pal.text_weak);
                                    }
                                    if ui.checkbox(&mut on, t).changed() {
                                        change = Some((*c, on));
                                    }
                                }
                                ui.end_row();
                            }
                            if let Some((c, on)) = change {
                                self.set_column(&kind, c, on);
                            }
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Settings are stored in {}", config_path().display())).small().color(pal.text_weak));
                    });
            });
        if !open {
            self.show_settings = false;
        }
    }

    // ------------------------------------------------------------ merge patterns

    fn start_merge(&mut self, ctx: &egui::Context) {
        let mut d = rfd::FileDialog::new().add_filter("Philips Log Viewer config / patterns", &["config", "json"]).add_filter("All files", &["*"]);
        if let Some(dir) = &self.cfg.last_dir {
            d = d.set_directory(dir);
        }
        // debug builds: PLV_MERGE_FILE replaces the file dialog (automated tests)
        #[cfg(debug_assertions)]
        let test_file = std::env::var("PLV_MERGE_FILE").ok().map(PathBuf::from);
        #[cfg(not(debug_assertions))]
        let test_file: Option<PathBuf> = None;
        let Some(file) = test_file.or_else(|| d.pick_file()) else { return };
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                self.toast(ctx, format!("Cannot read {}: {e}", file.display()));
                return;
            }
        };
        // a whole config file, or just a list of patterns
        let theirs: Vec<Pattern> = match serde_json::from_str::<GlobalConfig>(&text) {
            Ok(c) if !c.patterns.is_empty() => c.patterns,
            _ => match serde_json::from_str::<Vec<Pattern>>(&text) {
                Ok(v) => v,
                Err(_) => {
                    self.toast(ctx, "This file contains no patterns of Philips Log Viewer");
                    return;
                }
            },
        };
        if theirs.is_empty() {
            self.toast(ctx, "This file contains no patterns");
            return;
        }
        let items = classify(&self.patterns, theirs);
        self.merge = Some(MergeState { file, items });
    }

    pub(super) fn merge_dialog(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(m) = &mut self.merge else { return };
        let mut apply = false;
        let mut cancel = false;
        let modal = egui::Modal::new(Id::new("merge_patterns")).show(ctx, |ui| {
            ui.set_width(720.0);
            ui.heading("🎯  Merge patterns");
            ui.label(RichText::new(format!("From {}", m.file.display())).small().color(pal.text_weak));
            let new = m.items.iter().filter(|i| matches!(i.status, MergeStatus::New)).count();
            let same = m.items.iter().filter(|i| matches!(i.status, MergeStatus::Same(_))).count();
            let conf = m.items.len() - new - same;
            ui.label(format!("{} patterns in the file: {} new · {} already in your library · {} with the same name but different steps", m.items.len(), new, same, conf));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                egui::Grid::new("merge_grid").striped(true).num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (k, it) in m.items.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let (rr, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                            ui.painter().rect_filled(rr, 2.0, rgb(it.p.color));
                            ui.label(RichText::new(&it.p.name).strong());
                        })
                        .response
                        .on_hover_text(format!("{} steps{}", it.p.steps.len(), if it.p.description.is_empty() { String::new() } else { format!(" · {}", it.p.description) }));
                        match &it.status {
                            MergeStatus::New => {
                                ui.label(RichText::new("new").color(Color32::from_rgb(0x2a, 0xa1, 0x5a)));
                            }
                            MergeStatus::Same(n) => {
                                ui.label(RichText::new(format!("already have it as “{n}”")).color(pal.text_weak));
                            }
                            MergeStatus::Conflict(_, n) => {
                                ui.label(RichText::new(format!("differs from your “{n}”")).color(pal.sev_warn));
                            }
                        }
                        let conflict_id = if let MergeStatus::Conflict(id, _) = &it.status { Some(*id) } else { None };
                        let cur = match it.choice {
                            MergeChoice::Add => "Add",
                            MergeChoice::Skip => "Skip",
                            MergeChoice::KeepBoth => "Keep both",
                            MergeChoice::Replace(_) => "Replace mine",
                        };
                        egui::ComboBox::from_id_salt(("mc", k)).selected_text(cur).width(130.0).show_ui(ui, |ui| {
                            match conflict_id {
                                Some(id) => {
                                    if ui.selectable_label(matches!(it.choice, MergeChoice::KeepBoth), "Keep both").clicked() {
                                        it.choice = MergeChoice::KeepBoth;
                                    }
                                    if ui.selectable_label(matches!(it.choice, MergeChoice::Replace(_)), "Replace mine").clicked() {
                                        it.choice = MergeChoice::Replace(id);
                                    }
                                }
                                None => {
                                    if ui.selectable_label(matches!(it.choice, MergeChoice::Add), "Add").clicked() {
                                        it.choice = MergeChoice::Add;
                                    }
                                }
                            }
                            if ui.selectable_label(matches!(it.choice, MergeChoice::Skip), "Skip").clicked() {
                                it.choice = MergeChoice::Skip;
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Your own patterns are never removed. “Keep both” adds the colleague's version with “(imported)” in the name.").small().color(pal.text_weak));
            ui.horizontal(|ui| {
                let n = m.items.iter().filter(|i| !matches!(i.choice, MergeChoice::Skip)).count();
                if ui.add_enabled(n > 0, egui::Button::new(RichText::new(format!("Merge {n} patterns")).strong().color(Color32::WHITE)).fill(pal.accent)).clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if apply {
            let m = self.merge.take().unwrap();
            let mut next_id = self.patterns.iter().map(|p| p.id).max().unwrap_or(0).max(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            ) + 1;
            let mut added = 0;
            let mut replaced = 0;
            for it in m.items {
                let mut p = it.p;
                match it.choice {
                    MergeChoice::Skip => continue,
                    MergeChoice::Replace(id) => {
                        if let Some(x) = self.patterns.iter_mut().find(|x| x.id == id) {
                            p.id = id;
                            *x = p;
                            replaced += 1;
                        }
                        continue;
                    }
                    MergeChoice::KeepBoth => {
                        p.name = format!("{} (imported)", p.name.trim());
                    }
                    MergeChoice::Add => {}
                }
                if self.patterns.iter().any(|x| x.id == p.id) {
                    p.id = next_id;
                    next_id += 1;
                }
                self.patterns.push(p);
                added += 1;
            }
            if let Err(e) = self.save_cfg() {
                self.toast(ctx, format!("Cannot save the config: {e}"));
            }
            self.run_patterns();
            let occ = self.total_occurrences();
            self.toast(ctx, format!("Merged: {added} added, {replaced} replaced · {occ} occurrences in this log"));
        } else if cancel {
            self.merge = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::PatternStep;

    fn pat(id: u64, name: &str, ev: &str) -> Pattern {
        let mut p = Pattern::defaults_from(
            vec![PatternStep { event_id: ev.into(), description: "X failed".into(), category: "Error".into(), unit: String::new(), offset_ms: 0 }],
            String::new(),
            String::new(),
            id,
        );
        p.name = name.into();
        p
    }

    #[test]
    fn merge_classification() {
        let local = vec![pat(1, "Collision", "A"), pat(2, "Tube", "B")];
        let theirs = vec![pat(10, "Same steps other name", "A"), pat(2, "Tube", "C"), pat(11, "tube", "D"), pat(12, "New one", "E")];
        let r = classify(&local, theirs);
        assert!(matches!(r[0].status, MergeStatus::Same(ref n) if n == "Collision") && matches!(r[0].choice, MergeChoice::Skip));
        assert!(matches!(r[1].status, MergeStatus::Conflict(2, _)) && matches!(r[1].choice, MergeChoice::KeepBoth));
        assert!(matches!(r[2].status, MergeStatus::Conflict(2, _)));
        assert!(matches!(r[3].status, MergeStatus::New) && matches!(r[3].choice, MergeChoice::Add));
        // a whole config file parses too
        let cfg = serde_json::to_string(&GlobalConfig { patterns: vec![pat(5, "P", "Z")], ..Default::default() }).unwrap();
        assert_eq!(serde_json::from_str::<GlobalConfig>(&cfg).unwrap().patterns.len(), 1);
    }
}
