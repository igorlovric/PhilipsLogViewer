use std::collections::HashMap;
use std::path::PathBuf;

pub type Sym = u32;

/// String pool: repetitive values (category, unit, event id...) are stored only once.
#[derive(Default)]
pub struct Pool {
    strings: Vec<String>,
    lower: Vec<String>,
    map: HashMap<String, Sym>,
}

impl Pool {
    pub fn intern(&mut self, s: &str) -> Sym {
        if let Some(&i) = self.map.get(s) {
            return i;
        }
        let i = self.strings.len() as Sym;
        self.strings.push(s.to_owned());
        self.lower.push(s.to_lowercase());
        self.map.insert(s.to_owned(), i);
        i
    }
    #[inline]
    pub fn get(&self, i: Sym) -> &str {
        &self.strings[i as usize]
    }
    #[inline]
    pub fn lower(&self, i: Sym) -> &str {
        &self.lower[i as usize]
    }
    pub fn find(&self, s: &str) -> Option<Sym> {
        self.map.get(s).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Severity {
    Info = 0,
    Warning = 1,
    Error = 2,
    Fatal = 3,
    Other = 4,
}

impl Severity {
    pub fn parse(s: &str) -> Self {
        let l = s.trim().to_ascii_lowercase();
        match l.as_str() {
            "information" | "info" | "informational" | "debuginfo" | "debug" | "dbug" | "note" | "notice" | "trace" | "verbose"
            | "fine" | "finer" | "finest" | "config" => Severity::Info,
            "warning" | "warn" => Severity::Warning,
            "error" | "err" | "severe" => Severity::Error,
            "fatal" | "critical" | "crash" => Severity::Fatal,
            _ => Severity::Other,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Information",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
            Severity::Fatal => "Fatal",
            Severity::Other => "Other",
        }
    }
}

/// Row coloring class (Fatal > Error > Warning > UserMessage > Command > Information)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Fatal,
    Error,
    Warning,
    UserMessage,
    Command,
    Info,
    Other,
}

impl RowKind {
    pub fn classify(sev: Severity, category: &str, info_cat: &str, description: &str) -> Self {
        let cat = category.trim().to_ascii_lowercase();
        match sev {
            Severity::Fatal => return RowKind::Fatal,
            Severity::Error => return RowKind::Error,
            Severity::Warning => return RowKind::Warning,
            _ => {}
        }
        if cat == "usermessage" || info_cat.trim().eq_ignore_ascii_case("UserMessage") {
            return RowKind::UserMessage;
        }
        if cat == "command" || description.trim_start().to_ascii_lowercase().starts_with("command") {
            return RowKind::Command;
        }
        if sev == Severity::Info {
            RowKind::Info
        } else {
            RowKind::Other
        }
    }
}

pub struct Event {
    pub index: i64,
    pub kind: RowKind,
    /// milliseconds since 1970-01-01 (time as written in the log, no TZ conversion)
    pub ts: i64,
    pub severity: Severity,
    pub category: Sym,
    pub description: Sym,
    /// lowercase description with digit runs replaced by '#', used by pattern matching
    pub desc_norm: Sym,
    pub event_id: Sym,
    pub system_mode: Sym,
    pub log_cat: Sym,
    pub info_cat: Sym,
    pub machine: Sym,
    pub unit: Sym,
    pub subunit: Sym,
    pub process: Sym,
    pub component: Sym,
    pub package: Sym,
    /// other non-empty attributes (UserContext, ThreadName, ResolutionText...)
    pub extras: Vec<(Sym, String)>,
    pub pid: u32,
    pub tid: u32,
    pub message: String,
    /// lowercase text used by global search
    pub hay: String,
}

#[derive(Default, Clone)]
pub struct LogInfo {
    pub family: String,
    pub model: String,
    pub version: String,
    pub serial: String,
    pub product_id: String,
    pub modality: String,
    pub creation: String,
    pub technical_version: String,
    pub hospital: String,
    pub department: String,
    pub room: String,
    pub city: String,
    pub country: String,
}

pub struct LogData {
    /// file on disk (the .xml, or the .zip)
    pub path: PathBuf,
    /// file inside the ZIP shown in the table instead of the main event log
    pub entry: Option<String>,
    /// kind of log (e.g. "Azurion event log", "CT log database", "Platform trace"); used for per-type column sets
    pub kind: String,
    pub file_name: String,
    pub events: Vec<Event>,
    pub pool: Pool,
    pub info: LogInfo,
    /// event indices sorted by time
    pub by_time: Vec<u32>,
    pub t_min: i64,
    pub t_max: i64,
    /// (start, end, system_mode) runs in time order
    pub mode_segments: Vec<(i64, i64, Sym)>,
    /// warning/error/fatal events in time order
    pub notable: Vec<u32>,
    pub counts: [usize; 5],
    pub load_ms: u128,
    /// all files of the ZIP (empty for a plain XML)
    pub archive: Vec<ArchiveEntry>,
    /// header of the event XML (SystemConfiguration...) as a tree
    pub sysinfo: Option<crate::xmltree::XmlNode>,
}

#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub index: usize,
    /// path inside the ZIP, always with '/'
    pub name: String,
    pub size: u64,
    /// this XML contains the events shown in the main table
    pub is_events: bool,
    /// last-modified time from the ZIP (ms, local time as stored)
    pub modified: i64,
}

impl ArchiveEntry {
    pub fn file_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
}

impl LogData {
    /// companion .plv.json file for bookmarks / notes / view state
    pub fn data_file(&self) -> PathBuf {
        match &self.entry {
            None => self.path.with_extension(crate::config::LOG_EXT),
            Some(e) => {
                let stem = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let inner: String = e
                    .rsplit('/')
                    .next()
                    .unwrap_or(e)
                    .chars()
                    .map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '_' })
                    .collect();
                self.path.with_file_name(format!("{stem}__{inner}.{}", crate::config::LOG_EXT))
            }
        }
    }

    pub fn finalize(&mut self) {
        let n = self.events.len();
        let mut by_time: Vec<u32> = (0..n as u32).collect();
        by_time.sort_by_key(|&i| self.events[i as usize].ts);
        self.t_min = by_time.first().map(|&i| self.events[i as usize].ts).unwrap_or(0);
        self.t_max = by_time.last().map(|&i| self.events[i as usize].ts).unwrap_or(0);
        if self.t_max <= self.t_min {
            self.t_max = self.t_min + 1000;
        }
        let mut segs: Vec<(i64, i64, Sym)> = Vec::new();
        let mut notable = Vec::new();
        let mut counts = [0usize; 5];
        for &i in &by_time {
            let e = &self.events[i as usize];
            counts[e.severity as usize] += 1;
            if matches!(e.severity, Severity::Warning | Severity::Error | Severity::Fatal) {
                notable.push(i);
            }
            match segs.last_mut() {
                Some(last) if last.2 == e.system_mode => last.1 = e.ts,
                Some(last) => {
                    last.1 = e.ts;
                    segs.push((e.ts, e.ts, e.system_mode));
                }
                None => segs.push((e.ts, e.ts, e.system_mode)),
            }
        }
        self.by_time = by_time;
        self.mode_segments = segs;
        self.notable = notable;
        self.counts = counts;
    }
}

pub fn normalize_desc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digit = false;
    for c in s.trim().chars() {
        if c.is_ascii_digit() {
            if !in_digit {
                out.push('#');
            }
            in_digit = true;
        } else {
            in_digit = false;
            for l in c.to_lowercase() {
                out.push(l);
            }
        }
    }
    out
}

// ---------- time helpers (no external crate) ----------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn num(b: &[u8]) -> i64 {
    let mut v = 0i64;
    for &c in b {
        if c.is_ascii_digit() {
            v = v * 10 + (c - b'0') as i64;
        }
    }
    v
}

/// Parses "2026-10-14T09:28:09" (+ optional fraction) into ms.
pub fn parse_ts(s: &str) -> i64 {
    let b = s.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let y = num(&b[0..4]);
    let mo = num(&b[5..7]);
    let d = num(&b[8..10]);
    let h = num(&b[11..13]);
    let mi = num(&b[14..16]);
    let se = num(&b[17..19]);
    let days = days_from_civil(y, mo, d);
    ((days * 86400 + h * 3600 + mi * 60 + se) * 1000) as i64
}

/// Parses "YYYY-MM-DD HH:MM:SS.mmm" (as written by fmt_ts) back into ms.
pub fn parse_ts_ms(s: &str) -> i64 {
    let s = s.trim();
    let base = parse_ts(s);
    let ms = s.get(20..23).and_then(|f| f.parse::<i64>().ok()).unwrap_or(0);
    base + ms
}

pub struct Dt {
    pub y: i64,
    pub mo: u32,
    pub d: u32,
    pub h: u32,
    pub mi: u32,
    pub s: u32,
    pub ms: u32,
}

pub fn split_ts(ms: i64) -> Dt {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = civil_from_days(days);
    Dt {
        y,
        mo,
        d,
        h: (rem / 3_600_000) as u32,
        mi: (rem / 60_000 % 60) as u32,
        s: (rem / 1000 % 60) as u32,
        ms: (rem % 1000) as u32,
    }
}

pub fn fmt_ts(ms: i64) -> String {
    let t = split_ts(ms);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.y, t.mo, t.d, t.h, t.mi, t.s, t.ms
    )
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn month_name(m: u32) -> &'static str {
    MONTHS[(m.clamp(1, 12) - 1) as usize]
}

pub fn fmt_ts_short(ms: i64) -> String {
    let t = split_ts(ms);
    format!("{} {} {:02}:{:02}:{:02}", t.d, month_name(t.mo), t.h, t.mi, t.s)
}

pub fn fmt_duration(ms: i64) -> String {
    let ms = ms.abs();
    if ms < 1000 {
        format!("{} ms", ms)
    } else if ms < 60_000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else if ms < 3_600_000 {
        format!("{}m {:02}s", ms / 60_000, ms / 1000 % 60)
    } else {
        format!("{}h {:02}m", ms / 3_600_000, ms / 60_000 % 60)
    }
}

pub fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
