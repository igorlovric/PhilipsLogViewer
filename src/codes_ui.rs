//! Error code dictionary: the user writes what a code / Event ID means once,
//! it is stored in the global config and shown for every log. Child module of `app`.

use super::*;

pub struct CodeEdit {
    pub code: String,
    pub description: String,
    pub comment: String,
    pub is_new: bool,
}

impl App {
    /// event_id symbol -> description, for the current log (fast lookups in table / search)
    pub(super) fn refresh_code_syms(&mut self) {
        self.code_syms.clear();
        let Some(log) = &self.log else { return };
        for (code, note) in &self.cfg.codes {
            if let Some(s) = log.pool.find(code) {
                self.code_syms.insert(s, note.description.clone());
            }
        }
    }

    pub(super) fn open_code_editor(&mut self, code: &str) {
        let note = self.cfg.codes.get(code).cloned();
        self.code_edit = Some(CodeEdit {
            code: code.to_owned(),
            description: note.as_ref().map(|n| n.description.clone()).unwrap_or_default(),
            comment: note.as_ref().map(|n| n.comment.clone()).unwrap_or_default(),
            is_new: note.is_none(),
        });
    }

    /// The "📘 code: description" box in the details panel. Returns a code to edit.
    pub(super) fn code_note_ui(codes: &std::collections::BTreeMap<String, CodeNote>, pal: Palette, ui: &mut egui::Ui, e: &Event, log: &LogData) -> Option<String> {
        let code = log.pool.get(e.event_id);
        if code.trim().is_empty() {
            return None;
        }
        let mut edit = None;
        if let Some(n) = codes.get(code) {
            egui::Frame::new().fill(Color32::from_rgb(0xea, 0xf2, 0xff)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("📘 {code}")).strong().color(pal.accent));
                    ui.add(egui::Label::new(RichText::new(&n.description).strong()).wrap().selectable(true));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("✏").on_hover_text("Edit the description of this code").clicked() {
                            edit = Some(code.to_owned());
                        }
                    });
                });
                if !n.comment.trim().is_empty() {
                    ui.add(egui::Label::new(RichText::new(&n.comment).color(pal.text_weak)).wrap().selectable(true));
                }
            });
        } else if matches!(e.severity, Severity::Warning | Severity::Error | Severity::Fatal) {
            if ui.small_button(format!("📘 Describe code {code}…")).on_hover_text("Write once what this code means – it is shown for every log").clicked() {
                edit = Some(code.to_owned());
            }
        }
        edit
    }

    pub(super) fn code_editor(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(ed) = &mut self.code_edit else { return };
        let mut save = false;
        let mut cancel = false;
        let mut delete = false;
        let m = egui::Modal::new(Id::new("code_editor")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading(format!("📘  {}", ed.code));
            ui.label(RichText::new("Stored in the global config, shown for this code in every log").small().color(pal.text_weak));
            ui.add_space(6.0);
            egui::Grid::new("code_ed").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Meaning *");
                let r = ui.add(egui::TextEdit::singleline(&mut ed.description).hint_text("e.g. Reconstruction server not reachable").desired_width(440.0));
                if ed.is_new && ed.description.is_empty() {
                    r.request_focus();
                }
                ui.end_row();
                ui.label("Comment");
                ui.add(egui::TextEdit::multiline(&mut ed.comment).hint_text("Cause, what to check, solution…").desired_rows(4).desired_width(440.0));
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!ed.description.trim().is_empty(), egui::Button::new(RichText::new("💾  Save").strong().color(Color32::WHITE)).fill(pal.accent)).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if !ed.is_new {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new("🗑 Delete").color(pal.sev_err)).clicked() {
                            delete = true;
                        }
                    });
                }
            });
        });
        if m.should_close() {
            cancel = true;
        }
        if save {
            let note = CodeNote { description: ed.description.trim().to_owned(), comment: ed.comment.trim().to_owned() };
            let code = ed.code.clone();
            self.cfg.codes.insert(code, note);
            self.code_edit = None;
            self.after_codes_changed(ctx);
        } else if delete {
            let code = ed.code.clone();
            self.cfg.codes.remove(&code);
            self.code_edit = None;
            self.after_codes_changed(ctx);
        } else if cancel {
            self.code_edit = None;
        }
    }

    fn after_codes_changed(&mut self, ctx: &egui::Context) {
        if let Err(e) = self.save_cfg() {
            self.toast(ctx, format!("Cannot save the config: {e}"));
        }
        self.refresh_code_syms();
        self.dirty = true;
    }

    pub(super) fn codes_window(&mut self, ctx: &egui::Context) {
        if !self.show_codes {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        let mut edit: Option<String> = None;
        let mut show: Option<String> = None;
        let q = self.codes_filter.trim().to_lowercase();
        // counts of codes in the current log
        let mut in_log: HashMap<String, (usize, Severity, String)> = HashMap::new();
        if let Some(log) = &self.log {
            for e in &log.events {
                let c = log.pool.get(e.event_id);
                if c.is_empty() {
                    continue;
                }
                let ent = in_log.entry(c.to_owned()).or_insert((0, e.severity, log.pool.get(e.description).to_owned()));
                ent.0 += 1;
                // keep the "worst" severity (Info < Warning < Error < Fatal)
                if e.severity > ent.1 && e.severity != Severity::Other {
                    ent.1 = e.severity;
                }
            }
        }
        egui::Window::new(RichText::new("📘  Code dictionary").strong())
            .open(&mut open)
            .default_size([820.0, 560.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.codes_filter).hint_text("🔍  filter codes / descriptions…").desired_width(280.0));
                    ui.label(RichText::new(format!("{} described codes · stored in {}", self.cfg.codes.len(), config_path().display())).small().color(pal.text_weak));
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    // codes of the current log with warnings / errors but no description yet
                    let mut missing: Vec<(&String, &(usize, Severity, String))> = in_log
                        .iter()
                        .filter(|(c, v)| matches!(v.1, Severity::Warning | Severity::Error | Severity::Fatal) && !self.cfg.codes.contains_key(*c))
                        .filter(|(c, v)| q.is_empty() || c.to_lowercase().contains(&q) || v.2.to_lowercase().contains(&q))
                        .collect();
                    missing.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .0.cmp(&a.1 .0)));
                    if !missing.is_empty() {
                        ui.label(RichText::new(format!("Error / warning codes in this log without a description ({})", missing.len())).strong());
                        egui::Grid::new("codes_missing").striped(true).num_columns(4).spacing([12.0, 3.0]).show(ui, |ui| {
                            for (c, (n, sev, desc)) in missing {
                                ui.label(RichText::new(c.as_str()).monospace().color(pal.sev_color(*sev)));
                                ui.label(RichText::new(format!("{n}×")).color(pal.text_weak));
                                ui.add(egui::Label::new(desc.as_str()).truncate());
                                ui.horizontal(|ui| {
                                    if ui.small_button("📘 Describe").clicked() {
                                        edit = Some(c.clone());
                                    }
                                    if ui.small_button("👁 Show").on_hover_text("Show only this code in the table").clicked() {
                                        show = Some(c.clone());
                                    }
                                });
                                ui.end_row();
                            }
                        });
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("Described codes").strong());
                    if self.cfg.codes.is_empty() {
                        ui.label(RichText::new("None yet. Click a row with an error and then “📘 Describe code…” in the details panel.").color(pal.text_weak));
                    }
                    egui::Grid::new("codes_all").striped(true).num_columns(4).spacing([12.0, 3.0]).show(ui, |ui| {
                        for (c, note) in &self.cfg.codes {
                            if !q.is_empty() && !c.to_lowercase().contains(&q) && !note.description.to_lowercase().contains(&q) && !note.comment.to_lowercase().contains(&q) {
                                continue;
                            }
                            ui.label(RichText::new(c.as_str()).monospace().strong());
                            let n = in_log.get(c).map(|v| v.0).unwrap_or(0);
                            ui.label(RichText::new(if n > 0 { format!("{n}× in this log") } else { "–".into() }).small().color(pal.text_weak));
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(note.description.as_str()).wrap());
                                if !note.comment.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(note.comment.as_str()).small().color(pal.text_weak)).wrap());
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("✏").on_hover_text("Edit").clicked() {
                                    edit = Some(c.clone());
                                }
                                if n > 0 && ui.small_button("👁").on_hover_text("Show only this code in the table").clicked() {
                                    show = Some(c.clone());
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
            });
        if !open {
            self.show_codes = false;
        }
        if let Some(c) = edit {
            self.open_code_editor(&c);
        }
        if let Some(c) = show {
            if let Some(log) = &self.log {
                if let Some(sym) = log.pool.find(&c) {
                    let set = self.value_filters.entry(Col::EventId).or_default();
                    set.clear();
                    set.insert(sym);
                    self.dirty = true;
                }
            }
        }
    }
}
