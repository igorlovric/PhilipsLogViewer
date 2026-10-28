//! Shared pattern library: sync with the PLV server, ☰ → Shared patterns window,
//! Settings → Sharing, author name prompt. Child module of `app`.

use super::*;
use crate::share::{self, Endpoint, Job, RemotePattern, SyncResult, Upload, UploadStatus};

#[derive(Default)]
pub struct ShareState {
    rx: Option<Receiver<SyncResult>>,
    /// manual sync: report the result even when nothing changed
    manual: bool,
    /// library as fetched at the last successful sync
    pub remote: Vec<RemotePattern>,
    pub synced_once: bool,
    pub last_sync: Option<String>,
    pub error: Option<String>,
    pub show: bool,
    tab: u8,
    /// uids ticked in the lists
    picked: HashSet<String>,
    /// local copies of colleagues' patterns that their author withdrew (asked one by one)
    withdrawn: Vec<String>,
    /// sync shortly after a change
    pub due: Option<f64>,
    author_edit: Option<String>,
    confirm_admin_delete: Option<String>,
}

impl App {
    fn endpoint(&self) -> Endpoint {
        let pick = |over: &str, def: &str| if over.trim().is_empty() { def.to_owned() } else { over.trim().to_owned() };
        Endpoint {
            base: pick(&self.cfg.share_server, share::DEFAULT_SERVER),
            key: pick(&self.cfg.share_api_key, share::BUILT_IN_KEY),
            admin: self.cfg.share_admin_key.trim().to_owned(),
            owner: share::owner_key(),
        }
    }

    pub(super) fn author(&self) -> String {
        if self.cfg.author_name.trim().is_empty() {
            windows_user()
        } else {
            self.cfg.author_name.trim().to_owned()
        }
    }

    /// gives every pattern a uid (patterns from before 1.8.0 have none) and makes duplicates unique
    pub(super) fn ensure_uids(&mut self) -> bool {
        let mut seen = HashSet::new();
        let mut changed = false;
        for p in &mut self.patterns {
            if p.uid.is_empty() || !seen.insert(p.uid.clone()) {
                p.uid = new_uid();
                p.synced_checksum.clear();
                p.remote_checksum.clear();
                p.foreign = false;
                seen.insert(p.uid.clone());
                changed = true;
            }
            if p.author.is_empty() && !p.foreign {
                p.author = self.cfg.author_name.clone();
            }
        }
        changed
    }

    pub(super) fn schedule_share_sync(&mut self, now: f64) {
        if self.cfg.share_sync {
            self.share.due = Some(now + 2.0);
        }
    }

    pub(super) fn start_share_sync(&mut self, manual: bool) {
        if self.share.rx.is_some() {
            return;
        }
        self.share.due = None;
        let author = self.author();
        let uploads = self
            .patterns
            .iter()
            .filter(|p| p.needs_upload())
            .map(|p| Upload { uid: p.uid.clone(), author: author.clone(), data: p.content_json(), local_checksum: p.checksum() })
            .collect();
        let job = Job { ep: self.endpoint(), uploads, withdraw: self.cfg.share_pending_withdraw.clone() };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(share::run(job));
        });
        self.share.rx = Some(rx);
        self.share.manual = manual;
    }

    pub(super) fn sharing_busy(&self) -> bool {
        self.share.rx.is_some()
    }

    /// colleagues' patterns not in the local library
    fn available(&self) -> Vec<&RemotePattern> {
        let local: HashSet<&str> = self.patterns.iter().map(|p| p.uid.as_str()).collect();
        self.share.remote.iter().filter(|r| !r.deleted && !local.contains(r.uid.as_str())).collect()
    }

    pub(super) fn new_shared_count(&self) -> usize {
        self.available().iter().filter(|r| !r.mine && !self.cfg.share_seen.contains(&r.uid)).count()
    }

    pub(super) fn poll_share(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(due) = self.share.due {
            if now >= due && self.editor.is_none() {
                self.start_share_sync(false);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((due - now).max(0.1)));
            }
        }
        let Some(rx) = &self.share.rx else { return };
        let Ok(res) = rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        };
        self.share.rx = None;
        let manual = self.share.manual;
        let mut changed = false;
        let mut content_changed = false;
        let mut msgs: Vec<String> = Vec::new();

        // uploads
        let mut uploaded = 0;
        for (uid, st) in res.uploaded {
            let Some(p) = self.patterns.iter_mut().find(|p| p.uid == uid) else { continue };
            match st {
                UploadStatus::Done(local, server) => {
                    // only if it was not edited again meanwhile
                    if p.checksum() == local {
                        p.synced_checksum = local;
                        p.remote_checksum = server;
                    }
                    uploaded += 1;
                    changed = true;
                }
                UploadStatus::Forbidden => {
                    // the uid is someone else's pattern (e.g. taken over from a colleague's config file)
                    p.foreign = true;
                    p.shared = false;
                    changed = true;
                }
                UploadStatus::Failed(e) => msgs.push(format!("“{}” was not shared: {e}", p.name)),
            }
        }
        if uploaded > 0 {
            msgs.push(format!("{uploaded} pattern(s) shared"));
        }
        if !res.withdrawn.is_empty() {
            self.cfg.share_pending_withdraw.retain(|u| !res.withdrawn.contains(u));
            changed = true;
        }

        // library
        if let Some(remote) = res.remote {
            let mut updated = 0;
            for r in &remote {
                let Some(p) = self.patterns.iter_mut().find(|p| p.uid == r.uid) else { continue };
                if p.foreign {
                    if r.deleted {
                        if !self.share.withdrawn.contains(&r.uid) {
                            self.share.withdrawn.push(r.uid.clone());
                        }
                    } else if r.checksum != p.remote_checksum {
                        if let Ok(c) = serde_json::from_str::<SharedContent>(&r.data) {
                            p.set_content(c);
                            p.author = r.author.clone();
                            p.synced_checksum = p.checksum();
                            p.remote_checksum = r.checksum.clone();
                            updated += 1;
                            changed = true;
                            content_changed = true;
                        }
                    }
                } else if r.deleted && p.shared && !p.synced_checksum.is_empty() && !self.cfg.share_pending_withdraw.contains(&r.uid) {
                    // own pattern removed from the library by an admin
                    p.shared = false;
                    p.synced_checksum.clear();
                    p.remote_checksum.clear();
                    msgs.push(format!("“{}” was removed from the shared library by an admin", p.name));
                    changed = true;
                }
            }
            if updated > 0 {
                msgs.push(format!("{updated} shared pattern(s) updated by their authors"));
            }
            self.share.remote = remote;
            self.share.synced_once = true;
            self.share.error = None;
            self.share.last_sync = Some(chrono::Local::now().format("%H:%M").to_string());
            let n = self.new_shared_count();
            if n > 0 && !self.share.show {
                msgs.push(format!("{n} new shared pattern(s) from colleagues – ☰ → Shared patterns"));
            }
        }
        if let Some(e) = res.error {
            if manual || self.share.show {
                msgs.push(format!("Shared patterns: {e}"));
            }
            self.share.error = Some(e);
        } else if manual && msgs.is_empty() {
            msgs.push("Shared patterns are up to date".to_owned());
        }
        if changed {
            if let Err(e) = self.save_cfg() {
                msgs.push(format!("Error saving: {e}"));
            }
        }
        if content_changed {
            self.run_patterns();
        }
        if !msgs.is_empty() {
            self.toast(ctx, msgs.join(" · "));
        }
    }

    /// own pattern deleted or unshared: take it out of the library at the next sync
    pub(super) fn withdraw_later(&mut self, p: &Pattern) {
        if !p.foreign && !p.synced_checksum.is_empty() && !self.cfg.share_pending_withdraw.contains(&p.uid) {
            self.cfg.share_pending_withdraw.push(p.uid.clone());
        }
    }

    fn next_pattern_id(&self) -> u64 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        self.patterns.iter().map(|p| p.id + 1).max().unwrap_or(0).max(now)
    }

    fn add_from_library(&mut self, uids: &[String]) -> usize {
        let mut n = 0;
        for uid in uids {
            let Some(r) = self.share.remote.iter().find(|r| &r.uid == uid).cloned() else { continue };
            if self.patterns.iter().any(|p| p.uid == r.uid) {
                continue;
            }
            let Ok(c) = serde_json::from_str::<SharedContent>(&r.data) else { continue };
            let mut p = Pattern::defaults_from(vec![], String::new(), String::new(), self.next_pattern_id());
            p.set_content(c);
            p.uid = r.uid.clone();
            p.author = r.author.clone();
            // own pattern from another installation of the same user stays editable
            p.foreign = !r.mine;
            p.shared = r.mine;
            p.synced_checksum = p.checksum();
            p.remote_checksum = r.checksum.clone();
            self.patterns.push(p);
            n += 1;
        }
        n
    }

    // ------------------------------------------------------------ windows

    pub(super) fn share_windows(&mut self, ctx: &egui::Context) {
        self.author_prompt(ctx);
        self.withdrawn_prompt(ctx);
        self.shared_window(ctx);
    }

    /// first start: confirm the name shown with shared patterns
    fn author_prompt(&mut self, ctx: &egui::Context) {
        if self.cfg.author_asked || self.show_about {
            return;
        }
        let pal = self.pal;
        let name = self.share.author_edit.get_or_insert_with(windows_user);
        let mut done = false;
        egui::Modal::new(Id::new("author_prompt")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Your name");
            ui.label(RichText::new("Patterns you share with colleagues are shown with this name. You can change it later in ☰ → Settings → Sharing.").small().color(pal.text_weak));
            ui.add_space(8.0);
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(300.0));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = !name.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new(RichText::new("✔  Use this name").strong().color(Color32::WHITE)).fill(pal.accent)).clicked()
                    || (ok && r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                {
                    done = true;
                }
            });
        });
        if done {
            self.cfg.author_name = self.share.author_edit.take().unwrap_or_default().trim().to_owned();
            self.cfg.author_asked = true;
            let author = self.cfg.author_name.clone();
            for p in self.patterns.iter_mut().filter(|p| !p.foreign) {
                p.author = author.clone();
            }
            self.save_settings();
        }
    }

    /// "the author withdrew X – delete it here too?"
    fn withdrawn_prompt(&mut self, ctx: &egui::Context) {
        let Some(uid) = self.share.withdrawn.first().cloned() else { return };
        if !self.cfg.author_asked || self.editor.is_some() {
            return;
        }
        let Some(p) = self.patterns.iter().find(|p| p.uid == uid && p.foreign) else {
            self.share.withdrawn.remove(0);
            return;
        };
        let (name, author) = (p.name.clone(), p.author.clone());
        let pal = self.pal;
        let mut choice = None;
        egui::Modal::new(Id::new("withdrawn_prompt")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Shared pattern withdrawn");
            ui.add_space(4.0);
            ui.label(format!("“{name}” was withdrawn from the shared library{}.", if author.is_empty() { String::new() } else { format!(" by {author}") }));
            ui.label("Delete it from your library too?");
            ui.label(RichText::new("If you keep it, it becomes your own pattern (not shared).").small().color(pal.text_weak));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("🗑  Delete").color(pal.sev_err)).clicked() {
                    choice = Some(true);
                }
                if ui.button("Keep").clicked() {
                    choice = Some(false);
                }
            });
        });
        if let Some(del) = choice {
            self.share.withdrawn.remove(0);
            if del {
                self.patterns.retain(|p| p.uid != uid);
            } else if let Some(p) = self.patterns.iter_mut().find(|p| p.uid == uid) {
                p.uid = new_uid();
                p.foreign = false;
                p.shared = false;
                p.synced_checksum.clear();
                p.remote_checksum.clear();
            }
            let _ = self.save_cfg();
            self.run_patterns();
        }
    }

    fn shared_window(&mut self, ctx: &egui::Context) {
        if !self.share.show {
            return;
        }
        let pal = self.pal;
        let mut open = true;
        let busy = self.sharing_busy();
        let admin = !self.cfg.share_admin_key.trim().is_empty();
        let mut add: Vec<String> = Vec::new();
        let mut share_toggle: Option<(String, bool)> = None;
        let mut admin_delete: Option<String> = None;
        let mut sync = false;
        let available: Vec<RemotePattern> = self.available().into_iter().cloned().collect();
        let n_new = self.new_shared_count();
        egui::Window::new(RichText::new("🎯  Shared patterns").strong())
            .open(&mut open)
            .default_size([760.0, 560.0])
            .min_size([640.0, 420.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tabs = [(0u8, format!("Available ({})", available.len())), (1u8, format!("My patterns ({})", self.patterns.iter().filter(|p| !p.foreign).count()))];
                    for (t, name) in tabs {
                        let sel = self.share.tab == t;
                        let txt = RichText::new(name).size(14.0).color(if sel { pal.accent } else { pal.text });
                        if ui.selectable_label(sel, if sel { txt.strong() } else { txt }).clicked() {
                            self.share.tab = t;
                            self.share.picked.clear();
                        }
                    }
                    if admin && ui.selectable_label(self.share.tab == 2, RichText::new("All on server (admin)").size(14.0)).clicked() {
                        self.share.tab = 2;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(!busy, egui::Button::new(if busy { "Syncing…" } else { "⟳  Sync now" })).clicked() {
                            sync = true;
                        }
                        let status = match (&self.share.error, &self.share.last_sync) {
                            (Some(e), _) => RichText::new(format!("⚠ {e}")).color(pal.sev_err).small(),
                            (None, Some(t)) => RichText::new(format!("synced {t}")).color(pal.text_weak).small(),
                            _ => RichText::new("not synced yet").color(pal.text_weak).small(),
                        };
                        ui.label(status);
                    });
                });
                ui.separator();
                match self.share.tab {
                    0 => {
                        if available.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(if self.share.synced_once { "You have all shared patterns." } else { "The shared library has not been loaded yet." }).color(pal.text_weak));
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Tick the patterns you want and click Add. Patterns with the same steps as one of yours are marked.").small().color(pal.text_weak));
                            });
                            let picked_n = available.iter().filter(|r| self.share.picked.contains(&r.uid)).count();
                            ui.horizontal(|ui| {
                                if ui.small_button("Tick all").clicked() {
                                    self.share.picked = available.iter().map(|r| r.uid.clone()).collect();
                                }
                                if ui.small_button("None").clicked() {
                                    self.share.picked.clear();
                                }
                                if ui.add_enabled(picked_n > 0, egui::Button::new(RichText::new(format!("➕  Add {picked_n}")).strong())).clicked() {
                                    add = available.iter().filter(|r| self.share.picked.contains(&r.uid)).map(|r| r.uid.clone()).collect();
                                }
                            });
                            ui.add_space(4.0);
                            let local_keys: Vec<(String, String)> = self.patterns.iter().map(|p| (menu_ui::steps_key(p), p.name.clone())).collect();
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                for r in &available {
                                    let Ok(c) = serde_json::from_str::<SharedContent>(&r.data) else { continue };
                                    let mut tmp = Pattern::defaults_from(vec![], String::new(), String::new(), 0);
                                    tmp.set_content(c);
                                    let same = local_keys.iter().find(|(k, _)| *k == menu_ui::steps_key(&tmp)).map(|(_, n)| n.clone());
                                    let is_new = !r.mine && !self.cfg.share_seen.contains(&r.uid);
                                    pattern_card(ui, &pal, &tmp, |ui| {
                                        let mut on = self.share.picked.contains(&r.uid);
                                        if ui.checkbox(&mut on, "").changed() {
                                            if on {
                                                self.share.picked.insert(r.uid.clone());
                                            } else {
                                                self.share.picked.remove(&r.uid);
                                            }
                                        }
                                    }, |ui| {
                                        if is_new {
                                            ui.label(RichText::new("NEW").small().strong().color(pal.accent));
                                        }
                                        let who = if r.mine { "you (another computer)".to_owned() } else { r.author.clone() };
                                        ui.label(RichText::new(format!("by {who} · {}", r.updated_at.get(..10).unwrap_or(""))).small().color(pal.text_weak));
                                        if let Some(n) = &same {
                                            ui.label(RichText::new(format!("same steps as your “{n}”")).small().color(pal.sev_warn));
                                        }
                                    });
                                }
                            });
                        }
                    }
                    1 => {
                        ui.label(RichText::new("Tick “Share” to put a pattern into the shared library; untick it to take it out again. Changes you make to a shared pattern are sent automatically.").small().color(pal.text_weak));
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            let mine: Vec<Pattern> = self.patterns.iter().filter(|p| !p.foreign).cloned().collect();
                            if mine.is_empty() {
                                ui.label(RichText::new("You have no own patterns yet.").color(pal.text_weak));
                            }
                            for p in &mine {
                                pattern_card(ui, &pal, p, |ui| {
                                    let mut on = p.shared;
                                    if ui.checkbox(&mut on, "Share").changed() {
                                        share_toggle = Some((p.uid.clone(), on));
                                    }
                                }, |ui| {
                                    let (t, c) = share_state(p);
                                    ui.label(RichText::new(t).small().color(match c {
                                        0 => pal.text_weak,
                                        1 => pal.accent,
                                        _ => pal.sev_warn,
                                    }));
                                });
                            }
                            let foreign = self.patterns.iter().filter(|p| p.foreign).count();
                            if foreign > 0 {
                                ui.add_space(6.0);
                                ui.label(RichText::new(format!("{foreign} pattern(s) in your library come from colleagues – they are kept up to date automatically; editing one makes your own copy.")).small().color(pal.text_weak));
                            }
                        });
                    }
                    _ => {
                        ui.label(RichText::new("Admin: withdraw any pattern from the shared library (it stays in the server history).").small().color(pal.text_weak));
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            for r in self.share.remote.iter().filter(|r| !r.deleted) {
                                ui.horizontal(|ui| {
                                    if self.share.confirm_admin_delete.as_deref() == Some(r.uid.as_str()) {
                                        if ui.button(RichText::new("Confirm withdraw").color(pal.sev_err)).clicked() {
                                            admin_delete = Some(r.uid.clone());
                                        }
                                    } else if ui.button("🗑").on_hover_text("Withdraw from the shared library").clicked() {
                                        self.share.confirm_admin_delete = Some(r.uid.clone());
                                    }
                                    ui.label(RichText::new(&r.name).strong());
                                    ui.label(RichText::new(format!("by {} · {}", r.author, r.updated_at)).small().color(pal.text_weak));
                                });
                            }
                        });
                    }
                }
            });
        if !open && n_new > 0 {
            // everything listed counts as seen once the window is closed
            for r in &available {
                if !self.cfg.share_seen.contains(&r.uid) {
                    self.cfg.share_seen.push(r.uid.clone());
                }
            }
            self.cfg_save_due = Some(ctx.input(|i| i.time) + 1.0);
        }
        if !add.is_empty() {
            let n = self.add_from_library(&add);
            self.share.picked.clear();
            let _ = self.save_cfg();
            self.run_patterns();
            self.toast(ctx, format!("{n} pattern(s) added · {} occurrences recognized in this log", self.total_occurrences()));
        }
        if let Some((uid, on)) = share_toggle {
            if let Some(p) = self.patterns.iter().find(|p| p.uid == uid).cloned() {
                if !on {
                    self.withdraw_later(&p);
                } else {
                    self.cfg.share_pending_withdraw.retain(|u| *u != uid);
                }
                if let Some(x) = self.patterns.iter_mut().find(|p| p.uid == uid) {
                    x.shared = on;
                    if !on {
                        x.synced_checksum.clear();
                        x.remote_checksum.clear();
                    }
                    if x.author.is_empty() {
                        x.author = self.cfg.author_name.clone();
                    }
                }
                let _ = self.save_cfg();
                self.schedule_share_sync(ctx.input(|i| i.time));
            }
        }
        if let Some(uid) = admin_delete {
            self.share.confirm_admin_delete = None;
            let ep = self.endpoint();
            match share::withdraw(&ep, &uid) {
                Ok(()) => sync = true,
                Err(e) => self.toast(ctx, format!("Cannot withdraw: {e}")),
            }
        }
        if sync {
            self.start_share_sync(true);
        }
        if !open {
            self.share.show = false;
        }
    }

    pub(super) fn sharing_tab(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        ui.label(RichText::new("Shared patterns").strong().size(15.0));
        ui.label(RichText::new("Patterns you choose to share go to a common library; colleagues' patterns can be added from ☰ → Shared patterns.").small().color(pal.text_weak));
        ui.add_space(6.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Your name");
            changed |= ui.add(egui::TextEdit::singleline(&mut self.cfg.author_name).desired_width(220.0).hint_text(windows_user())).changed();
        });
        changed |= ui.checkbox(&mut self.cfg.share_sync, "Sync the shared library at startup and after changes").changed();
        ui.add_space(6.0);
        let ep = self.endpoint();
        ui.label(RichText::new(format!("Server: {}", ep.base)).small().color(pal.text_weak));
        if !self.cfg.share_server.trim().is_empty() {
            ui.label(RichText::new("(server set in the config file instead of the built-in one)").small().color(pal.sev_warn));
        }
        if !self.cfg.share_admin_key.trim().is_empty() {
            ui.label(RichText::new("Admin key is set – ☰ → Shared patterns has an admin tab").small().color(pal.accent));
        }
        if let Some(e) = &self.share.error {
            ui.label(RichText::new(format!("Last sync failed: {e}")).small().color(pal.sev_err));
        }
        ui.add_space(6.0);
        if ui.add_enabled(!self.sharing_busy(), egui::Button::new("⟳  Sync now")).clicked() {
            self.start_share_sync(true);
        }
        if changed {
            let author = self.author();
            for p in self.patterns.iter_mut().filter(|p| !p.foreign) {
                p.author = author.clone();
            }
            self.cfg_save_due = Some(ui.input(|i| i.time) + 0.8);
        }
    }
}

/// (text, 0 = neutral / 1 = good / 2 = pending)
fn share_state(p: &Pattern) -> (&'static str, u8) {
    if p.foreign {
        ("from a colleague", 0)
    } else if !p.shared {
        ("not shared", 0)
    } else if p.needs_upload() {
        ("waiting to be sent", 2)
    } else {
        ("shared ✔", 1)
    }
}

pub(super) fn share_label(p: &Pattern) -> Option<(String, u8)> {
    if p.foreign {
        Some((format!("from {}", if p.author.is_empty() { "a colleague" } else { &p.author }), 0))
    } else if p.shared {
        let (t, c) = share_state(p);
        Some((t.to_owned(), c))
    } else {
        None
    }
}

fn pattern_card(ui: &mut egui::Ui, pal: &Palette, p: &Pattern, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(pal.panel2).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            left(ui);
            let (rr, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(rr, 3.0, rgb(p.color));
            ui.label(RichText::new(&p.name).strong());
            ui.label(RichText::new(format!("{} steps · ~{}", p.steps.len(), fmt_duration(p.span_ms()))).small().color(pal.text_weak));
            right(ui);
        });
        if !p.description.is_empty() {
            ui.label(RichText::new(&p.description).small());
        }
        if !p.comment.is_empty() {
            ui.label(RichText::new(format!("💬 {}", p.comment)).small().italics().color(pal.text_weak));
        }
    });
    ui.add_space(4.0);
}

/// name of the logged-in Windows user
pub fn windows_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default()
}
