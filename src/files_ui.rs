//! UI for the other files in the ZIP: file list, text viewer, "open as table",
//! open externally / save, and the system info tree. Child module of `app`.

use super::*;
use crate::files::{self, TextKind};
use crate::xmltree::XmlNode;
use std::collections::BTreeMap;
use std::path::Path;

pub struct Viewer {
    pub entry: ArchiveEntry,
    pub text: String,
    /// ASCII-lowercased copy of `text` (same byte offsets) for searching
    lower: String,
    pub lines: Vec<(u32, u32)>,
    pub levels: Vec<u8>,
    pub encoding: &'static str,
    pub kind: TextKind,
    /// hex dump when the file is binary
    pub hex: Option<String>,
    search: String,
    hits: Vec<u32>,
    cur: usize,
    scroll_to: Option<usize>,
    only_problems: bool,
    visible: Vec<u32>,
}

impl Viewer {
    pub fn load(zip: &Path, entry: &ArchiveEntry) -> Result<Viewer, String> {
        let bytes = parser::read_zip_entry(zip, entry.index)?;
        let decoded = if files::is_binary_name(&entry.name) { None } else { files::decode(&bytes) };
        let mut v = Viewer {
            entry: entry.clone(),
            text: String::new(),
            lower: String::new(),
            lines: vec![],
            levels: vec![],
            encoding: "",
            kind: TextKind::Plain,
            hex: None,
            search: String::new(),
            hits: vec![],
            cur: 0,
            scroll_to: None,
            only_problems: false,
            visible: vec![],
        };
        match decoded {
            None => v.hex = Some(files::hex_dump(&bytes, 64 * 1024)),
            Some((text, enc)) => {
                v.lines = files::line_offsets(&text);
                v.levels = v.lines.iter().map(|&(a, b)| files::line_level(&text[a as usize..b as usize])).collect();
                v.kind = files::detect_kind(&text);
                v.lower = text.to_ascii_lowercase();
                v.encoding = enc;
                v.text = text;
                v.visible = (0..v.lines.len() as u32).collect();
            }
        }
        Ok(v)
    }

    fn refilter(&mut self) {
        let q = self.search.trim().to_ascii_lowercase();
        self.visible.clear();
        self.hits.clear();
        for i in 0..self.lines.len() {
            if self.only_problems && self.levels[i] == 0 {
                continue;
            }
            let (a, b) = self.lines[i];
            let hit = !q.is_empty() && self.lower[a as usize..b as usize].contains(q.as_str());
            if hit {
                self.hits.push(self.visible.len() as u32);
            }
            self.visible.push(i as u32);
        }
        self.cur = 0;
        if let Some(&h) = self.hits.first() {
            self.scroll_to = Some(h as usize);
        }
    }
}

/// a cell of fixed width (left aligned), so truncated labels keep their space
fn fixed<R>(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(egui::vec2(w, ui.spacing().interact_size.y), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(w);
        add(ui)
    })
    .inner
}

pub(super) fn open_with_system(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer").arg(path).spawn().map(|_| ())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }
}

fn highlight_line(s: &str, lower: &str, q: &str, pal: &Palette, font: egui::FontId) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat { font_id: font.clone(), color: pal.text, ..Default::default() };
    let hl = egui::TextFormat { font_id: font, color: pal.text, background: Color32::from_rgb(0xff, 0xd3, 0x4d), ..Default::default() };
    if q.is_empty() {
        job.append(s, 0.0, normal);
        return job;
    }
    let mut i = 0;
    while let Some(p) = lower[i..].find(q) {
        let a = i + p;
        let b = a + q.len();
        if !s.is_char_boundary(a) || !s.is_char_boundary(b) {
            break;
        }
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

impl App {
    pub(super) fn open_entry_viewer(&mut self, ctx: &egui::Context, entry: ArchiveEntry) {
        let Some(log) = &self.log else { return };
        let zip = log.path.clone();
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Viewer::load(&zip, &entry));
            ctx2.request_repaint();
        });
        self.viewer_rx = Some(rx);
    }

    /// Loads a text/trace log from the ZIP into the main table.
    pub(super) fn open_entry_table(&mut self, ctx: &egui::Context, entry: ArchiveEntry) {
        if self.log_save_due.is_some() {
            self.save_log_file();
        }
        let Some(log) = &self.log else { return };
        let zip = log.path.clone();
        let archive = log.archive.clone();
        let info = log.info.clone();
        let sysinfo = log.sysinfo.clone();
        let (tx, rx) = channel();
        let progress = Progress::new();
        let ctx2 = ctx.clone();
        let name = entry.file_name().to_owned();
        std::thread::spawn(move || {
            let r = (|| {
                let bytes = parser::read_zip_entry(&zip, entry.index)?;
                let lname = entry.name.to_ascii_lowercase();
                if lname.ends_with(".mdb") || lname.ends_with(".accdb") {
                    // Access database (CT log): needs a real file
                    let dir = std::env::temp_dir().join("PhilipsLogViewer");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let tmp = dir.join(entry.file_name());
                    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
                    let mut log = crate::mdb::load(&tmp)?;
                    log.path = zip.clone();
                    log.entry = Some(entry.name.clone());
                    log.archive = archive;
                    return Ok(log);
                }
                let (text, _) = files::decode(&bytes).ok_or("This is a binary file")?;
                let kind = files::detect_kind(&text);
                let mut log = files::text_to_log(&zip, &entry, &text, kind)?;
                log.archive = archive;
                log.info = info;
                log.sysinfo = sysinfo;
                Ok(log)
            })();
            let _ = tx.send(r.map(Loaded::Log));
            ctx2.request_repaint();
        });
        self.show_files = false;
        self.error = None;
        self.loading = Some(Loading { rx, progress, name });
    }

    fn entry_external(&mut self, ctx: &egui::Context, entry: &ArchiveEntry) {
        let Some(log) = &self.log else { return };
        let r = (|| -> Result<PathBuf, String> {
            let bytes = parser::read_zip_entry(&log.path, entry.index)?;
            let dir = std::env::temp_dir().join("PhilipsLogViewer");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let p = dir.join(entry.file_name());
            std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
            open_with_system(&p).map_err(|e| e.to_string())?;
            Ok(p)
        })();
        match r {
            Ok(_) => self.toast(ctx, format!("Opening {} …", entry.file_name())),
            Err(e) => self.toast(ctx, format!("Cannot open: {e}")),
        }
    }

    fn entry_save(&mut self, ctx: &egui::Context, entry: &ArchiveEntry) {
        let Some(log) = &self.log else { return };
        let mut d = rfd::FileDialog::new().set_file_name(entry.file_name());
        if let Some(dir) = log.path.parent() {
            d = d.set_directory(dir);
        }
        let Some(dest) = d.save_file() else { return };
        let r = parser::read_zip_entry(&log.path, entry.index).and_then(|b| std::fs::write(&dest, b).map_err(|e| e.to_string()));
        match r {
            Ok(()) => self.toast(ctx, format!("Saved {}", dest.display())),
            Err(e) => self.toast(ctx, format!("Cannot save: {e}")),
        }
    }

    pub(super) fn poll_viewer(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.viewer_rx {
            match rx.try_recv() {
                Ok(Ok(v)) => {
                    self.viewer = Some(v);
                    self.viewer_rx = None;
                }
                Ok(Err(e)) => {
                    self.viewer_rx = None;
                    self.toast(ctx, format!("Cannot read file: {e}"));
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
            }
        }
    }

    // ------------------------------------------------------------ file list

    pub(super) fn files_window(&mut self, ctx: &egui::Context) {
        if !self.show_files {
            return;
        }
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let mut open = true;
        enum Act {
            View(ArchiveEntry),
            Table(ArchiveEntry),
            External(ArchiveEntry),
            Save(ArchiveEntry),
            Main,
        }
        let mut act: Option<Act> = None;
        let current = log.entry.clone();
        let q = self.files_filter.trim().to_lowercase();
        let mut groups: BTreeMap<String, Vec<&ArchiveEntry>> = BTreeMap::new();
        for e in &log.archive {
            if !q.is_empty() && !e.name.to_lowercase().contains(&q) && !files::describe(&e.name).is_some_and(|d| d.to_lowercase().contains(&q)) {
                continue;
            }
            let folder = e.name.rsplit_once('/').map(|(a, _)| a.to_owned()).unwrap_or_default();
            groups.entry(folder).or_default().push(e);
        }
        let total: u64 = log.archive.iter().map(|e| e.size).sum();
        let shown: usize = groups.values().map(|v| v.len()).sum();
        egui::Window::new(RichText::new("📁  Files in this ZIP").strong())
            .open(&mut open)
            .default_size([960.0, 640.0])
            .min_width(520.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.files_filter).hint_text("🔍  filter by name or description…").desired_width(300.0));
                    ui.label(RichText::new(format!("{} of {} files · {}", shown, log.archive.len(), files::fmt_size(total))).color(pal.text_weak));
                });
                ui.label(
                    RichText::new("Click a name to view it · 📊 opens a log as a table (filters, timeline, patterns) · ↗ opens it in Windows (e.g. .evtx in Event Viewer) · 💾 saves a copy")
                        .small()
                        .color(pal.text_weak),
                );
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (folder, list) in &groups {
                        let title = format!("📁 {}   ({})", if folder.is_empty() { "/" } else { folder.as_str() }, list.len());
                        egui::CollapsingHeader::new(RichText::new(title).strong())
                            .id_salt(("folder", folder))
                            .default_open(list.len() <= 40)
                            .open(if q.is_empty() { None } else { Some(true) })
                            .show(ui, |ui| {
                                egui::Grid::new(("files", folder)).striped(true).num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
                                    for e in list {
                                        let name = e.file_name();
                                        let is_current = current.as_deref() == Some(e.name.as_str()) || (current.is_none() && e.is_events);
                                        let mut t = RichText::new(format!("{} {}", files::icon(name), name));
                                        if is_current {
                                            t = t.strong().color(pal.accent);
                                        }
                                        let r = fixed(ui, 360.0, |ui| ui.add(egui::Label::new(t).sense(Sense::click()).truncate())).on_hover_text(&e.name);
                                        if r.clicked() {
                                            act = Some(if e.is_events { Act::Main } else { Act::View((*e).clone()) });
                                        }
                                        fixed(ui, 64.0, |ui| ui.label(RichText::new(files::fmt_size(e.size)).color(pal.text_weak)));
                                        let desc = if is_current {
                                            "● shown in the table".to_owned()
                                        } else if e.is_events {
                                            "Event log (main table)".to_owned()
                                        } else {
                                            files::describe(&e.name).unwrap_or("").to_owned()
                                        };
                                        fixed(ui, 300.0, |ui| ui.add(egui::Label::new(RichText::new(desc).small().color(if is_current { pal.accent } else { pal.text_weak })).truncate()));
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            let binary = files::is_binary_name(&e.name);
                                            if e.is_events {
                                                if !is_current && ui.small_button("📊").on_hover_text("Back to the main event log").clicked() {
                                                    act = Some(Act::Main);
                                                }
                                            } else if !binary && e.size > 0 && !e.name.to_ascii_lowercase().ends_with(".xml") && ui.small_button("📊").on_hover_text("Open as a table").clicked() {
                                                act = Some(Act::Table((*e).clone()));
                                            }
                                            if !e.is_events && ui.small_button("📄").on_hover_text("View").clicked() {
                                                act = Some(Act::View((*e).clone()));
                                            }
                                            if ui.small_button("↗").on_hover_text("Open with the Windows default program").clicked() {
                                                act = Some(Act::External((*e).clone()));
                                            }
                                            if ui.small_button("💾").on_hover_text("Save a copy…").clicked() {
                                                act = Some(Act::Save((*e).clone()));
                                            }
                                        });
                                        ui.end_row();
                                    }
                                });
                            });
                    }
                });
            });
        if !open {
            self.show_files = false;
        }
        match act {
            Some(Act::View(e)) | Some(Act::Table(e)) if self.entry_info(&e) => {}
            Some(Act::View(e)) => self.open_entry_viewer(ctx, e),
            Some(Act::Table(e)) => self.open_entry_table(ctx, e),
            Some(Act::External(e)) => self.entry_external(ctx, &e),
            Some(Act::Save(e)) => self.entry_save(ctx, &e),
            Some(Act::Main) => self.back_to_main(ctx),
            None => {}
        }
    }

    pub(super) fn back_to_main(&mut self, ctx: &egui::Context) {
        let Some(log) = &self.log else { return };
        if log.entry.is_some() {
            let p = log.path.clone();
            self.show_files = false;
            self.open(ctx, p);
        }
    }

    // ------------------------------------------------------------ text viewer

    pub(super) fn viewer_window(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some(v) = &mut self.viewer else { return };
        let mut open = true;
        let mut as_table = false;
        let mut external = false;
        let mut save = false;
        let title = format!("📄  {}", v.entry.file_name());
        egui::Window::new(RichText::new(title).strong())
            .id(Id::new("file_viewer"))
            .open(&mut open)
            .default_size([1040.0, 680.0])
            .min_width(480.0)
            .min_height(240.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&v.entry.name).small().color(pal.text_weak));
                    ui.label(RichText::new(format!("· {}", files::fmt_size(v.entry.size))).small().color(pal.text_weak));
                    if v.hex.is_none() {
                        ui.label(RichText::new(format!("· {} · {} lines · {}", v.encoding, fmt_num(v.lines.len()), v.kind.label())).small().color(pal.text_weak));
                    }
                    if let Some(d) = files::describe(&v.entry.name) {
                        ui.label(RichText::new(format!("· {d}")).small().color(pal.text_weak));
                    }
                });
                ui.horizontal(|ui| {
                    if v.hex.is_none() {
                        let r = ui.add(egui::TextEdit::singleline(&mut v.search).hint_text("🔍  Search in file…").desired_width(260.0));
                        if r.changed() {
                            v.refilter();
                        }
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let n = v.hits.len();
                        let mut step: i32 = 0;
                        if ui.small_button("▲").on_hover_text("Previous match").clicked() {
                            step = -1;
                        }
                        if ui.small_button("▼").on_hover_text("Next match (Enter)").clicked() || enter {
                            step = 1;
                        }
                        if enter {
                            r.request_focus();
                        }
                        if n > 0 && step != 0 {
                            v.cur = ((v.cur as i32 + step).rem_euclid(n as i32)) as usize;
                            v.scroll_to = Some(v.hits[v.cur] as usize);
                        }
                        if !v.search.trim().is_empty() {
                            let t = if n == 0 { RichText::new("0 matches").color(pal.sev_err) } else { RichText::new(format!("{}/{}", v.cur + 1, fmt_num(n))).color(pal.text_weak) };
                            ui.label(t);
                        }
                        let mut only = v.only_problems;
                        if ui.toggle_value(&mut only, "⚠ errors & warnings only").changed() {
                            v.only_problems = only;
                            v.refilter();
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("💾 Save…").clicked() {
                            save = true;
                        }
                        if ui.button("↗ Open in Windows").clicked() {
                            external = true;
                        }
                        if v.hex.is_none() && v.kind.tabular() {
                            if ui
                                .add(egui::Button::new(RichText::new("📊 Open as table").strong().color(Color32::WHITE)).fill(pal.accent))
                                .on_hover_text("Show this log in the main table with filters, timeline, search and patterns")
                                .clicked()
                            {
                                as_table = true;
                            }
                        }
                    });
                });
                ui.separator();
                if let Some(hex) = &v.hex {
                    ui.label(RichText::new("Binary file – first 64 KB as hex. Use “Open in Windows” to view it with its own program.").color(pal.text_weak));
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(hex.as_str()).monospace()).extend());
                    });
                    return;
                }
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let row_h = ui.fonts(|f| f.row_height(&font)) + 3.0;
                let q = v.search.trim().to_ascii_lowercase();
                let n = v.visible.len();
                let digits = v.lines.len().to_string().len();
                let mut sa = egui::ScrollArea::both().auto_shrink([false, false]);
                if let Some(r) = v.scroll_to.take() {
                    sa = sa.vertical_scroll_offset((r as f32 * row_h - 3.0 * row_h).max(0.0));
                }
                let cur_line = v.hits.get(v.cur).map(|&h| h as usize);
                sa.show_rows(ui, row_h, n, |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for vi in range {
                        let li = v.visible[vi] as usize;
                        let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width().max(2000.0), row_h));
                        let bg = if Some(vi) == cur_line {
                            Some(pal.row_sel)
                        } else {
                            match v.levels[li] {
                                2 => Some(pal.row_err),
                                1 => Some(pal.row_warn),
                                _ => None,
                            }
                        };
                        if let Some(c) = bg {
                            ui.painter().rect_filled(rect, 0.0, c);
                        }
                        let (a, b) = v.lines[li];
                        let mut end = (b as usize).min(a as usize + 5000);
                        while !v.text.is_char_boundary(end) {
                            end -= 1;
                        }
                        let s = &v.text[a as usize..end];
                        let lo = &v.lower[a as usize..end];
                        ui.horizontal(|ui| {
                            ui.set_height(row_h);
                            ui.label(RichText::new(format!("{:>w$} ", li + 1, w = digits)).monospace().color(pal.text_weak));
                            let job = highlight_line(s, lo, &q, &pal, font.clone());
                            ui.add(egui::Label::new(job).extend().selectable(true));
                        });
                    }
                });
            });
        let entry = v.entry.clone();
        if !open {
            self.viewer = None;
        }
        if as_table {
            self.viewer = None;
            self.open_entry_table(ctx, entry);
        } else if external {
            self.entry_external(ctx, &entry);
        } else if save {
            self.entry_save(ctx, &entry);
        }
    }

    // ------------------------------------------------------------ system info

    pub(super) fn sysinfo_window(&mut self, ctx: &egui::Context) {
        if !self.show_sysinfo {
            return;
        }
        let pal = self.pal;
        let Some(log) = &self.log else { return };
        let Some(root) = &log.sysinfo else { return };
        let mut open = true;
        egui::Window::new(RichText::new("ℹ  System information").strong())
            .open(&mut open)
            .default_size([720.0, 600.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let i = &log.info;
                egui::Grid::new("sys_head").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("Product", format!("{} {}", i.family, if i.model != i.family { i.model.as_str() } else { "" })),
                        ("Version", format!("{}  {}", i.version, if i.technical_version.is_empty() { String::new() } else { format!("(technical {})", i.technical_version) })),
                        ("Serial number", i.serial.clone()),
                        ("Product ID", i.product_id.clone()),
                        ("Modality", i.modality.clone()),
                        ("Hospital", i.hospital.clone()),
                        ("Department / room", [i.department.as_str(), i.room.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" / ")),
                        ("City", [i.city.as_str(), i.country.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ")),
                        ("Log created", i.creation.clone()),
                    ] {
                        if v.trim().is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(k).color(pal.text_weak));
                        ui.add(egui::Label::new(RichText::new(v.trim()).strong()).selectable(true));
                        ui.end_row();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (k, c) in root.children.iter().enumerate() {
                        node_ui(ui, c, 0, k, &pal);
                    }
                });
            });
        if !open {
            self.show_sysinfo = false;
        }
    }
}

fn node_title(n: &XmlNode) -> String {
    let main = ["Name", "Type", "HospitalName", "ProductFamily"].iter().find_map(|k| n.attr(k)).filter(|v| !v.is_empty());
    match main {
        Some(m) => format!("{}  {}", n.name, m),
        None => n.name.clone(),
    }
}

fn node_ui(ui: &mut egui::Ui, n: &XmlNode, depth: usize, idx: usize, pal: &Palette) {
    let attrs: Vec<&(String, String)> = n.attrs.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    if n.children.is_empty() {
        // one line: Name · key: value · ...
        let mut job = egui::text::LayoutJob::default();
        let body = egui::TextStyle::Body.resolve(ui.style());
        let strong = egui::TextFormat { font_id: body.clone(), color: pal.text, ..Default::default() };
        let weak = egui::TextFormat { font_id: body.clone(), color: pal.text_weak, ..Default::default() };
        job.append(&node_title(n), 0.0, strong.clone());
        for (k, v) in attrs {
            if matches!(k.as_str(), "Name" | "Type") {
                continue;
            }
            job.append(&format!("   {k}: "), 0.0, weak.clone());
            job.append(v, 0.0, strong.clone());
        }
        if !n.text.is_empty() {
            job.append(&format!("   {}", n.text), 0.0, weak.clone());
        }
        ui.add(egui::Label::new(job).wrap().selectable(true));
        return;
    }
    egui::CollapsingHeader::new(RichText::new(format!("{}   ({})", node_title(n), n.children.len())).strong())
        .id_salt(("sysnode", depth, idx, &n.name))
        .default_open(depth < 2)
        .show(ui, |ui| {
            if !attrs.is_empty() {
                egui::Grid::new(("sysattrs", depth, idx, &n.name)).num_columns(2).spacing([14.0, 2.0]).show(ui, |ui| {
                    for (k, v) in attrs {
                        ui.label(RichText::new(k).color(pal.text_weak));
                        ui.add(egui::Label::new(v.as_str()).selectable(true));
                        ui.end_row();
                    }
                });
            }
            for (k, c) in n.children.iter().enumerate() {
                node_ui(ui, c, depth + 1, k, pal);
            }
        });
}
