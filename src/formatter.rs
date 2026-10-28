//! "Formatted" view of description / additional info:
//! escape sequences, key=value groups as tables, exception & stack trace analysis.

use crate::theme::Palette;
use eframe::egui::{self, Color32, RichText};

// ------------------------------------------------------------------ model

#[derive(Debug, Clone)]
pub struct KvRow {
    pub key: String,
    pub value: String,
    pub header: bool,
    pub depth: u8,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub full: String,
    pub short: String,
    pub namespace: String,
    pub location: String,
    pub app: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ExcEntry {
    /// exception type, e.g. System.Data.SqlClient.SqlException ("" when unknown)
    pub kind: String,
    pub message: String,
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
pub struct ExceptionInfo {
    pub lang: &'static str,
    pub headline: String,
    pub chain: Vec<ExcEntry>,
    pub caller: Vec<(String, String)>,
    pub hint: Option<&'static str>,
}

impl ExceptionInfo {
    /// innermost exception that has a type (the root cause)
    pub fn root(&self) -> Option<&ExcEntry> {
        self.chain.iter().rev().find(|e| !e.kind.is_empty()).or(self.chain.first())
    }
}

#[derive(Debug, Clone)]
pub enum Block {
    Para(String),
    Table(Vec<KvRow>),
    Exception(ExceptionInfo),
    Post(crate::post::PostReport),
}

// ------------------------------------------------------------------ helpers

/// Turns literal escape sequences (\n, \r\n, \t, \\) into real characters.
pub fn unescape(s: &str) -> String {
    // control characters (except line breaks / tabs) -> space
    let cleaned: String;
    let s = if s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t') {
        cleaned = s.chars().map(|c| if c.is_control() && c != '\n' && c != '\r' && c != '\t' { ' ' } else { c }).collect();
        cleaned.as_str()
    } else {
        s
    };
    if !s.contains('\\') {
        return s.replace("\r\n", "\n");
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.peek() {
                Some('n') => {
                    it.next();
                    out.push('\n');
                }
                Some('r') => {
                    it.next();
                    if it.peek() == Some(&'\\') {
                        // \r\n
                        let mut clone = it.clone();
                        clone.next();
                        if clone.peek() == Some(&'n') {
                            it.next();
                            it.next();
                        }
                    }
                    out.push('\n');
                }
                Some('t') => {
                    it.next();
                    out.push_str("    ");
                }
                Some('\\') => {
                    it.next();
                    out.push('\\');
                }
                _ => out.push('\\'),
            }
        } else if c == '\r' {
            if it.peek() != Some(&'\n') {
                out.push('\n');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True when the formatted view would differ from the plain text.
#[allow(dead_code)]
pub fn worth_formatting(s: &str) -> bool {
    s.contains('\n') || s.contains("\\n") || s.contains('=') || s.contains('{') || s.contains('[') || s.contains('*') || s.contains(';')
}

const FRAMEWORK_PREFIXES: &[&str] = &["System.", "Microsoft.", "MS.", "Mono.", "java.", "javax.", "sun.", "jdk.", "com.sun.", "org.apache.", "Newtonsoft."];

fn parse_frame(line: &str) -> Option<Frame> {
    let t = line.trim();
    let rest = t.strip_prefix("at ")?;
    let paren = rest.find('(')?;
    let qual = rest[..paren].trim();
    if qual.is_empty() || qual.contains(' ') {
        return None;
    }
    let parts: Vec<&str> = qual.split('.').collect();
    let (ns, short) = if parts.len() >= 2 {
        (parts[..parts.len() - 2].join("."), parts[parts.len() - 2..].join("."))
    } else {
        (String::new(), qual.to_owned())
    };
    // java location "(File.java:123)"
    let inner = &rest[paren + 1..];
    let close = inner.find(')').unwrap_or(inner.len());
    let args = &inner[..close];
    let location = if args.contains(".java") || args.contains(".kt") || args == "Native Method" || args == "Unknown Source" {
        args.to_owned()
    } else {
        String::new()
    };
    let app = !FRAMEWORK_PREFIXES.iter().any(|p| qual.starts_with(p)) && !qual.starts_with("cpp.");
    Some(Frame { full: t.to_owned(), short, namespace: ns, location, app })
}

/// "Name: x, Other: y" -> pairs (None when the text isn't such a list)
fn colon_list(s: &str) -> Option<Vec<(String, String)>> {
    let parts: Vec<&str> = s.split(", ").collect();
    if parts.len() < 2 {
        return None;
    }
    let mut out = vec![];
    for p in parts {
        let (k, v) = p.split_once(": ")?;
        if k.trim().is_empty() || k.len() > 40 {
            return None;
        }
        out.push((k.trim().to_owned(), v.trim().to_owned()));
    }
    Some(out)
}

const HINTS: &[(&str, &str)] = &[
    (
        "converting identity to data type int",
        "SQL Server: an IDENTITY (auto-increment) column of a database table has reached the maximum value of type int (2,147,483,647). New rows cannot be inserted into that table until it is cleaned up / reseeded or the database is re-initialized.",
    ),
    ("deadlock victim", "SQL Server chose this transaction as a deadlock victim. Usually transient – the operation can be retried."),
    ("a network-related or instance-specific error", "SQL Server could not be reached (service not running, wrong instance name or network problem)."),
    ("timeout expired", "A database or network operation timed out."),
    ("there is not enough space on the disk", "The disk is full."),
    ("outofmemory", "The process ran out of memory."),
    ("the user name or password is incorrect", "Windows logon failure for the account used to start the process (wrong, changed or expired password)."),
    ("nullreferenceexception", "Software error: an object reference was null."),
    ("nullpointerexception", "Software error: an object reference was null."),
    ("unauthorizedaccessexception", "Access denied – missing file system or registry permissions."),
    ("access to the path", "Access denied – missing file system permissions."),
    ("filenotfoundexception", "A required file is missing."),
    ("could not find file", "A required file is missing."),
    ("socketexception", "Network connection problem (host unreachable, port closed or connection reset)."),
    ("connection refused", "The target service is not listening (not started or blocked)."),
];

fn find_hint(t: &str) -> Option<&'static str> {
    let l = t.to_lowercase();
    HINTS.iter().find(|(n, _)| l.contains(n)).map(|(_, h)| *h)
}

fn java_exception_line(t: &str) -> Option<(String, String)> {
    let t = t.trim();
    let t = t.strip_prefix("Caused by:").map(|x| x.trim()).unwrap_or(t);
    let t = if let Some(p) = t.find("Exception in thread") {
        // Exception in thread "main" java.lang.X: msg
        let r = &t[p..];
        let q = r.rfind('"').map(|i| &r[i + 1..]).unwrap_or(r);
        q.trim()
    } else {
        t
    };
    let (ty, msg) = match t.split_once(": ") {
        Some((a, b)) => (a, b),
        None => (t, ""),
    };
    let ok = ty.contains('.')
        && !ty.contains(' ')
        && (ty.ends_with("Exception") || ty.ends_with("Error") || ty.ends_with("Throwable") || ty.ends_with("Fault"));
    ok.then(|| (ty.to_owned(), msg.to_owned()))
}

pub fn parse_exception(t: &str) -> Option<ExceptionInfo> {
    let lines: Vec<&str> = t.lines().collect();
    let frame_count = lines.iter().filter(|l| parse_frame(l).is_some()).count();
    let has_marker = t.contains("Exception Details:") || t.contains("Caused by:") || t.contains("CallStack:");
    if frame_count < 2 && !(has_marker && frame_count >= 1) {
        return None;
    }
    let java = lines.iter().any(|l| l.contains(".java:")) || t.contains("Caused by:");
    let mut chain: Vec<ExcEntry> = vec![ExcEntry::default()];
    let mut caller = vec![];
    let mut in_msg = true;
    for raw in &lines {
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(f) = parse_frame(l) {
            chain.last_mut().unwrap().frames.push(f);
            in_msg = false;
            continue;
        }
        if l.starts_with("CallStack:") || l.starts_with("StackTrace:") || l == "No callstack information available." || l.starts_with("--- End of") {
            in_msg = false;
            continue;
        }
        if let Some(rest) = l.strip_prefix("Caller Information") {
            let inner = rest.trim().trim_start_matches('[').trim_end_matches(']');
            if let Some(kv) = colon_list(inner) {
                caller = kv;
            }
            in_msg = false;
            continue;
        }
        let det = l.strip_prefix("Inner Exception Details:").or_else(|| l.strip_prefix("Exception Details:"));
        if let Some(d) = det {
            let d = d.trim();
            let (kind, msg) = if d.starts_with('[') {
                match d.find("]:") {
                    Some(i) => (d[1..i].to_owned(), d[i + 2..].trim().to_owned()),
                    None => (d.trim_matches(&['[', ']'][..]).to_owned(), String::new()),
                }
            } else {
                (String::new(), d.to_owned())
            };
            chain.push(ExcEntry { kind, message: msg, frames: vec![] });
            in_msg = true;
            continue;
        }
        if let Some((kind, msg)) = java_exception_line(l) {
            let cur = chain.last_mut().unwrap();
            if cur.kind.is_empty() && cur.frames.is_empty() && cur.message.is_empty() {
                cur.kind = kind;
                cur.message = msg;
            } else {
                chain.push(ExcEntry { kind, message: msg, frames: vec![] });
            }
            in_msg = true;
            continue;
        }
        let cur = chain.last_mut().unwrap();
        if in_msg || cur.frames.is_empty() {
            if !cur.message.is_empty() {
                cur.message.push('\n');
            }
            cur.message.push_str(l);
            in_msg = true;
        } else {
            // text after a stack: a new (untyped) section
            chain.push(ExcEntry { kind: String::new(), message: l.to_owned(), frames: vec![] });
            in_msg = true;
        }
    }
    let headline = chain
        .iter()
        .find(|e| !e.message.is_empty())
        .map(|e| e.message.lines().next().unwrap_or("").to_owned())
        .unwrap_or_default();
    // drop untyped, frameless repeats of the headline
    let first_msg = chain.first().map(|e| e.message.clone()).unwrap_or_default();
    let mut seen_first = false;
    chain.retain(|e| {
        if e.kind.is_empty() && e.frames.is_empty() {
            if e.message == first_msg && seen_first {
                return false;
            }
            if e.message.is_empty() {
                return false;
            }
        }
        if e.message == first_msg {
            seen_first = true;
        }
        true
    });
    Some(ExceptionInfo { lang: if java { "Java" } else { ".NET" }, headline, chain, caller, hint: find_hint(t) })
}

// ---------- key/value groups

enum Seg {
    Text(String),
    Group(String),
}

fn closer(c: u8) -> Option<u8> {
    match c {
        b'{' => Some(b'}'),
        b'[' => Some(b']'),
        _ => None,
    }
}

/// splits text into plain parts and top-level {...} / [...] groups
fn scan(t: &str) -> Vec<Seg> {
    let b = t.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    let mut text_start = 0;
    while i < b.len() {
        if let Some(_) = closer(b[i]) {
            // find matching close
            let mut stack = vec![closer(b[i]).unwrap()];
            let mut j = i + 1;
            while j < b.len() && !stack.is_empty() {
                if let Some(c) = closer(b[j]) {
                    stack.push(c);
                } else if b[j] == *stack.last().unwrap() {
                    stack.pop();
                } else if b[j] == b'}' || b[j] == b']' {
                    break; // mismatched
                }
                j += 1;
            }
            if stack.is_empty() {
                if i > text_start {
                    out.push(Seg::Text(t[text_start..i].to_owned()));
                }
                out.push(Seg::Group(t[i + 1..j - 1].to_owned()));
                i = j;
                text_start = j;
                continue;
            }
        }
        i += 1;
    }
    if text_start < b.len() {
        out.push(Seg::Text(t[text_start..].to_owned()));
    }
    out
}

fn top_level_eq(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.bytes().enumerate() {
        match c {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// splits at the separators that are not inside (), [] or {}
fn split_top<'a>(s: &'a str, seps: &[u8]) -> Vec<&'a str> {
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.bytes().enumerate() {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            c if depth == 0 && seps.contains(&c) => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

/// first '=' that is not inside (), [] or {}
fn eq_top(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.bytes().enumerate() {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// "a=1; b=2, c=3" (also "a=1 b=2") -> rows. Returns false when it is not such a list.
fn kv_list_rows(s: &str, depth: u8, rows: &mut Vec<KvRow>) -> bool {
    let parts = split_top(s, b";,");
    if parts.len() < 2 {
        return false;
    }
    let with_eq = parts.iter().filter(|p| eq_top(p).is_some()).count();
    if eq_top(parts[0]).is_none() || with_eq * 10 < parts.len() * 8 {
        return false;
    }
    for p in parts {
        kv_part(p, depth, rows);
    }
    true
}

/// one "key=value"; the value may be "name(a=1; b=2)", "[...]" or "x=1 y=2"
fn kv_part(p: &str, depth: u8, rows: &mut Vec<KvRow>) {
    let Some(eq) = eq_top(p) else {
        rows.push(KvRow { key: String::new(), value: p.trim().to_owned(), header: false, depth });
        return;
    };
    let k = p[..eq].trim();
    let v = p[eq + 1..].trim();
    // "actPL=891.59 setP=891.59": several key=value separated by spaces
    let toks: Vec<&str> = split_top(p, b" \t");
    if toks.len() >= 2 && toks.iter().all(|t| eq_top(t).is_some_and(|i| i > 0 && i + 1 < t.len())) && !k.contains(' ') {
        for t in toks {
            kv_part(t, depth, rows);
        }
        return;
    }
    // value "eds(DRV=3; DRVR=0; ...)"
    if let (Some(o), true) = (v.find('('), v.ends_with(')')) {
        let inner = &v[o + 1..v.len() - 1];
        if eq_top(inner).is_none() && inner.contains('=') || split_top(inner, b";,").len() >= 2 {
            let mut sub = vec![];
            if kv_list_rows(inner, depth + 1, &mut sub) {
                rows.push(KvRow { key: k.to_owned(), value: v[..o].trim().to_owned(), header: true, depth });
                rows.append(&mut sub);
                return;
            }
        }
    }
    // value is a [..] / {..} group or itself a list
    if v.starts_with('[') || v.starts_with('{') {
        let mut sub = vec![];
        group_rows(v, depth + 1, &mut sub);
        if sub.iter().any(|r| !r.key.is_empty()) {
            rows.push(KvRow { key: k.to_owned(), value: String::new(), header: true, depth });
            rows.append(&mut sub);
            return;
        }
    }
    let mut sub = vec![];
    if kv_list_rows(v, depth + 1, &mut sub) {
        rows.push(KvRow { key: k.to_owned(), value: String::new(), header: true, depth });
        rows.append(&mut sub);
        return;
    }
    rows.push(KvRow { key: k.to_owned(), value: v.to_owned(), header: false, depth });
}

fn group_rows(inner: &str, depth: u8, rows: &mut Vec<KvRow>) {
    let it = inner.trim();
    if it.is_empty() {
        return;
    }
    if it.starts_with('[') || it.starts_with('{') {
        // nested list like [[a=b] , [c=d]]
        let segs = scan(it);
        if segs.iter().any(|s| matches!(s, Seg::Group(_))) {
            for s in segs {
                match s {
                    Seg::Group(g) => group_rows(&g, depth, rows),
                    Seg::Text(t) => {
                        let t = t.trim().trim_matches(&[',', ';', '|'][..]).trim();
                        if !t.is_empty() {
                            rows.push(KvRow { key: String::new(), value: t.to_owned(), header: false, depth });
                        }
                    }
                }
            }
            return;
        }
    }
    if let Some(eq) = top_level_eq(it) {
        let k = it[..eq].trim().to_owned();
        let v = it[eq + 1..].trim();
        let segs = scan(v);
        let nested = segs.iter().any(|s| matches!(s, Seg::Group(g) if top_level_eq(g).is_some()));
        if nested {
            rows.push(KvRow { key: k, value: String::new(), header: true, depth });
            for s in segs {
                match s {
                    Seg::Group(g) => group_rows(&g, depth + 1, rows),
                    Seg::Text(t) => {
                        let t = t.trim().trim_matches(&[',', ';', '|'][..]).trim();
                        if !t.is_empty() {
                            rows.push(KvRow { key: String::new(), value: t.to_owned(), header: false, depth: depth + 1 });
                        }
                    }
                }
            }
        } else {
            // "Configuration=A = 1; B = 2" -> header + list;  "A=1; B=2" -> flat list
            let mut sub = vec![];
            if kv_list_rows(v, depth + 1, &mut sub) {
                rows.push(KvRow { key: k, value: String::new(), header: true, depth });
                rows.append(&mut sub);
            } else if !kv_list_rows(it, depth, rows) {
                kv_part(it, depth, rows);
            }
        }
        return;
    }
    if let Some(kv) = colon_list(it) {
        for (k, v) in kv {
            rows.push(KvRow { key: k, value: v, header: false, depth });
        }
        return;
    }
    rows.push(KvRow { key: it.to_owned(), value: String::new(), header: true, depth });
}

/// "a: 1; b: 2; c: 3" or "a=1; b=2" -> rows
fn semicolon_rows(t: &str) -> Option<Vec<KvRow>> {
    let parts: Vec<&str> = t.split(';').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    let mut rows = vec![];
    for p in parts {
        let (k, v) = p.split_once('=').or_else(|| p.split_once(':'))?;
        rows.push(KvRow { key: k.trim().to_owned(), value: v.trim().to_owned(), header: false, depth: 0 });
    }
    Some(rows)
}

pub fn analyze(text: &str) -> Vec<Block> {
    let t = unescape(text);
    if let Some(p) = crate::post::parse_post(&t) {
        return vec![Block::Post(p)];
    }
    if let Some(ex) = parse_exception(&t) {
        return vec![Block::Exception(ex)];
    }
    let mut blocks: Vec<Block> = vec![];
    let push_rows = |blocks: &mut Vec<Block>, mut rows: Vec<KvRow>| {
        if let Some(Block::Table(r)) = blocks.last_mut() {
            r.append(&mut rows);
        } else {
            blocks.push(Block::Table(rows));
        }
    };
    let mut any_rows = false;
    for seg in scan(&t) {
        match seg {
            Seg::Group(g) => {
                let mut rows = vec![];
                group_rows(&g, 0, &mut rows);
                if rows.iter().any(|r| !r.header) || rows.len() == 1 {
                    any_rows = true;
                    push_rows(&mut blocks, rows);
                }
            }
            Seg::Text(s) => {
                let trimmed = s.trim().trim_matches(&[',', ';', '|'][..]).trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(rows) = semicolon_rows(trimmed) {
                    any_rows = true;
                    push_rows(&mut blocks, rows);
                } else {
                    blocks.push(Block::Para(trimmed.to_owned()));
                }
            }
        }
    }
    if !any_rows {
        return vec![Block::Para(t.trim().to_owned())];
    }
    blocks
}

// ------------------------------------------------------------------ UI

/// Text with *emphasis* rendered bold.
pub fn emph_job(ui: &egui::Ui, s: &str, base: Color32, strong: Color32) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat { font_id: font.clone(), color: base, ..Default::default() };
    let bold = egui::TextFormat { font_id: egui::FontId::new(font.size, font.family.clone()), color: strong, underline: egui::Stroke::NONE, ..Default::default() };
    if s.matches('*').count() >= 2 && s.matches('*').count() % 2 == 0 {
        for (i, part) in s.split('*').enumerate() {
            if part.is_empty() {
                continue;
            }
            job.append(part, 0.0, if i % 2 == 1 { bold.clone() } else { normal.clone() });
        }
    } else {
        job.append(s, 0.0, normal);
    }
    job.wrap.max_width = ui.available_width();
    job
}

fn para(ui: &mut egui::Ui, s: &str, pal: &Palette) {
    let job = emph_job(ui, s, pal.text, pal.accent);
    ui.add(egui::Label::new(job).wrap().selectable(true));
}

fn kv_table(ui: &mut egui::Ui, id: egui::Id, rows: &[KvRow], pal: &Palette) {
    egui::Frame::new().stroke(egui::Stroke::new(1.0_f32, pal.border)).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
        egui::Grid::new(id).striped(true).num_columns(2).spacing([16.0, 3.0]).min_col_width(80.0).show(ui, |ui| {
            for r in rows {
                let indent = "    ".repeat(r.depth as usize);
                if r.header {
                    ui.label(RichText::new(format!("{indent}{}", r.key)).strong().color(pal.text));
                    ui.label(RichText::new(r.value.as_str()).strong().color(pal.text_weak));
                } else {
                    ui.add(egui::Label::new(RichText::new(format!("{indent}{}", r.key)).color(pal.text_weak)).selectable(true));
                    let job = emph_job(ui, &r.value, pal.text, pal.accent);
                    ui.add(egui::Label::new(job).wrap().selectable(true));
                }
                ui.end_row();
            }
        });
    });
}

fn short_type(t: &str) -> &str {
    t.rsplit('.').next().unwrap_or(t)
}

fn frames_ui(ui: &mut egui::Ui, frames: &[Frame], pal: &Palette) {
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let small = egui::FontId::proportional(11.0);
    for f in frames {
        // one truncated line per frame so long names never widen the panel
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let col = if f.app { pal.text } else { pal.text_weak };
        let mut fid = mono.clone();
        if f.app {
            fid.size += 0.5;
        }
        job.append(&f.short, 0.0, egui::TextFormat { font_id: fid, color: col, ..Default::default() });
        if !f.location.is_empty() {
            job.append(&f.location, 8.0, egui::TextFormat { font_id: small.clone(), color: pal.text_weak, ..Default::default() });
        }
        if !f.namespace.is_empty() {
            job.append(&f.namespace, 8.0, egui::TextFormat { font_id: small.clone(), color: pal.text_weak, ..Default::default() });
        }
        ui.add(egui::Label::new(job).truncate().selectable(true)).on_hover_text(&f.full);
    }
}

fn exception_ui(ui: &mut egui::Ui, id: egui::Id, ex: &ExceptionInfo, full: &str, pal: &Palette) {
    let red = pal.sev_err;
    egui::Frame::new()
        .fill(Color32::from_rgb(0xfd, 0xf0, 0xf0))
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(0xf0, 0xb4, 0xb4)))
        .corner_radius(8.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("⚠  {} exception", ex.lang)).small().strong().color(red));
            let generic = ex.headline.trim().trim_matches('*').trim().ends_with(':') || ex.headline.trim().len() < 3;
            let headline = match ex.root() {
                Some(r) if generic && !r.kind.is_empty() => {
                    let m = r.message.lines().next().unwrap_or("").trim();
                    if m.is_empty() { short_type(&r.kind).to_owned() } else { format!("{}: {}", short_type(&r.kind), m) }
                }
                _ => ex.headline.clone(),
            };
            ui.add(egui::Label::new(RichText::new(headline).size(15.0).strong().color(pal.text)).wrap().selectable(true));
            if let Some(h) = ex.hint {
                ui.add_space(4.0);
                egui::Frame::new().fill(Color32::from_rgb(0xff, 0xf6, 0xd6)).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(format!("ℹ  {h}")).color(pal.text)).wrap());
                });
            }
            ui.add_space(4.0);
            let root = ex.root();
            egui::Grid::new(id.with("sum")).num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                if let Some(r) = root {
                    if !r.kind.is_empty() {
                        ui.label(RichText::new("Root cause").color(pal.text_weak));
                        ui.add(egui::Label::new(RichText::new(short_type(&r.kind)).strong().color(red)).selectable(true)).on_hover_text(&r.kind);
                        ui.end_row();
                    }
                    if let Some(f) = r.frames.first() {
                        ui.label(RichText::new("Thrown in").color(pal.text_weak));
                        ui.add(egui::Label::new(RichText::new(&f.short).monospace()).selectable(true)).on_hover_text(&f.full);
                        ui.end_row();
                    }
                    let app = r.frames.iter().find(|f| f.app).or_else(|| ex.chain.iter().flat_map(|e| e.frames.iter()).find(|f| f.app));
                    if let Some(f) = app {
                        ui.label(RichText::new("First app frame").color(pal.text_weak));
                        ui.add(egui::Label::new(RichText::new(&f.short).monospace().strong()).selectable(true)).on_hover_text(&f.full);
                        ui.end_row();
                    }
                }
                {
                    let names: Vec<&str> = ex.chain.iter().filter(|e| !e.kind.is_empty()).map(|e| short_type(&e.kind)).collect();
                    if names.len() > 1 {
                        ui.label(RichText::new("Chain").color(pal.text_weak));
                        ui.label(names.join("  →  "));
                        ui.end_row();
                    }
                }
                for (k, v) in &ex.caller {
                    ui.label(RichText::new(k).color(pal.text_weak));
                    ui.label(v);
                    ui.end_row();
                }
            });
        });
    ui.add_space(4.0);
    let shown: Vec<&ExcEntry> = ex.chain.iter().filter(|e| !e.kind.is_empty() || !e.frames.is_empty()).collect();
    let n = shown.len();
    for (i, e) in shown.into_iter().enumerate() {
        let title = if e.kind.is_empty() {
            format!("{}. {}", i + 1, e.message.lines().next().unwrap_or("(message)"))
        } else {
            match e.message.lines().next().map(str::trim).filter(|m| !m.is_empty()) {
                Some(m) => format!("{}. {} — {}", i + 1, short_type(&e.kind), m),
                None => format!("{}. {}", i + 1, short_type(&e.kind)),
            }
        };
        let is_root = std::ptr::eq(e, ex.root().unwrap());
        egui::CollapsingHeader::new(RichText::new(format!("{}   ({} frames)", title, e.frames.len())).color(if is_root { red } else { pal.text }))
            .id_salt(id.with(("exc", i)))
            .default_open(is_root || n == 1)
            .show(ui, |ui| {
                if !e.kind.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&e.kind).monospace().small().color(pal.text_weak)).selectable(true));
                }
                if e.message.lines().count() > 1 {
                    ui.add(egui::Label::new(&e.message).wrap().selectable(true));
                }
                frames_ui(ui, &e.frames, pal);
            });
    }
    egui::CollapsingHeader::new(RichText::new("Full text").color(pal.text_weak)).id_salt(id.with("full")).show(ui, |ui| {
        let mut s: &str = full;
        ui.add(egui::TextEdit::multiline(&mut s).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
    });
}

/// Shows text either raw (exactly as in the log) or formatted.
pub fn show(ui: &mut egui::Ui, id: egui::Id, text: &str, raw: bool, pal: &Palette) {
    if raw {
        let mut s: &str = text;
        ui.add(egui::TextEdit::multiline(&mut s).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(1));
        return;
    }
    let blocks = analyze(text);
    for (i, b) in blocks.iter().enumerate() {
        match b {
            Block::Para(p) => para(ui, p, pal),
            Block::Table(rows) => kv_table(ui, id.with(("t", i)), rows, pal),
            Block::Exception(ex) => exception_ui(ui, id.with(("x", i)), ex, &unescape(text), pal),
            Block::Post(p) => crate::post::post_ui(ui, id.with(("p", i)), p, pal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_curly() {
        let b = analyze("4080CALCULATION RESULT {Buffer ID = 5} {kVHiRes = 77.00} {Range Limits} {Max kV = 125} ");
        assert!(matches!(&b[0], Block::Para(p) if p.contains("CALCULATION")));
        match &b[1] {
            Block::Table(r) => {
                assert_eq!(r[0].key, "Buffer ID");
                assert_eq!(r[0].value, "5");
                assert!(r[2].header);
                assert_eq!(r.len(), 4);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn kv_nested() {
        let b = analyze("[[humanReadableEventId=21203] ,[description = The Service Procedure is Closed] , [originatingSubUnit = ServiceToolClose] Survey / Survey All,[SystemMode = FieldService]]");
        let Block::Table(r) = &b[0] else { panic!("{:?}", b) };
        assert_eq!(r[0].key, "humanReadableEventId");
        assert!(r.iter().any(|x| x.key == "SystemMode" && x.value == "FieldService"));
        let b = analyze("[Exception Info=[detail=CARM sensor level errors ]][EmbDateTime=2026-Oct-14, 14:28:45][ClassName=GSCRT]");
        let Block::Table(r) = &b[0] else { panic!() };
        assert!(r[0].header && r[0].key == "Exception Info");
        assert_eq!(r[1].key, "detail");
    }

    #[test]
    fn kv_nested_lists() {
        let t = "[Exception Info=[detail=eds(DRV=3; DRVR=0; AE=3; I=0;), MMS=2; MSS=0; actPM=891.596755; actPL=891.596755 setP=891.596755; FDE=0;tDV=0.000000; DIIN6=1]][EmbDateTime=2026-Feb-03, 09:48:05][ClassName=GSCRT]";
        let b = analyze(t);
        let Block::Table(r) = &b[0] else { panic!("{:?}", b) };
        let keys: Vec<&str> = r.iter().map(|x| x.key.as_str()).collect();
        println!("{:#?}", r);
        assert!(r[0].header && r[0].key == "Exception Info");
        assert!(r.iter().any(|x| x.header && x.key == "detail" && x.value == "eds"));
        for k in ["DRV", "DRVR", "AE", "I", "MMS", "MSS", "actPM", "actPL", "setP", "FDE", "tDV", "DIIN6", "EmbDateTime", "ClassName"] {
            assert!(keys.contains(&k), "missing {k}: {keys:?}");
        }
        assert_eq!(r.iter().find(|x| x.key == "EmbDateTime").unwrap().value, "2026-Feb-03, 09:48:05");
        assert_eq!(r.iter().find(|x| x.key == "DRV").unwrap().depth, 2);
        let b = analyze("[Configuration=FrontalStandType = CLEA_C; DetectorTypeFrontal = FD20;LateralStandType = NONE; ISOHeight = 10650; SpacerOption = Not Installed]");
        let Block::Table(r) = &b[0] else { panic!() };
        assert!(r[0].header && r[0].key == "Configuration");
        assert_eq!(r.iter().find(|x| x.key == "SpacerOption").unwrap().value, "Not Installed");
    }

    #[test]
    fn all_real_messages() {
        for var in ["LOGFILE", "LOGFILE2", "LOGFILE3"] {
            let Ok(p) = std::env::var(var) else { continue };
            let log = crate::parser::load(std::path::Path::new(&p), crate::parser::Progress::new()).unwrap();
            let t = std::time::Instant::now();
            let mut tables = 0;
            for e in &log.events {
                for txt in [log.pool.get(e.description), e.message.as_str()] {
                    if analyze(txt).iter().any(|b| matches!(b, Block::Table(_))) {
                        tables += 1;
                    }
                }
            }
            println!("{p}: {} events, {tables} texts as tables, {:?}", log.events.len(), t.elapsed());
        }
    }

    #[test]
    fn newlines_and_semicolons() {
        assert_eq!(unescape("a\\nb"), "a\nb");
        let b = analyze("Temperature card 0: 41; Temperature card 1: 46; Die temperature channel 0: 58; ");
        assert!(matches!(&b[0], Block::Table(r) if r.len() == 3));
    }

    #[test]
    fn dotnet_exception() {
        let t = "Arithmetic overflow error converting IDENTITY to data type int.\nArithmetic overflow occurred.\nCaller Information [Thread Name: -, Thread ID: 61, Thread State: Background, Thread Type: Background]\nCallStack:\n    at Philips.A.B.Util.Log(System.Exception e) [IL: 0x12][Native: 0x70]\n    at Philips.A.B.Plugin.UpdateToDB(System.Data.DataSet d) [IL: 0x1]\n\nArithmetic overflow error converting IDENTITY to data type int.\nArithmetic overflow occurred.\nException Details: [Philips.PmsMip.Public.Exceptions.FailException]: Arithmetic overflow error converting IDENTITY to data type int.\nArithmetic overflow occurred.\nCallStack:\nNo callstack information available.\n\nInner Exception Details: [System.Data.SqlClient.SqlException]: Arithmetic overflow error converting IDENTITY to data type int.\nCallStack:\n    at System.Data.Common.DbDataAdapter.UpdatedRowStatusErrors(System.Data.Common.RowUpdatedEventArgs r) [IL: 0xC7]\n    at Philips.A.B.Plugin.UpdateToDB(System.Data.DataSet d) [IL: 0x1]\n";
        let ex = parse_exception(t).unwrap();
        let root = ex.root().unwrap();
        assert_eq!(root.kind, "System.Data.SqlClient.SqlException");
        assert_eq!(root.frames[0].short, "DbDataAdapter.UpdatedRowStatusErrors");
        assert!(!root.frames[0].app);
        assert!(root.frames[1].app);
        assert!(ex.hint.is_some());
        assert_eq!(ex.caller.len(), 4);
        assert_eq!(ex.chain.iter().filter(|e| !e.kind.is_empty()).count(), 2);
    }

    #[test]
    fn java_exception() {
        let t = "java.lang.IllegalStateException: boom\n\tat com.philips.x.Foo.bar(Foo.java:12)\n\tat java.lang.Thread.run(Thread.java:748)\nCaused by: java.io.IOException: disk\n\tat com.philips.x.Io.read(Io.java:5)\n";
        let ex = parse_exception(t).unwrap();
        assert_eq!(ex.lang, "Java");
        assert_eq!(ex.root().unwrap().kind, "java.io.IOException");
        assert_eq!(ex.chain[0].frames[0].location, "Foo.java:12");
    }
}
