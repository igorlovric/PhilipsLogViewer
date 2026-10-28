//! Settings → Colors: row background per event category and the selected-row color.
//! Child module of `app`.

use super::*;
use crate::theme::{fallback_key, RowColors};

fn to_rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// default color of a category name (case-insensitive), if it is one of the built-in ones
fn default_color(name: &str) -> Option<Option<[u8; 3]>> {
    default_category_colors().into_iter().find(|d| d.name.eq_ignore_ascii_case(name.trim())).map(|d| d.color)
}

impl App {
    /// rebuilds the lookup used by the table after the color list changed
    pub(super) fn apply_colors(&mut self) {
        self.row_colors = RowColors::new(&self.cfg.category_colors);
        self.pal.set_row_colors(&self.row_colors, self.cfg.selected_row_color);
    }

    fn colors_changed(&mut self, now: f64) {
        self.apply_colors();
        self.cfg_save_due = Some(now + 0.8);
    }

    /// counts the categories of the loaded log and adds the ones the color list does not have yet;
    /// a new category starts with the color of the default it belongs to (e.g. "Severe" → Error red)
    pub(super) fn discover_categories(&mut self) {
        self.cat_counts.clear();
        let Some(log) = &self.log else { return };
        // (0, category) for normal rows, (1, 0) UserMessage, (2, 0) Command
        let mut m: HashMap<(u8, Sym), (usize, RowKind)> = HashMap::new();
        for e in &log.events {
            let k = match e.kind {
                RowKind::UserMessage => (1, 0),
                RowKind::Command => (2, 0),
                _ => (0, e.category),
            };
            let v = m.entry(k).or_insert((0, e.kind));
            v.0 += 1;
        }
        let mut found: Vec<(String, usize, RowKind)> = m
            .into_iter()
            .map(|((t, s), (n, kind))| {
                let name = match t {
                    1 => "UserMessage".to_owned(),
                    2 => "Command".to_owned(),
                    _ => log.pool.get(s).trim().to_owned(),
                };
                (name, n, kind)
            })
            .filter(|(name, _, _)| !name.is_empty())
            .collect();
        found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut added = false;
        for (name, _, kind) in &found {
            if !self.row_colors.contains(name) && !self.cfg.category_colors.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                let color = self.row_colors.named(fallback_key(*kind)).map(to_rgb);
                self.cfg.category_colors.push(CategoryColor { name: name.clone(), color });
                added = true;
            }
        }
        // merge counts case-insensitively ("INFO" and "Info" are one category)
        let mut counts: Vec<(String, usize)> = Vec::new();
        for (name, n, _) in found {
            match counts.iter_mut().find(|(c, _)| c.eq_ignore_ascii_case(&name)) {
                Some(c) => c.1 += n,
                None => counts.push((name, n)),
            }
        }
        self.cat_counts = counts;
        if added {
            self.apply_colors();
            self.save_settings();
        }
    }

    fn count_of(&self, name: &str) -> Option<usize> {
        self.cat_counts.iter().find(|(c, _)| c.eq_ignore_ascii_case(name)).map(|c| c.1)
    }

    pub(super) fn colors_tab(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let now = ui.input(|i| i.time);
        let mut changed = false;
        ui.label(RichText::new("Row colors by event category").strong().size(15.0));
        ui.label(
            RichText::new(
                "Every category found in a log is added here automatically. A new one starts with the color of the default it belongs to \
                 (e.g. \"Severe\" starts red like Error, \"Debug\" light green like Information). Untick a category to leave its rows without color.",
            )
            .small()
            .color(pal.text_weak),
        );
        ui.add_space(6.0);

        let mut remove: Option<usize> = None;
        let mut move_up: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("cat_colors").max_height(ui.available_height() - 150.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("cat_grid").num_columns(6).spacing([10.0, 4.0]).striped(false).show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new("Category").small().color(pal.text_weak));
                ui.label(RichText::new("Color").small().color(pal.text_weak));
                ui.label(RichText::new("Preview").small().color(pal.text_weak));
                ui.label(RichText::new("In this log").small().color(pal.text_weak));
                ui.label("");
                ui.end_row();
                let n = self.cfg.category_colors.len();
                for i in 0..n {
                    let count = self.count_of(&self.cfg.category_colors[i].name);
                    let builtin = default_color(&self.cfg.category_colors[i].name).is_some();
                    let cc = &mut self.cfg.category_colors[i];
                    let mut on = cc.color.is_some();
                    if ui.checkbox(&mut on, "").on_hover_text("Color the rows of this category").changed() {
                        cc.color = if on { Some(default_color(&cc.name).flatten().unwrap_or([0xe6, 0xe9, 0xee])) } else { None };
                        changed = true;
                    }
                    let mut name = RichText::new(&cc.name);
                    if count.is_some() {
                        name = name.strong();
                    }
                    ui.label(name);
                    match &mut cc.color {
                        Some(c) => {
                            if egui::color_picker::color_edit_button_srgb(ui, c).on_hover_text("Click to choose the color").changed() {
                                changed = true;
                            }
                        }
                        None => {
                            ui.label(RichText::new("no color").small().color(pal.text_weak));
                        }
                    }
                    let fill = cc.color.map(rgb).unwrap_or(pal.panel);
                    preview(ui, fill, &cc.name, pal);
                    ui.label(RichText::new(count.map(fmt_num).unwrap_or_else(|| "–".into())).color(if count.is_some() { pal.text } else { pal.text_weak }));
                    ui.horizontal(|ui| {
                        if i > 0 && ui.small_button("⏶").on_hover_text("Move up").clicked() {
                            move_up = Some(i);
                        }
                        if !builtin {
                            let tip = if count.is_some() {
                                "Remove from the list (it comes back when a log contains it)"
                            } else {
                                "Remove from the list"
                            };
                            if ui.small_button("×").on_hover_text(tip).clicked() {
                                remove = Some(i);
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some(i) = remove {
            self.cfg.category_colors.remove(i);
            changed = true;
        }
        if let Some(i) = move_up {
            self.cfg.category_colors.swap(i, i - 1);
            changed = true;
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Add a category");
            let r = ui.add(egui::TextEdit::singleline(&mut self.new_category).desired_width(180.0).hint_text("name as in the log"));
            let name = self.new_category.trim().to_owned();
            let exists = self.cfg.category_colors.iter().any(|c| c.name.eq_ignore_ascii_case(&name));
            let go = ui.add_enabled(!name.is_empty() && !exists, egui::Button::new("➕ Add")).clicked()
                || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !name.is_empty() && !exists);
            if exists && !name.is_empty() {
                ui.label(RichText::new("already in the list").small().color(pal.text_weak));
            }
            if go {
                self.cfg.category_colors.push(CategoryColor { name, color: Some([0xe6, 0xe9, 0xee]) });
                self.new_category.clear();
                changed = true;
            }
        });

        ui.separator();
        ui.label(RichText::new("Selected row").strong().size(15.0));
        ui.horizontal(|ui| {
            if egui::color_picker::color_edit_button_srgb(ui, &mut self.cfg.selected_row_color).changed() {
                changed = true;
            }
            preview(ui, rgb(self.cfg.selected_row_color), "Selected row", pal);
            if self.cfg.selected_row_color != default_selected_color() && ui.small_button("Default").clicked() {
                self.cfg.selected_row_color = default_selected_color();
                changed = true;
            }
        });
        ui.add_space(6.0);
        if ui.button("↩ Reset all colors to the defaults").on_hover_text("The default categories get their original colors back; categories found in logs start again from their defaults").clicked() {
            self.cfg.category_colors = default_category_colors();
            self.cfg.selected_row_color = default_selected_color();
            self.apply_colors();
            self.discover_categories();
            changed = true;
        }
        if changed {
            self.colors_changed(now);
        }
    }
}

/// a small row-like sample with the given background
fn preview(ui: &mut egui::Ui, fill: Color32, text: &str, pal: Palette) {
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.set_min_width(170.0);
            ui.label(RichText::new(format!("12:34:56.789   {text}")).monospace().size(11.5).color(pal.text));
        });
}
