//! Power-On Self Test (POST) reports:
//! `[Detail=!TestName:X;!TestReport:involved FRU's: A code#: … POSTItem: name Done/Passed [Description]: …, [Details]: … ;!Description:POST X Failed;started:… ended:…\nnote;]`
//! Recognized only when the text contains "POSTItem:" plus "!TestName:" or "FRU", so other texts are not affected.

use crate::theme::Palette;
use eframe::egui::{self, Color32, RichText};

#[derive(Debug, Clone, Default)]
pub struct PostItem {
    pub name: String,
    /// e.g. "Done"
    pub state: String,
    /// e.g. "Passed" / "Failed"
    pub result: String,
    pub description: String,
    pub details: String,
}

impl PostItem {
    pub fn failed(&self) -> bool {
        let r = self.result.to_ascii_lowercase();
        r.contains("fail") || r.contains("error") || r == "nok"
    }
    pub fn passed(&self) -> bool {
        let r = self.result.to_ascii_lowercase();
        r.contains("pass") || r == "ok"
    }
}

#[derive(Debug, Clone, Default)]
pub struct PostReport {
    pub test_name: String,
    /// "POST AD7X Failed"
    pub summary: String,
    pub started: String,
    pub ended: String,
    /// (FRU name, part code or hint)
    pub frus: Vec<(String, String)>,
    pub items: Vec<PostItem>,
    pub note: String,
}

impl PostReport {
    pub fn failed(&self) -> bool {
        self.items.iter().any(|i| i.failed()) || self.summary.to_ascii_lowercase().contains("fail")
    }
}

/// "Done/Passed" style token: two capitalized alphabetic words joined by '/'
fn status_token(t: &str) -> Option<(String, String)> {
    let (a, b) = t.split_once('/')?;
    let ok = |w: &str| w.len() >= 2 && w.chars().all(|c| c.is_ascii_alphabetic()) && w.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    (ok(a) && ok(b)).then(|| (a.to_owned(), b.to_owned()))
}

fn parse_item(s: &str) -> PostItem {
    let s = s.trim();
    let mut it = PostItem::default();
    // find the status token
    let mut pos = 0;
    let mut found: Option<(usize, usize, String, String)> = None;
    for tok in s.split(' ') {
        if let Some((a, b)) = status_token(tok.trim()) {
            found = Some((pos, pos + tok.len(), a, b));
            break;
        }
        pos += tok.len() + 1;
    }
    // single-word status ("Skipped", "Passed", "Failed"...)
    if found.is_none() {
        const WORDS: [&str; 10] = ["Passed", "Failed", "Skipped", "Done", "NotDone", "Aborted", "Error", "OK", "NOK", "Warning"];
        let cut = s.find("[Description]:").or_else(|| s.find("[Details]:")).unwrap_or(s.len());
        let mut pos = 0;
        for tok in s.split(' ') {
            if pos >= cut {
                break;
            }
            if WORDS.contains(&tok.trim()) {
                found = Some((pos, pos + tok.len(), String::new(), tok.trim().to_owned()));
            }
            pos += tok.len() + 1;
        }
    }
    let rest = match found {
        Some((st, en, a, b)) => {
            it.name = s[..st].trim().to_owned();
            it.state = a;
            it.result = b;
            s[en.min(s.len())..].trim()
        }
        None => {
            it.name = s.to_owned();
            ""
        }
    };
    if let Some(d) = rest.find("[Description]:") {
        let after = &rest[d + "[Description]:".len()..];
        match after.find("[Details]:") {
            Some(x) => {
                it.description = after[..x].trim().trim_end_matches(',').trim().to_owned();
                it.details = after[x + "[Details]:".len()..].trim().trim_end_matches(';').trim().to_owned();
            }
            None => it.description = after.trim().to_owned(),
        }
    } else if let Some(x) = rest.find("[Details]:") {
        it.description = rest[..x].trim().trim_end_matches(',').trim().to_owned();
        it.details = rest[x + "[Details]:".len()..].trim().to_owned();
    } else {
        it.description = rest.to_owned();
    }
    if it.details.eq_ignore_ascii_case("No details available.") {
        it.details.clear();
    }
    it
}

/// "A code#: c1 B code#: c2 C code#: c3" -> [(A, c1), (B, c2), (C, c3)]
fn parse_frus(s: &str) -> Vec<(String, String)> {
    let pieces: Vec<&str> = s.split("code#:").map(str::trim).collect();
    if pieces.len() < 2 {
        let t = s.trim();
        return if t.is_empty() { vec![] } else { vec![(t.to_owned(), String::new())] };
    }
    // text that every code has in common (e.g. "check the electronic spare-part-finder")
    let tails = &pieces[1..];
    let words: Vec<Vec<&str>> = tails.iter().map(|p| p.split_whitespace().collect()).collect();
    let mut common = 0;
    if words.len() >= 2 {
        'w: loop {
            let Some(w) = words[0].get(common) else { break };
            for ws in &words[1..] {
                if ws.get(common) != Some(w) {
                    break 'w;
                }
            }
            common += 1;
        }
    }
    let mut out = vec![];
    let mut name = pieces[0].to_owned();
    for (k, ws) in words.iter().enumerate() {
        let last = k == words.len() - 1;
        let (code, next): (Vec<&str>, Vec<&str>) = if last {
            (ws.clone(), vec![])
        } else if common > 0 {
            (ws[..common.min(ws.len())].to_vec(), ws[common.min(ws.len())..].to_vec())
        } else {
            // numeric part codes like "4598 012 76551"
            let n = ws.iter().take_while(|w| w.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.')).count().max(1).min(ws.len());
            (ws[..n].to_vec(), ws[n..].to_vec())
        };
        out.push((name.trim().to_owned(), code.join(" ")));
        name = next.join(" ");
    }
    out
}

pub fn parse_post(t: &str) -> Option<PostReport> {
    if !t.contains("POSTItem:") || !(t.contains("!TestName:") || t.contains("FRU")) {
        return None;
    }
    let mut body = t.trim();
    if let Some(b) = body.strip_prefix('[') {
        body = b.strip_suffix(']').unwrap_or(b);
    }
    if let Some(p) = body.find("!TestName:") {
        body = &body[p..];
    }
    let mut r = PostReport::default();
    // trailer ";!Description:...;started:... ended:...\nnote;"
    let (main, trailer) = match body.find(";!Description:") {
        Some(p) => (&body[..p], &body[p + ";!Description:".len()..]),
        None => (body, ""),
    };
    if !trailer.is_empty() {
        let (first, note) = trailer.split_once('\n').unwrap_or((trailer, ""));
        let mut parts = first.split(';');
        r.summary = parts.next().unwrap_or("").trim().to_owned();
        for p in parts {
            let p = p.trim();
            if let Some(x) = p.strip_prefix("started:") {
                match x.split_once("ended:") {
                    Some((a, b)) => {
                        r.started = a.trim().to_owned();
                        r.ended = b.trim().to_owned();
                    }
                    None => r.started = x.trim().to_owned(),
                }
            }
        }
        r.note = note.trim().trim_end_matches(']').trim().trim_end_matches(';').trim().to_owned();
    }
    let mut report = main;
    if let Some(x) = main.strip_prefix("!TestName:") {
        let (name, rest) = x.split_once(';').unwrap_or((x, ""));
        r.test_name = name.trim().to_owned();
        report = rest;
    }
    let report = report.trim().strip_prefix("!TestReport:").unwrap_or(report).trim();
    let mut chunks = report.split("POSTItem:");
    let head = chunks.next().unwrap_or("").trim();
    let fru_text = head.strip_prefix("involved FRU's:").or_else(|| head.strip_prefix("involved FRUs:")).unwrap_or(head);
    r.frus = parse_frus(fru_text);
    for c in chunks {
        if !c.trim().is_empty() {
            r.items.push(parse_item(c));
        }
    }
    if r.items.is_empty() {
        return None;
    }
    Some(r)
}

/// "Actual = 4.370, Lowlim = 1.638, Highlim = 4.650" -> (actual, low, high)
fn limits(details: &str) -> Option<(f64, f64, f64)> {
    let mut a = None;
    let mut lo = None;
    let mut hi = None;
    for p in details.split(',') {
        let (k, v) = p.split_once('=')?;
        let v: f64 = v.trim().parse().ok()?;
        match k.trim().to_ascii_lowercase().as_str() {
            "actual" => a = Some(v),
            "lowlim" | "lowlimit" | "low" | "min" => lo = Some(v),
            "highlim" | "highlimit" | "high" | "max" => hi = Some(v),
            _ => {}
        }
    }
    Some((a?, lo?, hi?))
}

fn limit_bar(ui: &mut egui::Ui, a: f64, lo: f64, hi: f64, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 10.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, pal.panel2);
    let span = (hi - lo).abs().max(1e-9);
    let pad = span * 0.15;
    let (v0, v1) = (lo.min(a) - pad, hi.max(a) + pad);
    let x = |v: f64| rect.left() + ((v - v0) / (v1 - v0)) as f32 * rect.width();
    let ok = a >= lo && a <= hi;
    let band = egui::Rect::from_min_max(egui::pos2(x(lo), rect.top() + 2.0), egui::pos2(x(hi), rect.bottom() - 2.0));
    p.rect_filled(band, 2.0, Color32::from_rgb(0xc8, 0xec, 0xd2));
    let xa = x(a);
    p.line_segment([egui::pos2(xa, rect.top()), egui::pos2(xa, rect.bottom())], egui::Stroke::new(2.5_f32, if ok { Color32::from_rgb(0x2a, 0xa1, 0x5a) } else { pal.sev_err }));
}

fn item_row(ui: &mut egui::Ui, it: &PostItem, pal: &Palette, stripe: bool) {
    let (icon, col) = if it.failed() {
        ("✖", pal.sev_err)
    } else if it.passed() {
        ("✔", Color32::from_rgb(0x2a, 0xa1, 0x5a))
    } else {
        ("○", pal.text_weak)
    };
    let fill = if stripe { pal.panel2 } else { Color32::TRANSPARENT };
    egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(icon).color(col).strong());
            let name_w = (ui.available_width() * 0.42).clamp(140.0, 300.0);
            ui.vertical(|ui| {
                ui.set_width(name_w);
                let mut name = RichText::new(&it.name);
                if it.failed() {
                    name = name.strong().color(pal.sev_err);
                }
                let r = ui.add(egui::Label::new(name).wrap());
                if !it.description.is_empty() {
                    r.on_hover_text(&it.description);
                }
            });
            ui.vertical(|ui| {
                if let Some((a, lo, hi)) = limits(&it.details) {
                    limit_bar(ui, a, lo, hi, pal);
                }
                let d = if it.details.is_empty() && !it.description.is_empty() && it.description.len() < 70 { it.description.as_str() } else { it.details.as_str() };
                if !d.is_empty() {
                    ui.add(egui::Label::new(RichText::new(d).small().color(if it.failed() { pal.text } else { pal.text_weak })).wrap().selectable(true));
                }
            });
        });
    });
}

pub fn post_ui(ui: &mut egui::Ui, id: egui::Id, r: &PostReport, pal: &Palette) {
    let failed: Vec<&PostItem> = r.items.iter().filter(|i| i.failed()).collect();
    let passed = r.items.iter().filter(|i| i.passed()).count();
    let bad = r.failed();
    let (bg, border) = if bad {
        (Color32::from_rgb(0xfd, 0xf0, 0xf0), Color32::from_rgb(0xf0, 0xb4, 0xb4))
    } else {
        (Color32::from_rgb(0xee, 0xf9, 0xf1), Color32::from_rgb(0xb5, 0xe0, 0xc2))
    };
    egui::Frame::new().fill(bg).stroke(egui::Stroke::new(1.0_f32, border)).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("🔧  Power-On Self Test").small().strong().color(if bad { pal.sev_err } else { Color32::from_rgb(0x2a, 0xa1, 0x5a) }));
        let title = if r.summary.is_empty() { format!("POST {}", r.test_name) } else { r.summary.clone() };
        ui.add(egui::Label::new(RichText::new(title).size(15.0).strong()).wrap().selectable(true));
        let other = r.items.len() - failed.len() - passed;
        let mut info = format!("{} failed · {} passed", failed.len(), passed);
        if other > 0 {
            info.push_str(&format!(" · {other} skipped / other"));
        }
        info.push_str(&format!(" · {} test items", r.items.len()));
        if !r.started.is_empty() {
            info.push_str(&format!(" · started {}", r.started));
        }
        if !r.ended.is_empty() && r.ended != r.started {
            info.push_str(&format!(" · ended {}", r.ended));
        }
        ui.label(RichText::new(info).small().color(pal.text_weak));
        if !r.note.is_empty() {
            ui.label(RichText::new(&r.note).small().italics().color(pal.text_weak));
        }
        for it in &failed {
            ui.add_space(4.0);
            egui::Frame::new().fill(Color32::WHITE).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let st = if it.state.is_empty() { it.result.clone() } else { format!("{}/{}", it.state, it.result) };
                ui.label(RichText::new(format!("✖  {}   {}", it.name, st)).strong().color(pal.sev_err));
                if !it.description.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&it.description).color(pal.text_weak)).wrap().selectable(true));
                }
                if !it.details.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&it.details).monospace()).wrap().selectable(true));
                }
            });
        }
    });
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new(format!("Test items ({})", r.items.len())).strong())
        .id_salt(id.with("items"))
        .default_open(!bad || r.items.len() <= 8)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, it) in r.items.iter().enumerate() {
                item_row(ui, it, pal, k % 2 == 1);
            }
        });
    if !r.frus.is_empty() {
        egui::CollapsingHeader::new(RichText::new(format!("Involved FRUs ({})", r.frus.len())).strong()).id_salt(id.with("frus")).default_open(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, (n, c)) in r.frus.iter().enumerate() {
                let fill = if k % 2 == 1 { pal.panel2 } else { Color32::TRANSPARENT };
                egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        let w = (ui.available_width() * 0.55).clamp(160.0, 340.0);
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            ui.add(egui::Label::new(n.as_str()).wrap().selectable(true));
                        });
                        let c = if c.eq_ignore_ascii_case("check the electronic spare-part-finder") { "see spare-part finder" } else { c.as_str() };
                        ui.add(egui::Label::new(RichText::new(c).monospace().color(pal.text_weak)).wrap().selectable(true));
                    });
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AD7X: &str = "[Detail=!TestName:AD7X;!TestReport:involved FRU's: Lateral Brake Assy code#: check the electronic spare-part-finder AD7X Node Interface Unit code#: check the electronic spare-part-finder Tilt Motor code#: check the electronic spare-part-finder POSTItem: Longitudinal clutch feedback test Done/Failed [Description]: Tests the clutch feedback, [Details]: InitialTest: REQ = 1 FB = 1 (dT 200ms) IFB = 1 (dT 200ms) ISenseFB = 0 (Timeout 200ms) POSTItem: Height drive memory access test Done/Passed [Description]: Tests whether the data memory of the drive can be accessed correctly, [Details]: No details available. POSTItem: Pivot position potmeter test Done/Passed [Description]: Tests the position potmeter, [Details]: Actual = 1.719, Lowlim = 0.250, Highlim = 4.700 ;!Description:POST AD7X Failed;started:9/5/2025 11:10:04 AM ended:9/5/2025 11:10:04 AM\nPOST for AD7X;]";

    #[test]
    fn ad7x() {
        let r = parse_post(AD7X).unwrap();
        assert_eq!(r.test_name, "AD7X");
        assert_eq!(r.summary, "POST AD7X Failed");
        assert_eq!(r.started, "9/5/2025 11:10:04 AM");
        assert_eq!(r.note, "POST for AD7X");
        assert_eq!(r.frus.len(), 3);
        assert_eq!(r.frus[1].0, "AD7X Node Interface Unit");
        assert_eq!(r.frus[2].1, "check the electronic spare-part-finder");
        assert_eq!(r.items.len(), 3);
        assert!(r.items[0].failed());
        assert_eq!(r.items[0].name, "Longitudinal clutch feedback test");
        assert!(r.items[0].details.starts_with("InitialTest: REQ = 1"));
        assert_eq!(r.items[1].details, "");
        assert_eq!(limits(&r.items[2].details), Some((1.719, 0.25, 4.7)));
    }

    #[test]
    fn variants() {
        let r = parse_post("[Detail=!TestName:Review Module;!TestReport:involved FRU's: Review Module code#: 4598 012 76551 POSTItem: Review Module Done/Passed  ;!Description:POST Review Module Passed;started:9/5/2025 11:08:50 AM ended:9/5/2025 11:08:51 AM\nPower On Self Test for Review module in control room;]").unwrap();
        assert_eq!(r.frus, vec![("Review Module".to_owned(), "4598 012 76551".to_owned())]);
        assert!(r.items[0].passed() && !r.failed());
        let r = parse_post("[Detail=!TestName:PC Hardware;!TestReport:involved FRU's: SuitePC code#: check the electronic spare-part-finder POSTItem: PC Hardware version Done/Passed Tests the PC hardware version POSTItem: CAN Board test Done/Passed Test the CAN Board ;!Description:POST PC Hardware Passed;started:x ended:y\nPOST for PC Hardware;]").unwrap();
        assert_eq!(r.items[1].name, "CAN Board test");
        assert_eq!(r.items[1].description, "Test the CAN Board");
        assert!(parse_post("[CHANNEL_INFO=X-Ray Channel Biplane] [Info=UIActivity Execute()]").is_none());
    }
}

#[cfg(test)]
mod real {
    #[test]
    fn real_post_messages() {
        let Ok(p) = std::env::var("POSTLOG") else { return };
        let log = crate::parser::load(std::path::Path::new(&p), crate::parser::Progress::new()).unwrap();
        let mut n = 0;
        for e in &log.events {
            if !e.message.contains("POSTItem:") {
                continue;
            }
            let t = crate::formatter::unescape(&e.message);
            let r = super::parse_post(&t).expect("not parsed");
            n += 1;
            assert_eq!(r.items.len(), t.matches("POSTItem:").count());
            let fails = t.matches("/Failed").count();
            assert_eq!(r.items.iter().filter(|i| i.failed()).count(), fails, "{}", e.index);
            assert!(r.items.iter().all(|i| !i.result.is_empty()), "{} {:?}", e.index, r.items.iter().find(|i| i.result.is_empty()));
            if e.index == 3586 || e.index == 4020 {
                println!("{} {} | {} FRUs, first {:?}, last {:?} | {} items | note {:?}", e.index, r.summary, r.frus.len(), r.frus.first(), r.frus.last(), r.items.len(), r.note);
            }
        }
        println!("{n} POST reports ok");
    }
}
