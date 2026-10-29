use crate::model::*;
use crate::xmltree::XmlNode;
use quick_xml::events::{BytesStart, Event as XmlEvent};
use quick_xml::Reader;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Progress {
            done: AtomicU64::new(0),
            total: AtomicU64::new(1),
        })
    }
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) as f64 / t as f64).min(1.0) as f32
    }
}

struct Counting<R> {
    inner: R,
    p: Arc<Progress>,
    base: u64,
    n: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let r = self.inner.read(buf)?;
        self.n += r as u64;
        self.p.done.store(self.base + self.n, Ordering::Relaxed);
        Ok(r)
    }
}

/// One event before it is interned into the log (also used by the text/trace log readers).
#[derive(Default)]
pub struct RawEvent {
    pub index: i64,
    pub ts: i64,
    pub system_mode: String,
    pub machine: String,
    pub unit: String,
    pub subunit: String,
    pub process: String,
    pub component: String,
    pub package: String,
    pub pid: u32,
    pub tid: u32,
    pub description: String,
    pub log_cat: String,
    pub info_cat: String,
    pub category: String,
    pub event_id: String,
    pub message: String,
    pub extras: Vec<(String, String)>,
}

type Partial = RawEvent;

pub fn new_log(path: &Path, file_name: String) -> LogData {
    LogData {
        path: path.to_path_buf(),
        entry: None,
        kind: String::new(),
        file_name,
        events: Vec::with_capacity(32_000),
        pool: Pool::default(),
        info: LogInfo::default(),
        by_time: vec![],
        t_min: 0,
        t_max: 0,
        mode_segments: vec![],
        notable: vec![],
        counts: [0; 5],
        load_ms: 0,
        archive: vec![],
        sysinfo: None,
    }
}

pub fn push_event(log: &mut LogData, p: RawEvent) {
    let pool = &mut log.pool;
    let desc_norm = normalize_desc(&p.description);
    let mut hay = String::with_capacity(p.description.len() + p.message.len() + 64);
    for part in [&p.description, &p.event_id, &p.message, &p.unit, &p.subunit, &p.process, &p.machine, &p.component, &p.package] {
        hay.push_str(&part.to_lowercase());
        hay.push('\u{1}');
    }
    for (_, v) in &p.extras {
        hay.push_str(&v.to_lowercase());
        hay.push('\u{1}');
    }
    let severity = Severity::parse(&p.category);
    let extras = p.extras.iter().map(|(k, v)| (pool.intern(k), v.clone())).collect();
    let ev = Event {
        index: p.index,
        kind: RowKind::classify(severity, &p.category, &p.info_cat, &p.description),
        ts: p.ts,
        severity,
        category: pool.intern(&p.category),
        description: pool.intern(&p.description),
        desc_norm: pool.intern(&desc_norm),
        event_id: pool.intern(&p.event_id),
        system_mode: pool.intern(&p.system_mode),
        log_cat: pool.intern(&p.log_cat),
        info_cat: pool.intern(&p.info_cat),
        machine: pool.intern(&p.machine),
        unit: pool.intern(&p.unit),
        subunit: pool.intern(&p.subunit),
        process: pool.intern(&p.process),
        component: pool.intern(&p.component),
        package: pool.intern(&p.package),
        extras,
        pid: p.pid,
        tid: p.tid,
        message: p.message,
        hay,
    };
    log.events.push(ev);
}

/// Attributes that have their own field / column, or carry no information.
const KNOWN: &[&str] = &[
    "Index", "TimeStamp", "TimeFraction", "SecondTimeStamp", "SecondTimeFraction", "SystemMode", "MachineName", "Unit", "SubUnit",
    "ProcessName", "ProcessID", "ThreadID", "Component", "SoftwarePackage", "Description", "LogCategory", "InfoCategory",
    "EventCategory", "EventID",
];

fn keep_extra(k: &str, v: &str) -> bool {
    let v = v.trim();
    !v.is_empty() && !KNOWN.contains(&k) && !(v == "0" && matches!(k, "LineNumber" | "RelatedEventIndex"))
}

struct Builder {
    log: LogData,
    cur: Partial,
    in_event: bool,
    in_eventlog: bool,
    header: Vec<XmlNode>,
}

impl Builder {
    fn finish_event(&mut self) {
        let p = std::mem::take(&mut self.cur);
        push_event(&mut self.log, p);
        self.in_event = false;
    }

    fn handle_start(&mut self, e: &BytesStart, empty: bool) {
        let name = e.local_name();
        let name = name.as_ref();
        if name == b"EventLog" {
            self.in_eventlog = !empty;
            return;
        }
        // keep everything outside <EventLog> as a tree for the "System info" view
        if !self.in_eventlog && !self.in_event {
            let n = XmlNode::from_start(e);
            if empty {
                if let Some(top) = self.header.last_mut() {
                    top.children.push(n);
                }
            } else {
                self.header.push(n);
            }
        }
        match name {
            b"Event" => {
                self.cur = Partial::default();
                self.in_event = true;
                let mut ts = 0i64;
                let mut frac = 0i64;
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default();
                    let k = a.key.local_name();
                    match k.as_ref() {
                        b"Index" => self.cur.index = v.trim().parse().unwrap_or(0),
                        b"TimeStamp" => ts = parse_ts(&v),
                        b"TimeFraction" => frac = v.trim().parse().unwrap_or(0),
                        kk => {
                            let ks = String::from_utf8_lossy(kk);
                            if keep_extra(&ks, &v) {
                                self.cur.extras.push((ks.into_owned(), v.into_owned()));
                            }
                        }
                    }
                }
                self.cur.ts = ts + frac.clamp(0, 999);
                if empty {
                    self.finish_event();
                }
            }
            b"EventOriginatorInfo" if self.in_event => {
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default().into_owned();
                    let k = a.key.local_name();
                    match k.as_ref() {
                        b"SystemMode" => self.cur.system_mode = v,
                        b"MachineName" => self.cur.machine = v,
                        b"Unit" => self.cur.unit = v,
                        b"SubUnit" => self.cur.subunit = v,
                        b"ProcessName" => self.cur.process = v,
                        b"Component" => self.cur.component = v,
                        b"SoftwarePackage" => self.cur.package = v,
                        b"ProcessID" => self.cur.pid = v.trim().parse().unwrap_or(0),
                        b"ThreadID" => self.cur.tid = v.trim().parse().unwrap_or(0),
                        kk => {
                            let ks = String::from_utf8_lossy(kk).into_owned();
                            if keep_extra(&ks, &v) {
                                self.cur.extras.push((ks, v));
                            }
                        }
                    }
                }
            }
            b"EventInfo" if self.in_event => {
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default().into_owned();
                    let k = a.key.local_name();
                    match k.as_ref() {
                        b"Description" => self.cur.description = v,
                        b"LogCategory" => self.cur.log_cat = v,
                        b"InfoCategory" => self.cur.info_cat = v,
                        b"EventCategory" => self.cur.category = v,
                        b"EventID" => self.cur.event_id = v,
                        kk => {
                            let ks = String::from_utf8_lossy(kk).into_owned();
                            if keep_extra(&ks, &v) {
                                self.cur.extras.push((ks, v));
                            }
                        }
                    }
                }
            }
            b"AdditionalInfo" if self.in_event => {
                for a in e.attributes().flatten() {
                    if a.key.local_name().as_ref() == b"Message" {
                        let v = a.unescape_value().unwrap_or_default();
                        let v = v.trim_end();
                        if v.is_empty() {
                            continue;
                        }
                        if !self.cur.message.is_empty() {
                            self.cur.message.push_str(" | ");
                        }
                        self.cur.message.push_str(v);
                    }
                }
            }
            b"ProductInfo" if !self.in_eventlog => {
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default().into_owned();
                    match a.key.local_name().as_ref() {
                        b"ProductFamily" => self.log.info.family = v,
                        b"Model" => self.log.info.model = v,
                        b"Version" => self.log.info.version = v,
                        b"Modality" => self.log.info.modality = v,
                        b"TechnicalVersion" => self.log.info.technical_version = v,
                        _ => {}
                    }
                }
            }
            b"CustomerInfo" if !self.in_eventlog => {
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default().into_owned();
                    match a.key.local_name().as_ref() {
                        b"HospitalName" => self.log.info.hospital = v,
                        b"Department" => self.log.info.department = v,
                        b"Room" => self.log.info.room = v,
                        b"City" => self.log.info.city = v,
                        b"Country" => self.log.info.country = v,
                        _ => {}
                    }
                }
            }
            b"SystemConfiguration" if !self.in_eventlog => {
                for a in e.attributes().flatten() {
                    let v = a.unescape_value().unwrap_or_default().into_owned();
                    match a.key.local_name().as_ref() {
                        b"SerialNumber" => self.log.info.serial = v,
                        b"ProductID" => self.log.info.product_id = v,
                        _ => {}
                    }
                }
            }
            b"MedicalSystem" => {
                for a in e.attributes().flatten() {
                    if a.key.local_name().as_ref() == b"CreationDate" {
                        self.log.info.creation = a.unescape_value().unwrap_or_default().into_owned();
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_end(&mut self, name: &[u8]) {
        if name == b"Event" && self.in_event {
            self.finish_event();
            return;
        }
        if name == b"EventLog" {
            self.in_eventlog = false;
            return;
        }
        if !self.in_eventlog && !self.in_event && self.header.len() > 1 {
            let n = self.header.pop().unwrap();
            self.header.last_mut().unwrap().children.push(n);
        }
    }

    fn parse<R: BufRead>(&mut self, r: R) -> Result<(), String> {
        let mut reader = Reader::from_reader(r);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::with_capacity(4096);
        self.header = vec![XmlNode { name: "#document".into(), ..Default::default() }];
        self.in_eventlog = false;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(e)) => self.handle_start(&e, false),
                Ok(XmlEvent::Empty(e)) => self.handle_start(&e, true),
                Ok(XmlEvent::End(e)) => self.handle_end(e.local_name().as_ref()),
                Ok(XmlEvent::Eof) => break,
                Err(e) => {
                    if self.log.events.is_empty() {
                        return Err(format!("XML error at position {}: {}", reader.buffer_position(), e));
                    }
                    // tolerate truncated files: keep what was read
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        if self.in_event {
            self.finish_event();
        }
        while self.header.len() > 1 {
            let n = self.header.pop().unwrap();
            self.header.last_mut().unwrap().children.push(n);
        }
        if let Some(doc) = self.header.pop() {
            if self.log.sysinfo.is_none() && !doc.children.is_empty() {
                self.log.sysinfo = Some(doc);
            }
        }
        Ok(())
    }
}

pub fn is_zip(path: &Path) -> bool {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut m = [0u8; 4];
    f.read_exact(&mut m).is_ok() && &m == b"PK\x03\x04"
}

/// Does this XML (first 512 KB) look like an event log?
pub fn looks_like_events<R: Read>(r: R) -> bool {
    let mut head = Vec::with_capacity(1 << 19);
    let _ = r.take(1 << 19).read_to_end(&mut head);
    let find = |n: &[u8]| head.windows(n.len()).any(|w| w == n);
    find(b"<EventLog") || find(b"<Event ")
}

pub fn list_zip(path: &Path) -> Result<Vec<ArchiveEntry>, String> {
    let f = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(f)).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        if let Ok(zf) = zip.by_index_raw(i) {
            let name = zf.name().replace('\\', "/");
            if name.ends_with('/') {
                continue;
            }
            let modified = zf
                .last_modified()
                .map(|d| {
                    parse_ts(&format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                        d.year(),
                        d.month(),
                        d.day(),
                        d.hour(),
                        d.minute(),
                        d.second()
                    ))
                })
                .unwrap_or(0);
            out.push(ArchiveEntry { index: i, name, size: zf.size(), is_events: false, modified });
        }
    }
    Ok(out)
}

/// Reads one file from the ZIP into memory.
pub fn read_zip_entry(path: &Path, index: usize) -> Result<Vec<u8>, String> {
    let f = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(f)).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let mut zf = zip.by_index(index).map_err(|e| e.to_string())?;
    let mut v = Vec::with_capacity(zf.size() as usize);
    zf.read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

pub fn load(path: &Path, progress: Arc<Progress>) -> Result<LogData, String> {
    if crate::mdb::is_mdb(path) {
        progress.total.store(1, Ordering::Relaxed);
        let r = crate::mdb::load(path);
        progress.done.store(1, Ordering::Relaxed);
        return r;
    }
    let t0 = Instant::now();
    let mut b = Builder {
        log: new_log(path, path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
        cur: Partial::default(),
        in_event: false,
        in_eventlog: false,
        header: vec![],
    };

    if is_zip(path) {
        let mut entries = list_zip(path)?;
        let f = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
        let mut zip = zip::ZipArchive::new(BufReader::new(f)).map_err(|e| format!("Invalid ZIP: {e}"))?;
        // only XML files that actually contain <Event> records go to the main table
        let mut total = 0u64;
        for en in entries.iter_mut() {
            if en.name.to_ascii_lowercase().ends_with(".xml") {
                if let Ok(zf) = zip.by_index(en.index) {
                    if looks_like_events(zf) {
                        en.is_events = true;
                        total += en.size;
                    }
                }
            }
        }
        if total == 0 {
            return Err("The ZIP does not contain an XML file with <Event> records".into());
        }
        progress.total.store(total.max(1), Ordering::Relaxed);
        let mut base = 0u64;
        for en in entries.iter().filter(|e| e.is_events) {
            let zf = zip.by_index(en.index).map_err(|e| e.to_string())?;
            let rd = Counting { inner: zf, p: progress.clone(), base, n: 0 };
            b.parse(BufReader::with_capacity(1 << 16, rd))?;
            base += en.size;
        }
        b.log.archive = entries;
    } else {
        let f = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
        let len = f.metadata().map(|m| m.len()).unwrap_or(1);
        progress.total.store(len.max(1), Ordering::Relaxed);
        let rd = Counting { inner: f, p: progress.clone(), base: 0, n: 0 };
        b.parse(BufReader::with_capacity(1 << 16, rd))?;
    }
    if b.log.events.is_empty() {
        return Err("No <Event> records found in the file".into());
    }
    b.log.kind = if b.log.info.family.trim().is_empty() { "XML event log".into() } else { format!("{} event log", b.log.info.family.trim()) };
    b.log.finalize();
    b.log.load_ms = t0.elapsed().as_millis();
    Ok(b.log)
}
