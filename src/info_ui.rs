//! Window with what is known about a file that is not an event log (encrypted file, archive
//! without a log): rows of information, a notice, and buttons that open the documents in it.
//! Child module of `app`.

use super::*;
use crate::files;
use std::path::Path;

impl App {
    /// a file from Files in ZIP: show the info window instead of opening it, when it is encrypted
    pub(super) fn entry_info(&mut self, entry: &ArchiveEntry) -> bool {
        let Some(log) = &self.log else { return false };
        let zip = log.path.clone();
        let Ok(bytes) = parser::read_zip_entry(&zip, entry.index) else { return false };
        let head = &bytes[..bytes.len().min(256 * 1024)];
        match crate::docinfo::inspect_entry(&zip, entry, head) {
            Some(mut info) => {
                crate::docinfo::relabel(&mut info, &self.cfg.product_names);
                self.file_info = Some(info);
                true
            }
            None => false,
        }
    }

    fn info_doc_external(&mut self, ctx: &egui::Context, zip: &Path, entry: &ArchiveEntry) {
        let r = (|| -> Result<(), String> {
            let bytes = parser::read_zip_entry(zip, entry.index)?;
            let dir = std::env::temp_dir().join("PhilipsLogViewer");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let p = dir.join(entry.file_name());
            std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
            files_ui::open_with_system(&p).map_err(|e| e.to_string())
        })();
        match r {
            Ok(()) => self.toast(ctx, format!("Opening {} …", entry.file_name())),
            Err(e) => self.toast(ctx, format!("Cannot open: {e}")),
        }
    }

    fn info_doc_save(&mut self, ctx: &egui::Context, zip: &Path, entry: &ArchiveEntry) {
        let mut d = rfd::FileDialog::new().set_file_name(entry.file_name());
        if let Some(dir) = zip.parent() {
            d = d.set_directory(dir);
        }
        let Some(dest) = d.save_file() else { return };
        match parser::read_zip_entry(zip, entry.index).and_then(|b| std::fs::write(&dest, b).map_err(|e| e.to_string())) {
            Ok(()) => self.toast(ctx, format!("Saved {}", dest.display())),
            Err(e) => self.toast(ctx, format!("Cannot save: {e}")),
        }
    }

    pub(super) fn file_info_window(&mut self, ctx: &egui::Context) {
        let Some(info) = self.file_info.clone() else { return };
        let pal = self.pal;
        let mut open = true;
        let mut ext: Option<ArchiveEntry> = None;
        let mut save: Option<ArchiveEntry> = None;
        let mut copy = false;
        egui::Window::new(RichText::new(format!("ℹ  {}", info.title)).strong())
            .id(Id::new("file_info"))
            .open(&mut open)
            .default_width(720.0)
            .max_height(ctx.screen_rect().height() - 80.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(&info.kind).size(15.0).strong());
                ui.add_space(6.0);
                if let Some(n) = &info.notice {
                    let encrypted = info.documents.is_empty();
                    egui::Frame::new()
                        .fill(if encrypted { Color32::from_rgb(0xff, 0xf1, 0xd6) } else { pal.panel2 })
                        .stroke(Stroke::new(1.0_f32, if encrypted { pal.sev_warn } else { pal.border }))
                        .corner_radius(6.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                ui.label(RichText::new(if encrypted { "⚠" } else { "ℹ" }).size(18.0));
                                ui.add(egui::Label::new(RichText::new(n).color(pal.text)).wrap());
                            });
                        });
                    ui.add_space(6.0);
                }
                egui::Grid::new("fi_rows").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for (k, v) in &info.rows {
                        ui.label(RichText::new(k).color(pal.text_weak));
                        ui.add(egui::Label::new(RichText::new(v).strong()).selectable(true));
                        ui.end_row();
                    }
                });
                if ui.small_button("📋 Copy information").clicked() {
                    copy = true;
                }
                if !info.documents.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Documents").strong().size(15.0));
                    egui::Grid::new("fi_docs").num_columns(4).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                        for d in &info.documents {
                            let pdf = d.entry.name.to_ascii_lowercase().ends_with(".pdf");
                            ui.horizontal(|ui| {
                                let b = if pdf {
                                    egui::Button::new(RichText::new("📄 Open").color(Color32::WHITE).strong()).fill(pal.accent)
                                } else {
                                    egui::Button::new("↗ Open")
                                };
                                if ui.add(b).on_hover_text("Open with the Windows default program").clicked() {
                                    ext = Some(d.entry.clone());
                                }
                                if ui.small_button("💾").on_hover_text("Save a copy…").clicked() {
                                    save = Some(d.entry.clone());
                                }
                            });
                            ui.label(RichText::new(d.entry.file_name()).strong());
                            ui.label(RichText::new(&d.description).small().color(pal.text_weak));
                            ui.label(RichText::new(files::fmt_size(d.entry.size)).small().color(pal.text_weak));
                            ui.end_row();
                        }
                    });
                }
                if !info.hardware.is_empty() {
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new(RichText::new(format!("Hardware ({})", info.hardware.len())).strong()).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("fi_hw").max_height(260.0).show(ui, |ui| {
                            egui::Grid::new("fi_hw_g").num_columns(4).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
                                for h in ["Type", "Part number", "Serial number", "Version"] {
                                    ui.label(RichText::new(h).small().color(pal.text_weak));
                                }
                                ui.end_row();
                                for r in &info.hardware {
                                    for (i, v) in r.iter().enumerate() {
                                        let t = RichText::new(v).small();
                                        ui.add(egui::Label::new(if i == 0 { t.strong() } else { t.monospace() }).selectable(true));
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    });
                }
                if !info.software.is_empty() {
                    egui::CollapsingHeader::new(RichText::new(format!("Software parts ({})", info.software.len())).strong()).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("fi_sw").max_height(220.0).show(ui, |ui| {
                            egui::Grid::new("fi_sw_g").num_columns(2).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
                                for r in &info.software {
                                    ui.label(RichText::new(&r[0]).small());
                                    ui.label(RichText::new(&r[1]).small().color(pal.text_weak));
                                    ui.end_row();
                                }
                            });
                        });
                    });
                }
            });
        if copy {
            let text: Vec<String> = std::iter::once(info.kind.clone()).chain(info.rows.iter().map(|(k, v)| format!("{k}: {v}"))).collect();
            ctx.copy_text(text.join("\n"));
            self.toast(ctx, "Copied");
        }
        if let Some(e) = ext {
            self.info_doc_external(ctx, &info.path, &e);
        }
        if let Some(e) = save {
            self.info_doc_save(ctx, &info.path, &e);
        }
        if !open {
            self.file_info = None;
        }
    }
}
