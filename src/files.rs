//! Other files inside the log ZIP: decoding, short descriptions, and turning
//! trace / text logs into a table (LogData) so they get filters, timeline, patterns...

use crate::model::*;
use crate::parser::{self, RawEvent};
use std::path::Path;

// ------------------------------------------------------------------ descriptions

/// Short description of well-known files (lower-case file name patterns).
pub fn describe(path: &str) -> Option<&'static str> {
    let n = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let full = path.to_ascii_lowercase();
    let starts = |p: &str| n.starts_with(p);
    Some(match () {
        _ if n.ends_with(".enc") => "Encrypted file – readable only with the Philips service tools",
        _ if n == "systemlogs.xml" => "System event log",
        _ if n == "gxrsids.xml" => "System identification: software / hardware versions, licenses",
        _ if n == "exportedsetupregistry.xml" => "Exported setup registry (system configuration)",
        _ if starts("registeros") || starts("registersystem") => "Output of the installation / registration scripts",
        _ if starts("calpackcantrace") => "CAN bus trace (Calpack)",
        _ if starts("direct_") || starts("earlylogin_") => "Logi framework log (Java)",
        _ if starts("errout") => "Error output of a Java process",
        _ if starts("gchistory") && n.ends_with(".log") => "Examination clean-up (garbage collector) history",
        _ if starts("hookandhide") => "Shell / startup helper log",
        _ if starts("iddyn_calibpixel") => "Dynamic detector: pixel calibration",
        _ if starts("iddyn_qa_test_det_bist") => "Dynamic detector: built-in self test",
        _ if starts("iddyn_longterm") => "Dynamic detector: long-term log",
        _ if starts("iddyn") => "Dynamic detector (IdDyn) log",
        _ if starts("idfxd-ip") => "Flat detector pre-processing server log",
        _ if starts("idfxd") => "Flat detector (IdFxd) log",
        _ if starts("gip") => "Image processing (GIP) log",
        _ if starts("trace-") && n.ends_with(".txt") => "Platform trace (structured, opens as a table)",
        _ if full.contains("/solidcore/") => "Solidcore (application control) log",
        _ if n.ends_with(".mdb") || n.ends_with(".accdb") => "Access database (CT event log opens as a table)",
        _ if n.ends_with(".evtx") => "Windows event log – open externally (Event Viewer)",
        _ if n.ends_with(".etl") => "Windows ETW trace – open externally",
        _ if n.ends_with(".lck") => "Lock file (empty)",
        _ => return None,
    })
}

pub fn icon(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".xml") {
        "🗋"
    } else if n.ends_with(".evtx") || n.ends_with(".etl") || n.ends_with(".dat") {
        "⚙"
    } else {
        "📄"
    }
}

pub fn fmt_size(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.1} MB", b as f64 / 1024.0 / 1024.0)
    }
}

// ------------------------------------------------------------------ decoding

/// Decodes bytes as text (UTF-8 / UTF-16 with or without BOM, else Latin-1).
/// Returns None for binary content.
pub fn is_binary_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".evtx", ".etl", ".dat", ".exe", ".dll", ".zip", ".png", ".jpg", ".bmp"].iter().any(|e| n.ends_with(e))
}

pub fn decode(b: &[u8]) -> Option<(String, &'static str)> {
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((String::from_utf8_lossy(&b[3..]).into_owned(), "UTF-8"));
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let u: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&u)
    };
    if b.starts_with(&[0xFF, 0xFE]) {
        return Some((utf16(&b[2..], true), "UTF-16 LE"));
    }
    if b.starts_with(&[0xFE, 0xFF]) {
        return Some((utf16(&b[2..], false), "UTF-16 BE"));
    }
    let sample = &b[..b.len().min(8192)];
    if sample.is_empty() {
        return Some((String::new(), "empty"));
    }
    let zeros_odd = sample.iter().skip(1).step_by(2).filter(|&&x| x == 0).count();
    let zeros_even = sample.iter().step_by(2).filter(|&&x| x == 0).count();
    let half = sample.len() / 2;
    if half > 4 && zeros_odd > half * 7 / 10 && zeros_even < half / 10 {
        return Some((utf16(b, true), "UTF-16 LE"));
    }
    let ctrl = sample.iter().filter(|&&x| x == 0 || (x < 0x09) || (0x0E..0x1A).contains(&x)).count();
    if ctrl * 20 > sample.len() {
        return None;
    }
    match std::str::from_utf8(b) {
        Ok(s) => Some((s.to_owned(), "UTF-8")),
        Err(_) => Some((b.iter().map(|&c| c as char).collect(), "Latin-1")),
    }
}

pub fn hex_dump(b: &[u8], max: usize) -> String {
    let mut out = String::new();
    for (i, chunk) in b[..b.len().min(max)].chunks(16).enumerate() {
        out.push_str(&format!("{:08X}  ", i * 16));
        for j in 0..16 {
            match chunk.get(j) {
                Some(x) => out.push_str(&format!("{x:02X} ")),
                None => out.push_str("   "),
            }
        }
        out.push(' ');
        for &x in chunk {
            out.push(if (0x20..0x7F).contains(&x) { x as char } else { '.' });
        }
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------------ text view helpers

/// 0 = normal, 1 = warning, 2 = error (rough guess per line, for coloring the text view)
pub fn line_level(line: &str) -> u8 {
    let head: String = line.chars().take(90).collect();
    if line.contains("»Error") || line.contains("»Fatal") {
        return 2;
    }
    if line.contains("»Warning") {
        return 1;
    }
    for (w, l) in [("FATAL", 2), ("SEVERE", 2), ("ERROR", 2), ("ERR ", 2), ("[ERR", 2), ("WARNING", 1), ("WARN", 1)] {
        if head.contains(w) {
            return l;
        }
    }
    0
}

pub fn line_offsets(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let b = text.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' {
            let mut end = i;
            if end > start && b[end - 1] == b'\r' {
                end -= 1;
            }
            out.push((start as u32, end as u32));
            start = i + 1;
        }
    }
    if start < b.len() {
        out.push((start as u32, b.len() as u32));
    }
    out
}

// ------------------------------------------------------------------ text -> table

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TextKind {
    /// "Platform 2.0 Common Utilities Tracing" files (» separated columns)
    PhilipsTrace,
    /// tab separated with a header line and DATE / TIME columns
    Tsv,
    /// lines starting with a timestamp
    Timestamped,
    Plain,
}

impl TextKind {
    pub fn label(self) -> &'static str {
        match self {
            TextKind::PhilipsTrace => "Platform trace",
            TextKind::Tsv => "Tab-separated log",
            TextKind::Timestamped => "Text log",
            TextKind::Plain => "Text",
        }
    }
    pub fn tabular(self) -> bool {
        self != TextKind::Plain
    }
}

fn digits(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|c| c.is_ascii_digit())
}

fn num(b: &[u8]) -> i64 {
    b.iter().fold(0i64, |a, &c| a * 10 + (c - b'0') as i64)
}

fn ms_of(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    parse_ts(&format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"))
}

/// fraction after '.' or ',' -> (ms, bytes consumed)
fn fraction(b: &[u8]) -> (i64, usize) {
    if b.len() >= 2 && (b[0] == b'.' || b[0] == b',') && b[1].is_ascii_digit() {
        let n = b[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        let mut ms = 0i64;
        for k in 0..3 {
            ms = ms * 10 + b.get(1 + k).filter(|c| c.is_ascii_digit() && k < n).map(|c| (c - b'0') as i64).unwrap_or(0);
        }
        (ms, 1 + n)
    } else {
        (0, 0)
    }
}

/// Timestamp at the start of a line -> (ms, has date, bytes consumed).
/// Also accepts "[timestamp]" and a short prefix like "K.5932.1064: " before it.
pub fn ts_prefix(line: &str) -> Option<(i64, bool, usize)> {
    if let Some(r) = ts_at_start(line) {
        return Some(r);
    }
    if let Some(rest) = line.strip_prefix('[') {
        if let Some((t, d, n)) = ts_at_start(rest) {
            let n = 1 + n + usize::from(rest.as_bytes().get(n) == Some(&b']'));
            return Some((t, d, n));
        }
    }
    let head = &line.as_bytes()[..line.len().min(28)];
    if let Some(p) = head.windows(2).position(|w| w == b": ") {
        if p > 0 && !head[..p].contains(&b' ') {
            if let Some((t, d, n)) = ts_at_start(&line[p + 2..]) {
                return Some((t, d, p + 2 + n));
            }
        }
    }
    None
}

const MONTHS3: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn ts_at_start(line: &str) -> Option<(i64, bool, usize)> {
    let b = line.as_bytes();
    let time_at = |p: usize| -> Option<(i64, i64, i64)> {
        let t = b.get(p..p + 8)?;
        (digits(&t[0..2]) && t[2] == b':' && digits(&t[3..5]) && t[5] == b':' && digits(&t[6..8])).then(|| (num(&t[0..2]), num(&t[3..5]), num(&t[6..8])))
    };
    // dd.mm.yyyy HH:MM:SS
    if b.len() >= 19 && digits(&b[0..2]) && b[2] == b'.' && digits(&b[3..5]) && b[5] == b'.' && digits(&b[6..10]) && b[10] == b' ' {
        if let Some((h, mi, s)) = time_at(11) {
            let (f, n) = fraction(&b[19..]);
            return Some((ms_of(num(&b[6..10]), num(&b[3..5]), num(&b[0..2]), h, mi, s) + f, true, 19 + n));
        }
    }
    // yyyy-mm-dd[ T\t]HH:MM:SS
    if b.len() >= 19 && digits(&b[0..4]) && b[4] == b'-' && digits(&b[5..7]) && b[7] == b'-' && digits(&b[8..10]) && matches!(b[10], b' ' | b'T' | b'\t') {
        if let Some((h, mi, s)) = time_at(11) {
            let (f, n) = fraction(&b[19..]);
            return Some((ms_of(num(&b[0..4]), num(&b[5..7]), num(&b[8..10]), h, mi, s) + f, true, 19 + n));
        }
    }
    // yyyymmdd-HHMMSS
    if b.len() >= 15 && digits(&b[0..8]) && b[8] == b'-' && digits(&b[9..15]) {
        let (f, n) = fraction(&b[15..]);
        let ms = ms_of(num(&b[0..4]), num(&b[4..6]), num(&b[6..8]), num(&b[9..11]), num(&b[11..13]), num(&b[13..15]));
        return Some((ms + f, true, 15 + n));
    }
    // MM/DD/YY HH:MM:SS (US style; DD/MM when the first number > 12)
    if b.len() >= 17 && digits(&b[0..2]) && b[2] == b'/' && digits(&b[3..5]) && b[5] == b'/' && digits(&b[6..8]) && b[8] == b' ' {
        if let Some((h, mi, s)) = time_at(9) {
            let (a, c) = (num(&b[0..2]), num(&b[3..5]));
            let (mo, d) = if a > 12 { (c, a) } else { (a, c) };
            let (f, n) = fraction(&b[17..]);
            return Some((ms_of(2000 + num(&b[6..8]), mo, d, h, mi, s) + f, true, 17 + n));
        }
    }
    // "Aug 31 2026:17:51:01.178" / "Aug 31, 2026 17:51:50"
    if b.len() >= 19 && b[0].is_ascii_alphabetic() && b[3] == b' ' {
        if let Some(mo) = MONTHS3.iter().position(|m| line[..3].eq_ignore_ascii_case(m)) {
            let mut p = 4;
            let dn = b[p..].iter().take_while(|c| c.is_ascii_digit()).count();
            if (1..=2).contains(&dn) {
                let d = num(&b[p..p + dn]);
                p += dn;
                if b.get(p) == Some(&b',') {
                    p += 1;
                }
                if b.get(p) == Some(&b' ') && b.len() >= p + 14 && digits(&b[p + 1..p + 5]) && matches!(b[p + 5], b':' | b' ') {
                    let y = num(&b[p + 1..p + 5]);
                    if let Some((h, mi, s)) = time_at(p + 6) {
                        let (f, n) = fraction(&b[p + 14..]);
                        return Some((ms_of(y, mo as i64 + 1, d, h, mi, s) + f, true, p + 14 + n));
                    }
                }
            }
        }
    }
    // HH:MM:SS (time only)
    if let Some((h, mi, s)) = time_at(0) {
        if b.get(8).is_none_or(|c| !c.is_ascii_digit()) {
            let (f, n) = fraction(&b[8..]);
            return Some(((h * 3600 + mi * 60 + s) * 1000 + f, false, 8 + n));
        }
    }
    None
}

pub fn detect_kind(text: &str) -> TextKind {
    let head: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(40).collect();
    if head.first().is_some_and(|l| l.starts_with("Platform") && l.contains("Tracing")) || head.iter().any(|l| l.starts_with("DateTime") && l.contains('»')) {
        return TextKind::PhilipsTrace;
    }
    if let Some(h) = head.first() {
        if h.matches('\t').count() >= 3 && ts_prefix(h).is_none() && head.iter().skip(1).filter(|l| ts_prefix(l).is_some()).count() * 2 >= head.len().saturating_sub(1) && head.len() > 1 {
            return TextKind::Tsv;
        }
    }
    let with_ts = head.iter().filter(|l| ts_prefix(l).is_some()).count();
    if head.len() >= 2 && with_ts * 10 >= head.len() * 3 {
        TextKind::Timestamped
    } else {
        TextKind::Plain
    }
}

const LEVELS: &[(&str, &str)] = &[
    ("FATAL", "Fatal"),
    ("CRITICAL", "Fatal"),
    ("SEVERE", "Error"),
    ("ERROR", "Error"),
    ("ERR", "Error"),
    ("WARNING", "Warning"),
    ("WARN", "Warning"),
    ("NOTICE", "Note"),
    ("NOTE", "Note"),
    ("INFO", "Information"),
    ("DEBUG", "Debug"),
    ("DBUG", "Debug"),
    ("TRACE", "Trace"),
    ("VERBOSE", "Verbose"),
    ("FINEST", "Debug"),
    ("FINER", "Debug"),
    ("FINE", "Debug"),
    ("CONFIG", "Config"),
];

/// finds a level word (upper case, whole word) near the start of `s` -> (start, end, label)
fn find_level(s: &str) -> Option<(usize, usize, &'static str)> {
    let b = s.as_bytes();
    let lim = s.len().min(60);
    let mut best: Option<(usize, usize, &'static str)> = None;
    for (w, label) in LEVELS {
        let mut from = 0;
        while let Some(p) = s[from..lim.max(from)].find(w) {
            let st = from + p;
            let en = st + w.len();
            let before_ok = st == 0 || !b[st - 1].is_ascii_alphabetic();
            let after_ok = en >= b.len() || !b[en].is_ascii_alphabetic();
            if before_ok && after_ok {
                if best.is_none_or(|(bs, _, _)| st < bs) {
                    best = Some((st, en, label));
                }
                break;
            }
            from = st + 1;
            if from >= lim {
                break;
            }
        }
    }
    best
}

fn clean_component(s: &str) -> String {
    let t = s.trim_matches(|c: char| c.is_whitespace() || "[]():|-".contains(c));
    if t.len() <= 40 && !t.contains("  ") {
        t.to_owned()
    } else {
        String::new()
    }
}

fn finish(log: &mut LogData, ev: &mut Option<(RawEvent, Vec<String>)>) {
    if let Some((mut e, extra)) = ev.take() {
        if !extra.is_empty() {
            let more = extra.join("\n");
            if e.message.is_empty() {
                e.message = more;
            } else {
                e.message = format!("{}\n{}", e.message, more);
            }
        }
        parser::push_event(log, e);
    }
}

fn parse_trace(log: &mut LogData, text: &str, stem: &str) {
    let mut cols: Vec<String> = vec![];
    let mut rec: Option<(usize, String)> = None;
    let mut records: Vec<(usize, String)> = vec![];
    for (ln, line) in text.lines().enumerate() {
        if cols.is_empty() && line.starts_with("DateTime") && line.contains('»') {
            cols = line.split('»').map(|c| c.trim().to_owned()).collect();
            continue;
        }
        let b = line.as_bytes();
        let starts = b.len() > 16 && digits(&b[0..8]) && b[8] == b'-' && digits(&b[9..15]);
        if starts {
            if let Some(r) = rec.take() {
                records.push(r);
            }
            rec = Some((ln + 1, line.to_owned()));
        } else if let Some((_, r)) = rec.as_mut() {
            r.push('\n');
            r.push_str(line);
        }
    }
    if let Some(r) = rec.take() {
        records.push(r);
    }
    if cols.is_empty() {
        cols = "DateTime Namespace ClassName ProcessId ProcessName ThreadID ThreadName Severity LogType EventType Description AdditionalInfo ExceptionInfo StackTrace MachineName ContextInfo"
            .split(' ')
            .map(str::to_owned)
            .collect();
    }
    for (ln, r) in records {
        let f: Vec<&str> = r.split('»').map(|x| x.trim_matches(|c: char| c == '\t' || c == ' ' || c == '\r')).collect();
        let mut e = RawEvent { index: ln as i64, process: stem.to_owned(), ..Default::default() };
        let mut exc = String::new();
        let mut stack = String::new();
        for (i, v) in f.iter().enumerate() {
            let v = v.trim();
            if v.is_empty() || v == "*{0}*" {
                continue;
            }
            match cols.get(i).map(String::as_str).unwrap_or("") {
                "DateTime" => e.ts = ts_prefix(v).map(|t| t.0).unwrap_or(0),
                "Namespace" => e.package = v.to_owned(),
                "ClassName" => e.component = v.to_owned(),
                "ProcessId" => e.pid = v.parse().unwrap_or(0),
                "ProcessName" => e.process = v.to_owned(),
                "ThreadID" => e.tid = v.parse().unwrap_or(0),
                "Severity" => e.category = v.to_owned(),
                "LogType" => e.log_cat = v.to_owned(),
                "EventType" => e.info_cat = v.to_owned(),
                "Description" => e.description = v.to_owned(),
                "AdditionalInfo" => e.message = v.to_owned(),
                "ExceptionInfo" => exc = v.to_owned(),
                "StackTrace" => stack = v.to_owned(),
                "MachineName" => e.machine = v.to_owned(),
                "" => {}
                k => e.extras.push((k.to_owned(), v.to_owned())),
            }
        }
        for extra in [exc, stack] {
            if !extra.is_empty() {
                if !e.message.is_empty() {
                    e.message.push('\n');
                }
                e.message.push_str(&extra);
            }
        }
        if e.category.is_empty() {
            e.category = "Information".into();
        }
        parser::push_event(log, e);
    }
}

fn parse_tsv(log: &mut LogData, text: &str, stem: &str) {
    let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty());
    let Some((_, header)) = lines.next() else { return };
    let cols: Vec<&str> = header.split('\t').map(str::trim).collect();
    for (ln, line) in lines {
        let Some((ts, _, _)) = ts_prefix(line) else { continue };
        let f: Vec<&str> = line.split('\t').collect();
        let mut e = RawEvent { index: ln as i64 + 1, ts, process: stem.to_owned(), category: "Information".into(), ..Default::default() };
        let mut desc = vec![];
        let mut kv = String::new();
        for (i, v) in f.iter().enumerate().skip(2) {
            let v = v.trim();
            if matches!(v, "ERR" | "ERROR") {
                e.category = "Error".into();
            }
            if v.is_empty() {
                continue;
            }
            let name = cols.get(i).copied().unwrap_or("");
            desc.push(v.trim_matches('_'));
            kv.push_str(&format!("{{{} = {}}} ", if name.is_empty() { "?" } else { name }, v));
        }
        e.description = desc.join("  ·  ");
        e.message = kv.trim_end().to_owned();
        parser::push_event(log, e);
    }
}

fn parse_timestamped(log: &mut LogData, text: &str, stem: &str, modified: i64) {
    // time-only logs (no date): count midnight roll-overs so the last line lands on the file date
    let mut day_offset_total = 0i64;
    let mut prev = -1i64;
    let mut has_date_any = false;
    for line in text.lines() {
        if let Some((t, has_date, _)) = ts_prefix(line) {
            if has_date {
                has_date_any = true;
                break;
            }
            if prev >= 0 && t + 12 * 3_600_000 < prev {
                day_offset_total += 1;
            }
            prev = t;
        }
    }
    let file_day = modified.div_euclid(86_400_000) * 86_400_000;
    let mut day = file_day - day_offset_total * 86_400_000;
    prev = -1;

    let mut cur: Option<(RawEvent, Vec<String>)> = None;
    for (ln, line) in text.lines().enumerate() {
        match ts_prefix(line) {
            Some((mut t, has_date, used)) => {
                finish(log, &mut cur);
                if !has_date && !has_date_any {
                    if prev >= 0 && t + 12 * 3_600_000 < prev {
                        day += 86_400_000;
                    }
                    prev = t;
                    t += day;
                }
                let rest = &line[used.min(line.len())..];
                let mut e = RawEvent { index: ln as i64 + 1, ts: t, process: stem.to_owned(), ..Default::default() };
                let mut body = rest;
                if let Some((st, en, label)) = find_level(rest) {
                    e.category = label.to_owned();
                    let before = clean_component(&rest[..st]);
                    body = &rest[en..];
                    body = body.trim_start_matches(|c: char| c == '.' || c == ':' || c == ']' || c == ' ' || c == '-' || c == '|');
                    if !before.is_empty() {
                        e.component = before;
                    } else if body.starts_with('(') {
                        if let Some(p) = body.find(')') {
                            e.component = clean_component(&body[1..p]);
                            body = body[p + 1..].trim_start_matches(|c: char| c == ':' || c == ' ');
                        }
                    }
                } else {
                    e.category = "Information".into();
                }
                e.description = body.trim().to_owned();
                cur = Some((e, vec![]));
            }
            None => {
                if let Some((e, extra)) = cur.as_mut() {
                    if e.description.is_empty() {
                        e.description = line.trim().to_owned();
                    } else if !(line.trim().is_empty() && extra.is_empty()) {
                        extra.push(line.trim_end().to_owned());
                    }
                }
            }
        }
    }
    finish(log, &mut cur);
}

/// Builds a table (LogData) from a text file inside the ZIP.
pub fn text_to_log(zip: &Path, entry: &ArchiveEntry, text: &str, kind: TextKind) -> Result<LogData, String> {
    let t0 = std::time::Instant::now();
    let name = entry.file_name().to_owned();
    let stem = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(&name).to_owned();
    let mut log = parser::new_log(zip, name);
    log.entry = Some(entry.name.clone());
    match kind {
        TextKind::PhilipsTrace => parse_trace(&mut log, text, &stem),
        TextKind::Tsv => parse_tsv(&mut log, text, &stem),
        TextKind::Timestamped => parse_timestamped(&mut log, text, &stem, entry.modified),
        TextKind::Plain => return Err("This file has no timestamps, so it cannot be shown as a table".into()),
    }
    if log.events.is_empty() {
        return Err("No log lines with a timestamp were found".into());
    }
    log.kind = kind.label().to_owned();
    log.finalize();
    log.load_ms = t0.elapsed().as_millis();
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert!(ts_prefix("31.08.2026 17:51:24.634 INFO.......  Begin").is_some());
        assert!(ts_prefix("2026-08-31\t17:50:53.5450\t____").is_some());
        assert!(ts_prefix("20260831-130722.289684\t»Philips").is_some());
        let (t, d, _) = ts_prefix("03:25:13 INFO:Wallpaper").unwrap();
        assert!(!d && t == (3 * 3600 + 25 * 60 + 13) * 1000);
        assert_eq!(fmt_ts(ts_prefix("31.08.2026 17:51:24.634 X").unwrap().0), "2026-08-31 17:51:24.634");
        assert_eq!(fmt_ts(ts_prefix("[2026-08-31 17:48:20] [INFO] x").unwrap().0), "2026-08-31 17:48:20.000");
        assert_eq!(fmt_ts(ts_prefix("08/19/26 10:47:01 [6748] FORCE").unwrap().0), "2026-08-19 10:47:01.000");
        assert_eq!(fmt_ts(ts_prefix("K.5932.1064: Aug 31 2026:17:51:01.178:   SYSTEM").unwrap().0), "2026-08-31 17:51:01.178");
        assert_eq!(fmt_ts(ts_prefix("Aug 31, 2026 17:51:50 INFO: P").unwrap().0), "2026-08-31 17:51:50.000");
    }

    #[test]
    fn levels() {
        let l = find_level(" MAIN: NOTE: IdDyn startup").unwrap();
        assert_eq!(l.2, "Note");
        assert_eq!(clean_component(&" MAIN: NOTE: IdDyn"[..l.0]), "MAIN");
        assert_eq!(find_level(" [   INFO] (IdfxdStartupController): x").unwrap().2, "Information");
        assert_eq!(find_level(" SEVERE.....  Instantiation problem").unwrap().2, "Error");
        assert!(find_level(" Something Errorless happened").is_none());
    }

    #[test]
    fn decode_utf16() {
        let s = "Hello";
        let mut b = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&b).unwrap().0, "Hello");
    }

    #[test]
    fn real_zip_files() {
        let Ok(zip) = std::env::var("ZIPFILE") else { return };
        let zip = std::path::PathBuf::from(zip);
        let entries = parser::list_zip(&zip).unwrap();
        let mut seen = std::collections::HashSet::new();
        for en in &entries {
            // one file per name family is enough
            let fam: String = en.file_name().chars().filter(|c| !c.is_ascii_digit()).collect();
            if !seen.insert(fam) {
                continue;
            }
            let bytes = parser::read_zip_entry(&zip, en.index).unwrap();
            let t = std::time::Instant::now();
            match if is_binary_name(&en.name) { None } else { decode(&bytes) } {
                None => println!("{:<70} binary {}", en.name, fmt_size(en.size)),
                Some((text, enc)) => {
                    let kind = detect_kind(&text);
                    let r = if kind.tabular() { text_to_log(&zip, en, &text, kind).map(|l| (l.events.len(), l.counts)) } else { Err("-".into()) };
                    match r {
                        Ok((n, c)) => println!("{:<70} {:<9} {:<16} {:>7} rows  W{} E{} F{}  {:?}", en.name, enc, kind.label(), n, c[1], c[2], c[3], t.elapsed()),
                        Err(e) => println!("{:<70} {:<9} {:<16} {}", en.name, enc, kind.label(), e),
                    }
                }
            }
        }
    }
}
