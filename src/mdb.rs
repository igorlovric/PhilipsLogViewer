//! CT event logs exported as an Access database (Log_Database_Export_*.mdb).
//!
//! Tables: Event_Index (time, event), Event_Def (event name, type), Event_Type_Def (type name),
//! Event_Detail (key = value pairs per event). Read with the pure-Rust `jetdb` crate.

use crate::model::*;
use crate::parser::{self, RawEvent};
use jetdb::{read_catalog, read_table_def, read_table_rows, ObjectType, PageReader, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Access / Jet signature in the first page.
pub fn is_mdb(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "mdb" || ext == "accdb" {
        return true;
    }
    let mut head = [0u8; 32];
    use std::io::Read;
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && (head[4..].starts_with(b"Standard Jet DB") || head[4..].starts_with(b"Standard ACE DB"))
}

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Text(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Byte(x) => x.to_string(),
        Value::Int(x) => x.to_string(),
        Value::Long(x) => x.to_string(),
        Value::BigInt(x) => x.to_string(),
        Value::Float(x) => x.to_string(),
        Value::Double(x) => x.to_string(),
        Value::Timestamp(d) => fmt_ts(ole_to_ms(*d)),
        other => format!("{other:?}"),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Byte(x) => *x as i64,
        Value::Int(x) => *x as i64,
        Value::Long(x) => *x as i64,
        Value::BigInt(x) => *x,
        Value::Bool(b) => *b as i64,
        Value::Text(s) => s.trim().parse().unwrap_or(0),
        Value::Double(x) => *x as i64,
        _ => 0,
    }
}

/// OLE date (days since 1899-12-30) -> ms since 1970-01-01
fn ole_to_ms(d: f64) -> i64 {
    ((d - 25569.0) * 86_400_000.0).round() as i64
}

/// "2024-02-05  10:32:46.873" (one or more spaces)
fn parse_stime(s: &str) -> Option<i64> {
    let mut it = s.split_whitespace();
    let date = it.next()?;
    let time = it.next()?;
    if date.len() != 10 || time.len() < 8 {
        return None;
    }
    let base = parse_ts(&format!("{date}T{}", &time[..8]));
    let ms = time.get(9..).map(|f| {
        let f: String = f.chars().take(3).collect();
        format!("{f:0<3}").parse::<i64>().unwrap_or(0)
    });
    (base != 0).then(|| base + ms.unwrap_or(0))
}

struct Table {
    cols: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl Table {
    fn col(&self, name: &str) -> Option<usize> {
        self.cols.iter().position(|c| c.eq_ignore_ascii_case(name))
    }
}

fn read_tables(path: &Path) -> Result<HashMap<String, Table>, String> {
    let mut r = PageReader::open(path).map_err(|e| format!("Cannot open the Access database: {e}"))?;
    let cat = read_catalog(&mut r).map_err(|e| format!("Cannot read the database catalog: {e}"))?;
    let mut out = HashMap::new();
    for e in &cat {
        if e.name.starts_with("MSys") || !matches!(e.object_type, ObjectType::Table) {
            continue;
        }
        let td = read_table_def(&mut r, &e.name, e.table_page).map_err(|x| format!("Table {}: {x}", e.name))?;
        let rs = read_table_rows(&mut r, &td).map_err(|x| format!("Table {}: {x}", e.name))?;
        out.insert(e.name.clone(), Table { cols: td.columns.iter().map(|c| c.name.clone()).collect(), rows: rs.rows });
    }
    Ok(out)
}

/// Loads a CT log database into the normal event model.
pub fn load(path: &Path) -> Result<LogData, String> {
    let t0 = Instant::now();
    let tables = read_tables(path)?;
    let get = |n: &str| tables.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v);
    let (Some(index), Some(def)) = (get("Event_Index"), get("Event_Def")) else {
        let mut names: Vec<&String> = tables.keys().collect();
        names.sort();
        return Err(format!(
            "This Access database is not a known event log (expected tables Event_Index and Event_Def, found: {})",
            names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    };

    // event types
    let mut types: HashMap<i64, String> = HashMap::new();
    if let Some(t) = get("Event_Type_Def") {
        if let (Some(i), Some(n)) = (t.col("Event_Type_ID"), t.col("Event_Type_Name")) {
            for r in &t.rows {
                types.insert(int(&r[i]), text(&r[n]));
            }
        }
    }
    // event definitions: id -> (type id, name, description)
    let mut defs: HashMap<i64, (i64, String, String)> = HashMap::new();
    {
        let (i, ty, n, d) = (def.col("Event_ID"), def.col("Event_Type_ID"), def.col("Event_Name"), def.col("Event_Description"));
        for r in &def.rows {
            let id = i.map(|c| int(&r[c])).unwrap_or(0);
            defs.insert(
                id,
                (ty.map(|c| int(&r[c])).unwrap_or(0), n.map(|c| text(&r[c])).unwrap_or_default(), d.map(|c| text(&r[c])).unwrap_or_default()),
            );
        }
    }
    // details: uid -> [(sub index, key, value, show as key)]
    let mut details: HashMap<i64, Vec<(i64, String, String, bool)>> = HashMap::new();
    if let Some(t) = get("Event_Detail") {
        let (u, v, s, si, k) = (t.col("Event_UID"), t.col("Key_Value"), t.col("ShowAsKey"), t.col("SubIndex"), t.col("Key_Name"));
        for r in &t.rows {
            let uid = u.map(|c| int(&r[c])).unwrap_or(0);
            details.entry(uid).or_default().push((
                si.map(|c| int(&r[c])).unwrap_or(0),
                k.map(|c| text(&r[c])).unwrap_or_default().trim().to_owned(),
                v.map(|c| text(&r[c])).unwrap_or_default().trim().to_owned(),
                s.map(|c| int(&r[c]) != 0).unwrap_or(false),
            ));
        }
    }

    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut log = parser::new_log(path, name);
    log.info.family = "CT".into();
    log.info.modality = "CT".into();
    let (ui, ti, ei, si) = (index.col("Event_UID"), index.col("Event_Time"), index.col("Event_ID"), index.col("Event_STime"));
    let mut philips = false;
    for r in &index.rows {
        let uid = ui.map(|c| int(&r[c])).unwrap_or(0);
        let eid = ei.map(|c| int(&r[c])).unwrap_or(0);
        let ts = si
            .and_then(|c| parse_stime(&text(&r[c])))
            .or_else(|| ti.and_then(|c| if let Value::Timestamp(d) = r[c] { Some(ole_to_ms(d)) } else { None }))
            .unwrap_or(0);
        let (type_id, ev_name, ev_desc) = defs.get(&eid).cloned().unwrap_or((0, format!("Event {eid}"), String::new()));
        let type_name = types.get(&type_id).cloned().unwrap_or_else(|| if type_id == 0 { String::new() } else { format!("Type {type_id}") });
        let mut det = details.remove(&uid).unwrap_or_default();
        det.sort_by_key(|d| d.0);

        let is_error = type_id == 1 || type_name.to_ascii_lowercase().contains("error");
        // keys shown "as key" with an empty value are error codes (e.g. RS00000010025)
        let codes: Vec<String> = det.iter().filter(|d| d.3 && d.2.is_empty() && !d.1.is_empty()).map(|d| d.1.clone()).collect();
        let mut msg = String::new();
        for (_, k, v, as_key) in &det {
            if *as_key && v.is_empty() {
                continue;
            }
            if !philips && v.starts_with("1.3.46.670589.") {
                philips = true;
            }
            let v = v.replace(['{', '}'], "").replace("\r\n", " ").replace('\n', " ");
            msg.push_str(&format!("{{{} = {}}} ", if k.is_empty() { "?" } else { k }, v.trim()));
        }
        let mut e = RawEvent {
            index: uid,
            ts,
            category: if is_error { "Error".into() } else { "Information".into() },
            log_cat: type_name,
            event_id: if let Some(c) = codes.first() { c.clone() } else { eid.to_string() },
            description: if codes.is_empty() { ev_name.clone() } else { format!("{ev_name} {}", codes.join(", ")) },
            message: msg.trim_end().to_owned(),
            ..Default::default()
        };
        if !ev_desc.is_empty() && ev_desc != ev_name {
            e.extras.push(("Event Description".into(), ev_desc));
        }
        if codes.len() > 1 {
            e.extras.push(("Error Codes".into(), codes.join(", ")));
        }
        e.extras.push(("Event Number".into(), eid.to_string()));
        parser::push_event(&mut log, e);
    }
    if log.events.is_empty() {
        return Err("The event log in this database is empty".into());
    }
    if philips {
        log.info.family = "Philips CT".into();
    }
    log.info.version = String::new();
    log.kind = "CT log database".into();
    log.finalize();
    log.load_ms = t0.elapsed().as_millis();
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stime() {
        assert_eq!(fmt_ts(parse_stime("2024-02-05  10:32:46.873").unwrap()), "2024-02-05 10:32:46.873");
        assert_eq!(fmt_ts(ole_to_ms(45327.439421296294)), "2024-02-05 10:32:46.000");
    }

    #[test]
    fn real_mdb() {
        let Ok(p) = std::env::var("MDBFILE") else { return };
        let t = Instant::now();
        let log = load(Path::new(&p)).unwrap();
        println!("{} events in {:?}, counts {:?}, {} -> {}", log.events.len(), t.elapsed(), log.counts, fmt_ts(log.t_min), fmt_ts(log.t_max));
        for e in log.events.iter().filter(|e| e.severity == Severity::Error).take(3) {
            println!("  {} | {} | {} | {}", fmt_ts(e.ts), log.pool.get(e.event_id), log.pool.get(e.description), e.message);
        }
        for e in log.events.iter().take(4) {
            println!("  {} | {} | {} | {}", fmt_ts(e.ts), log.pool.get(e.log_cat), log.pool.get(e.description), &e.message[..e.message.len().min(160)]);
        }
        assert!(log.counts[Severity::Error as usize] > 0);
    }
}
