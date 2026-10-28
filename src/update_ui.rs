//! "Update available" button and dialog, Settings → Updates. Child module of `app`.

use super::*;
use crate::update::{self, Progress, Release};
use std::sync::atomic::Ordering;

#[derive(Default)]
pub struct UpdateState {
    /// running check (manual = started from the menu, shows "up to date" too)
    check: Option<(bool, Receiver<Result<Release, String>>)>,
    pub available: Option<Release>,
    pub show: bool,
    /// running download (auto = background download, installed when the program closes)
    download: Option<(bool, Arc<Progress>, Receiver<Result<PathBuf, String>>)>,
    error: Option<String>,
    /// downloaded in the background, put in place when the program closes
    pub install_on_exit: Option<PathBuf>,
    /// installed now – start the new exe when this one has closed
    pub restart: Option<PathBuf>,
}

/// version the running program compares against (debug builds can pretend to be older for testing)
fn local_version() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("PLV_FAKE_VERSION") {
        return v;
    }
    crate::version::VERSION.to_owned()
}

fn mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}

impl App {
    pub(super) fn start_update_check(&mut self, manual: bool) {
        if self.upd.check.is_some() || self.upd.download.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(update::latest());
        });
        self.upd.check = Some((manual, rx));
    }

    fn start_download(&mut self, auto: bool) {
        let Some(rel) = self.upd.available.clone() else { return };
        let prog = Arc::new(Progress::default());
        let p2 = prog.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(update::download(&rel, &p2));
        });
        self.upd.error = None;
        self.upd.download = Some((auto, prog, rx));
    }

    /// installs the downloaded exe and closes the program; the new one starts with the same log
    fn install_now(&mut self, ctx: &egui::Context, file: PathBuf) {
        if self.log_save_due.is_some() || self.log_changed {
            self.save_log_file();
        }
        match update::install(&file) {
            Ok(exe) => {
                self.upd.restart = Some(exe);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&file);
                self.upd.error = Some(e);
            }
        }
    }

    /// called from on_exit: background update goes in place, or the freshly installed exe is started
    pub(super) fn update_on_exit(&mut self) {
        if let Some(f) = self.upd.install_on_exit.take() {
            let _ = update::install(&f);
        }
        if let Some(exe) = self.upd.restart.take() {
            let log = self.log.as_ref().map(|l| l.path.clone());
            let _ = update::restart(&exe, log.as_deref());
        }
    }

    /// polls the background check / download; called every frame
    pub(super) fn poll_update(&mut self, ctx: &egui::Context) {
        if let Some((manual, rx)) = &self.upd.check {
            let manual = *manual;
            if let Ok(res) = rx.try_recv() {
                self.upd.check = None;
                match res {
                    Ok(rel) if update::is_newer(&rel.version, &local_version()) => {
                        let skipped = !manual && rel.version == self.cfg.skipped_version;
                        if !skipped {
                            self.upd.available = Some(rel);
                            if manual {
                                self.upd.show = true;
                            } else if self.cfg.auto_update {
                                self.start_download(true);
                            }
                        }
                    }
                    Ok(_) => {
                        if manual {
                            self.toast(ctx, format!("You have the latest version ({})", crate::version::VERSION));
                        }
                    }
                    Err(e) => {
                        if manual {
                            self.toast(ctx, format!("Could not check for updates: {e}"));
                        }
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        if let Some((auto, _, rx)) = &self.upd.download {
            let auto = *auto;
            if let Ok(res) = rx.try_recv() {
                self.upd.download = None;
                match res {
                    Ok(file) if auto => self.upd.install_on_exit = Some(file),
                    Ok(file) => self.install_now(ctx, file),
                    Err(e) => {
                        if !auto {
                            self.upd.error = Some(e);
                        }
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    /// small button in the top bar while an update is available
    pub(super) fn update_badge(&mut self, ui: &mut egui::Ui) {
        let Some(rel) = &self.upd.available else { return };
        let pal = self.pal;
        let (text, tip) = if self.upd.install_on_exit.is_some() {
            (format!("⏶  {} ready", rel.version), "The new version is downloaded and is installed when you close the program")
        } else {
            (format!("⏶  Update {}", rel.version), "A new version is available – click for details")
        };
        let b = egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(pal.accent).stroke(Stroke::new(1.0_f32, pal.accent));
        if ui.add(b).on_hover_text(tip).clicked() {
            self.upd.show = true;
        }
    }

    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        if !self.upd.show {
            return;
        }
        let Some(rel) = self.upd.available.clone() else {
            self.upd.show = false;
            return;
        };
        let pal = self.pal;
        let mut open = true;
        let mut action: Option<&str> = None;
        egui::Window::new(RichText::new("⏶  Update available").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Philips Log Viewer {}", rel.version)).size(17.0).strong());
                ui.label(RichText::new(format!("You have version {}", local_version())).color(pal.text_weak));
                ui.add_space(6.0);
                if !rel.notes.is_empty() {
                    ui.label(RichText::new("What's new").strong());
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for line in rel.notes.lines().map(str::trim).filter(|l| !l.is_empty()) {
                            let l = line.trim_start_matches(['-', '*']).trim();
                            if line.starts_with('#') {
                                ui.label(RichText::new(line.trim_start_matches('#').trim()).strong());
                            } else {
                                ui.horizontal_top(|ui| {
                                    ui.label(RichText::new("•").color(pal.accent));
                                    ui.add(egui::Label::new(l).wrap());
                                });
                            }
                        }
                    });
                }
                ui.add_space(8.0);
                if let Some((_, prog, _)) = &self.upd.download {
                    let done = prog.done.load(Ordering::Relaxed);
                    let total = prog.total.load(Ordering::Relaxed).max(1);
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(format!("Downloading… {} / {}", mb(done), mb(total))));
                    if ui.button("Cancel").clicked() {
                        prog.cancel.store(true, Ordering::Relaxed);
                    }
                    return;
                }
                if let Some(e) = &self.upd.error {
                    ui.label(RichText::new(format!("Update failed: {e}")).color(pal.sev_err));
                    ui.add_space(4.0);
                }
                if self.upd.install_on_exit.is_some() {
                    ui.label(RichText::new("Already downloaded – it is installed when you close the program.").color(pal.accent));
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Install and restart now").strong()).clicked() {
                            action = Some("install");
                        }
                        if ui.button("Later").clicked() {
                            action = Some("later");
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("The program downloads the new version, replaces itself and starts again with the same log. Settings, patterns and notes stay as they are.")
                            .small()
                            .color(pal.text_weak),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("⏶  Update now").color(Color32::WHITE).strong()).fill(pal.accent)).clicked() {
                            action = Some("update");
                        }
                        if ui.button("Later").clicked() {
                            action = Some("later");
                        }
                        if ui.button("Skip this version").on_hover_text("Do not offer this version again at startup").clicked() {
                            action = Some("skip");
                        }
                    });
                }
                if !rel.page.is_empty() {
                    ui.add_space(4.0);
                    ui.hyperlink_to(RichText::new("Release page on GitHub").small(), &rel.page);
                }
            });
        match action {
            Some("update") => self.start_download(false),
            Some("install") => {
                if let Some(f) = self.upd.install_on_exit.take() {
                    self.install_now(ctx, f);
                }
            }
            Some("later") => self.upd.show = false,
            Some("skip") => {
                self.cfg.skipped_version = rel.version.clone();
                self.save_settings();
                self.upd.available = None;
                self.upd.show = false;
            }
            _ => {}
        }
        if !open {
            self.upd.show = false;
        }
    }

    pub(super) fn updates_tab(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        ui.label(RichText::new("Program updates").strong().size(15.0));
        ui.label(RichText::new(format!("Version {} · new versions come from github.com/{}", crate::version::VERSION, update::REPO)).small().color(pal.text_weak));
        ui.add_space(6.0);
        let mut changed = false;
        changed |= ui.checkbox(&mut self.cfg.check_updates, "Check for a new version when the program starts").changed();
        ui.add_enabled_ui(self.cfg.check_updates, |ui| {
            changed |= ui
                .checkbox(&mut self.cfg.auto_update, "Install updates automatically")
                .on_hover_text("A new version is downloaded in the background and put in place when you close the program")
                .changed();
        });
        ui.label(
            RichText::new("Without automatic installation a button appears in the top right corner when a new version is available, and you decide when to update.")
                .small()
                .color(pal.text_weak),
        );
        if !self.cfg.skipped_version.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Skipped version: {}", self.cfg.skipped_version)).small());
                if ui.small_button("Offer it again").clicked() {
                    self.cfg.skipped_version.clear();
                    changed = true;
                }
            });
        }
        ui.add_space(8.0);
        let busy = self.upd.check.is_some() || self.upd.download.is_some();
        if ui.add_enabled(!busy, egui::Button::new("Check now")).clicked() {
            self.start_update_check(true);
        }
        if changed {
            self.save_settings();
        }
    }
}
