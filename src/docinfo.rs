//! Files that are not event logs: encrypted Philips files (e.g. ultrasound transducer tests, `.enc`)
//! and archives without an event log (e.g. ultrasound utilization reports: MedicalSystem.xml + PDFs).
//! Everything that can be learned without opening them is collected into a `FileInfo`.

use crate::files::fmt_size;
use crate::model::ArchiveEntry;
use crate::xmltree::{self, XmlNode};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Document {
    pub entry: ArchiveEntry,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct FileInfo {
    /// the file on disk (the .enc, or the ZIP)
    pub path: PathBuf,
    pub title: String,
    pub kind: String,
    /// label / value pairs
    pub rows: Vec<(String, String)>,
    /// e.g. "cannot be opened because it is encrypted"
    pub notice: Option<String>,
    /// files of the archive that can be opened with Windows (PDF reports first)
    pub documents: Vec<Document>,
    /// hardware parts from MedicalSystem.xml: type, part number, serial number, version
    pub hardware: Vec<[String; 4]>,
    /// software parts: name, install date
    pub software: Vec<[String; 2]>,
}

fn modality(code: &str) -> Option<&'static str> {
    Some(match code.to_ascii_uppercase().as_str() {
        "US" => "Ultrasound",
        "CT" => "Computed tomography",
        "MR" => "Magnetic resonance",
        "XR" | "DXR" | "DR" => "X-ray",
        "CV" | "IGT" => "Interventional X-ray",
        "NM" => "Nuclear medicine",
        "PET" => "PET",
        _ => return None,
    })
}

/// "TransducerTest" → "Transducer test", "UtilizationReports" → "Utilization reports"
fn words(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        if i > 0 && c.is_uppercase() && (chars[i - 1].is_lowercase() || next_lower) {
            out.push(' ');
        }
        out.push(c);
    }
    let mut it = out.split(' ');
    let first = it.next().unwrap_or("").to_owned();
    let rest: Vec<String> = it.map(|w| if w.chars().all(|c| c.is_uppercase()) { w.to_owned() } else { w.to_lowercase() }).collect();
    std::iter::once(first).chain(rest).collect::<Vec<_>>().join(" ")
}

/// Philips ultrasound product numbers (6 digits). One system can have several numbers
/// (options, refurbished "Circular Edition"…).
/// `true` = confirmed: seen in a log (MedicalSystem.xml) or as the product number on philips.com;
/// `false` = from a collected list, not confirmed (shown as "probably …").
const PRODUCTS: &[(&str, &str, bool)] = &[
    ("795210", "Affiniti 70", true),
    ("795119", "Affiniti 70 Circular Edition", true),
    ("795208", "Affiniti 50", true),
    ("795120", "Affiniti 50 Circular Edition", true),
    ("795218", "Affiniti 30", true),
    ("795121", "Affiniti 30", true),
    ("795190", "Affiniti CVx", true),
    ("795200", "EPIQ 7", true),
    ("795117", "EPIQ 7 Circular Edition", true),
    ("795204", "EPIQ 5", true),
    ("795118", "EPIQ 5", true),
    ("795098", "EPIQ Elite", true),
    ("795125", "EPIQ Elite", true),
    ("795231", "EPIQ CVx", true),
    ("795232", "EPIQ CVxi", true),
    ("795080", "ClearVue 350", true),
    ("795094", "ClearVue 550", true),
    ("795220", "ClearVue 650", true),
    ("795226", "ClearVue 850", true),
    ("795090", "Sparq", true),
    ("795005", "Lumify", true),
    ("795001", "InnoSight", true),
    ("795201", "EPIQ 7", false),
    ("795202", "EPIQ 7W", false),
    ("795206", "EPIQ 5W", false),
    ("795234", "EPIQ Elite", false),
    ("795101", "Xperius", false),
    ("795161", "Lumify (iOS)", false),
];

/// "Affiniti 70" or "probably EPIQ 7W"
pub fn product_name(id: &str) -> Option<String> {
    PRODUCTS.iter().find(|(n, _, _)| *n == id.trim()).map(|(_, name, sure)| if *sure { (*name).to_owned() } else { format!("probably {name}") })
}

/// "20260401115618" → "2026-04-01 11:56:18"
fn fmt_stamp(s: &str) -> Option<String> {
    if s.len() == 14 && s.bytes().all(|b| b.is_ascii_digit()) {
        Some(format!("{}-{}-{} {}:{}:{}", &s[0..4], &s[4..6], &s[6..8], &s[8..10], &s[10..12], &s[12..14]))
    } else if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        Some(format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8]))
    } else {
        None
    }
}

/// Information in Philips service file names, e.g.
/// `US_TransducerTest_795210_US225F1439_12.0.0.1280_20260401115618.enc`
/// = modality _ content _ product ID _ serial number _ software version _ date+time.
/// Returns (content title, rows).
pub fn name_fields(file_name: &str) -> (Option<String>, Vec<(String, String)>) {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = base.rsplit_once('.').map(|(a, b)| if b.len() <= 4 && !b.chars().all(|c| c.is_ascii_digit()) { a } else { base }).unwrap_or(base);
    // the version contains dots, so split the stem by '_' only
    let parts: Vec<&str> = stem.split('_').collect();
    let mut rows = Vec::new();
    if parts.len() < 3 {
        return (None, rows);
    }
    let Some(m) = modality(parts[0]) else { return (None, rows) };
    let title = words(parts[1]);
    rows.push(("Modality".to_owned(), format!("{m} ({})", parts[0])));
    rows.push(("Content".to_owned(), title.clone()));
    let mut rest: Vec<&str> = parts[2..].to_vec();
    // date/time is the last part with only digits
    if let Some(ts) = rest.last().and_then(|s| fmt_stamp(s)) {
        rest.pop();
        rows.push(("Date and time".to_owned(), ts));
    }
    if let Some(v) = rest.iter().position(|s| s.contains('.') && s.chars().all(|c| c.is_ascii_digit() || c == '.')) {
        rows.push(("Software version".to_owned(), rest.remove(v).to_owned()));
    }
    if let Some(pid) = rest.first().filter(|s| s.chars().all(|c| c.is_ascii_digit())) {
        let name = product_name(pid).map(|n| format!(" ({n})")).unwrap_or_default();
        rows.push(("Product ID".to_owned(), format!("{pid}{name}")));
        rest.remove(0);
    }
    if let Some(sn) = rest.first() {
        rows.push(("Serial number".to_owned(), (*sn).to_owned()));
    }
    // order: content, system, serial, software, date
    let order = ["Content", "Modality", "Product ID", "Serial number", "Software version", "Date and time"];
    rows.sort_by_key(|(k, _)| order.iter().position(|o| o == k).unwrap_or(99));
    (Some(title), rows)
}

fn entropy(b: &[u8]) -> f64 {
    let mut c = [0usize; 256];
    for &x in b {
        c[x as usize] += 1;
    }
    let n = b.len() as f64;
    c.iter().filter(|&&v| v > 0).map(|&v| {
        let p = v as f64 / n;
        -p * p.log2()
    }).sum()
}

/// random-looking content without any known file signature = encrypted
pub fn looks_encrypted(head: &[u8]) -> bool {
    if head.len() < 512 {
        return false;
    }
    let magic: [&[u8]; 12] = [b"PK", b"%PDF", b"<?xml", b"\x1f\x8b", b"\x89PNG", b"MZ", b"SQLite", b"\x00\x01\x00\x00Standard", b"\xff\xd8", b"7z\xbc\xaf", b"Rar!", b"ElfFile"];
    if magic.iter().any(|m| head.starts_with(m)) {
        return false;
    }
    entropy(head) > 7.9
}

fn encrypted_info(path: &Path, name: &str, size: u64, from_zip: bool) -> FileInfo {
    let (title, mut rows) = name_fields(name);
    let title = title.unwrap_or_else(|| "Encrypted file".to_owned());
    rows.push(("File".to_owned(), name.rsplit('/').next().unwrap_or(name).to_owned()));
    rows.push(("Size".to_owned(), fmt_size(size)));
    if from_zip {
        rows.push(("In archive".to_owned(), path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    let is_us = name.rsplit('/').next().is_some_and(|n| n.to_ascii_uppercase().starts_with("US_"));
    FileInfo {
        path: path.to_path_buf(),
        kind: format!("Encrypted {}{}", if is_us { "ultrasound " } else { "" }, title.to_lowercase()),
        title,
        rows,
        notice: Some(
            "This file cannot be opened: its content is encrypted by the system (no readable structure, random-looking data in blocks of 16 bytes, typical for AES). \
             It can only be read with the Philips service tools. The information shown here comes from the file name."
                .to_owned(),
        ),
        ..Default::default()
    }
}

/// what a document in an archive is (ultrasound utilization reports and others)
fn describe_doc(name: &str) -> String {
    let n = name.rsplit('/').next().unwrap_or(name);
    let stem = n.rsplit_once('.').map(|(a, _)| a).unwrap_or(n).to_ascii_lowercase();
    match stem.as_str() {
        "summary" => "Utilization summary – system usage, studies, users, transducer usage".into(),
        "systemusage" => "System usage per month – idle / in use hours".into(),
        "transducer" => "Transducer usage – studies and hours per transducer".into(),
        "study" => "Study report – studies and hours per user".into(),
        "referral" => "Referral report – studies per referring physician".into(),
        _ => crate::files::describe(name).map(str::to_owned).unwrap_or_default(),
    }
}

fn text_of<'a>(n: &'a XmlNode, path: &[&str]) -> Option<&'a XmlNode> {
    let mut cur = n;
    for p in path {
        cur = cur.children.iter().find(|c| c.name == *p)?;
    }
    Some(cur)
}

/// rows and part lists from an ultrasound `MedicalSystem.xml`
fn medical_system(doc: &XmlNode, info: &mut FileInfo) {
    let Some(ms) = doc.children.iter().find(|c| c.name == "MedicalSystem") else { return };
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut add = |k: &str, v: String| {
        if !v.trim().is_empty() && !rows.iter().any(|(kk, _)| kk == k) {
            rows.push((k.to_owned(), v.trim().to_owned()));
        }
    };
    if let Some(sc) = text_of(ms, &["SystemConfiguration"]) {
        if let Some(pi) = text_of(sc, &["ProductInfo"]) {
            add("Product", pi.attr("ProductFamily").unwrap_or("").to_owned());
            add("Model", pi.attr("Model").unwrap_or("").to_owned());
        }
        add("Serial number", sc.attr("SerialNumber").unwrap_or("").to_owned());
        let pid = sc.attr("ProductID").unwrap_or("");
        add("Product ID", pid.to_owned());
        if let Some(m) = sc.attr("Modality") {
            add("Modality", modality(m).map(|x| format!("{x} ({m})")).unwrap_or_else(|| m.to_owned()));
        }
        if let Some(sv) = text_of(sc, &["SoftwareVersion"]) {
            let v: Vec<&str> = ["Release", "Level", "Version", "Build"].iter().filter_map(|k| sv.attr(k)).collect();
            add("Software version", v.join("."));
        }
        if let Some(hw) = text_of(sc, &["Hardware"]) {
            for p in hw.children.iter().filter(|c| c.name == "HWPart") {
                let g = |k: &str| p.attr(k).unwrap_or("").to_owned();
                info.hardware.push([g("Type"), g("PartNumber"), g("SerialNumber"), g("Version")]);
            }
        }
        if let Some(sw) = text_of(sc, &["Software"]) {
            for p in sw.children.iter().filter(|c| c.name == "SWPart") {
                info.software.push([p.attr("Name").unwrap_or("").to_owned(), p.attr("InstallDate").unwrap_or("").replace('T', " ")]);
            }
        }
    }
    for cd in ms.children.iter().filter(|c| c.name == "CustomData") {
        let d = cd.attr("Description").unwrap_or("Custom data");
        let v = text_of(cd, &["Blob"]).map(|b| b.text.clone()).unwrap_or_default();
        add(if d.eq_ignore_ascii_case("Institution Name") { "Institution" } else { d }, v);
    }
    if !info.software.is_empty() {
        let last = info.software.iter().map(|s| s[1].as_str()).max().unwrap_or("").to_owned();
        add("Software installed", last);
    }
    // MedicalSystem.xml values replace what the file name says
    for (k, v) in rows.into_iter().rev() {
        info.rows.retain(|(kk, _)| *kk != k);
        info.rows.insert(0, (k, v));
    }
}

fn read_entry(zip: &mut zip::ZipArchive<BufReader<std::fs::File>>, index: usize, max: u64) -> Option<Vec<u8>> {
    let f = zip.by_index(index).ok()?;
    let mut v = Vec::new();
    f.take(max).read_to_end(&mut v).ok()?;
    Some(v)
}

/// Some(info) for a file that is not an event log the program can show: encrypted files, and ZIPs
/// without an event log. None = load it normally.
pub fn inspect_path(path: &Path) -> Option<FileInfo> {
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if crate::parser::is_zip(path) {
        let entries = crate::parser::list_zip(path).ok()?;
        let f = std::fs::File::open(path).ok()?;
        let mut zip = zip::ZipArchive::new(BufReader::new(f)).ok()?;
        let mut medical: Option<XmlNode> = None;
        for e in entries.iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".xml")) {
            let head = read_entry(&mut zip, e.index, 512 * 1024)?;
            if crate::parser::looks_like_events(&head[..]) {
                return None;
            }
            if medical.is_none() && head.windows(14).any(|w| w == b"<MedicalSystem") {
                medical = read_entry(&mut zip, e.index, 16 << 20).and_then(|b| xmltree::parse(&b[..]).ok());
            }
        }
        if entries.iter().any(|e| e.name.to_ascii_lowercase().ends_with(".mdb")) {
            // CT database inside: the normal loader handles it
            return None;
        }
        let (title, rows) = name_fields(&name);
        let mut info = FileInfo {
            path: path.to_path_buf(),
            title: title.clone().unwrap_or_else(|| "Archive".to_owned()),
            kind: match &title {
                Some(t) => format!("{} – archive without an event log", t),
                None => "Archive without an event log".to_owned(),
            },
            rows,
            notice: Some("This archive contains no event log, so there is nothing to show in the table. The system information and the documents it contains are listed here.".to_owned()),
            ..Default::default()
        };
        if let Some(doc) = &medical {
            medical_system(doc, &mut info);
        }
        info.rows.push(("File".to_owned(), name.clone()));
        info.rows.push(("Size".to_owned(), format!("{} · {} files", fmt_size(size), entries.len())));
        let mut docs: Vec<Document> = entries.iter().filter(|e| e.size > 0).map(|e| Document { entry: e.clone(), description: describe_doc(&e.name) }).collect();
        // PDFs first, in a useful order
        let rank = |d: &Document| {
            let n = d.entry.name.to_ascii_lowercase();
            let pos = ["summary", "systemusage", "transducer", "study", "referral"].iter().position(|k| n.rsplit('/').next().unwrap_or(&n).starts_with(k)).unwrap_or(9);
            (if n.ends_with(".pdf") { 0 } else { 1 }, pos, n)
        };
        docs.sort_by_key(rank);
        info.documents = docs;
        return Some(info);
    }
    // any other file: encrypted?
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 256 * 1024];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    looks_encrypted(&head).then(|| encrypted_info(path, &name, size, false))
}

/// Some(info) when a file from a ZIP is encrypted
pub fn inspect_entry(zip_path: &Path, entry: &ArchiveEntry, head: &[u8]) -> Option<FileInfo> {
    looks_encrypted(head).then(|| encrypted_info(zip_path, &entry.name, entry.size, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ultrasound_names() {
        let (t, r) = name_fields("US_TransducerTest_795210_US225F1439_12.0.0.1280_20260401115618.enc");
        assert_eq!(t.as_deref(), Some("Transducer test"));
        let get = |k: &str| r.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(get("Serial number"), "US225F1439");
        assert_eq!(get("Software version"), "12.0.0.1280");
        assert_eq!(get("Date and time"), "2026-04-01 11:56:18");
        assert_eq!(get("Product ID"), "795210 (Affiniti 70)");
        assert_eq!(get("Modality"), "Ultrasound (US)");
        let (t, _) = name_fields("US_UtilizationReports_795210_US225F1439_12.0.0.1280_20260401115037.zip");
        assert_eq!(t.as_deref(), Some("Utilization reports"));
        assert!(name_fields("SystemLogs.xml").0.is_none());
        assert_eq!(product_name("795202").as_deref(), Some("probably EPIQ 7W"));
        assert_eq!(product_name("795120").as_deref(), Some("Affiniti 50 Circular Edition"));
        assert!(product_name("123456").is_none());
    }
    #[test]
    fn encrypted_detection() {
        let mut x: u64 = 88172645463325252;
        let rnd: Vec<u8> = (0..4096)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        assert!(looks_encrypted(&rnd));
        assert!(!looks_encrypted(&[b'a'; 4096]));
        let mut pdf = b"%PDF-1.4".to_vec();
        pdf.extend(&rnd);
        assert!(!looks_encrypted(&pdf));
    }
}
