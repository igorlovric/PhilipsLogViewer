//! Shared pattern library on the PLV server (see server/ in the repository).
//! Network part only – runs on a background thread; the UI is in share_ui.rs.

use serde::Deserialize;
use std::path::PathBuf;

/// built-in server; can be overridden with "share_server" in PhilipsLogViewer.config
pub const DEFAULT_SERVER: &str = "https://plv.shadowdreams.net";
/// built-in API key, injected at build time from the GitHub secret PLV_API_KEY;
/// can be overridden with "share_api_key" in PhilipsLogViewer.config
pub const BUILT_IN_KEY: &str = match option_env!("PLV_API_KEY") {
    Some(k) => k,
    None => "",
};

#[derive(Clone)]
pub struct Endpoint {
    pub base: String,
    pub key: String,
    pub admin: String,
    pub owner: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RemotePattern {
    pub uid: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub name: String,
    pub checksum: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub mine: bool,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub data: String,
}

pub struct Upload {
    pub uid: String,
    pub author: String,
    pub data: String,
    pub local_checksum: String,
}

pub struct Job {
    pub ep: Endpoint,
    pub uploads: Vec<Upload>,
    pub withdraw: Vec<String>,
}

pub enum UploadStatus {
    /// (local checksum sent, checksum the server stored)
    Done(String, String),
    /// the uid belongs to someone else's pattern
    Forbidden,
    Failed(String),
}

#[derive(Default)]
pub struct SyncResult {
    pub uploaded: Vec<(String, UploadStatus)>,
    pub withdrawn: Vec<String>,
    pub remote: Option<Vec<RemotePattern>>,
    pub error: Option<String>,
}

/// file with the secret key that identifies this installation as the owner of its patterns.
/// Not in PhilipsLogViewer.config on purpose: that file is given to colleagues.
fn owner_file() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("PhilipsLogViewer").join("owner.key")
}

pub fn owner_key() -> String {
    let f = owner_file();
    if let Ok(s) = std::fs::read_to_string(&f) {
        let s = s.trim().to_owned();
        if s.len() >= 32 {
            return s;
        }
    }
    let k = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&f, &k);
    k
}

fn req(ep: &Endpoint, method: &str, path: &str) -> ureq::Request {
    let mut r = crate::update::agent().request(method, &format!("{}/v1/{}", ep.base.trim_end_matches('/'), path)).set("X-PLV-Owner", &ep.owner);
    if !ep.key.is_empty() {
        r = r.set("X-PLV-Key", &ep.key);
    }
    if !ep.admin.is_empty() {
        r = r.set("X-PLV-Admin", &ep.admin);
    }
    r
}

/// readable error text from a failed request (the server answers {"ok":false,"error":"..."})
fn err_text(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let msg = resp.into_string().ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|v| v["error"].as_str().map(str::to_owned));
            format!("server answered {code}{}", msg.map(|m| format!(": {m}")).unwrap_or_default())
        }
        ureq::Error::Transport(t) => format!("server not reachable ({})", t.kind()),
    }
}

pub fn fetch(ep: &Endpoint) -> Result<Vec<RemotePattern>, String> {
    let resp = req(ep, "GET", "patterns").call().map_err(err_text)?;
    let v: serde_json::Value = serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())?;
    serde_json::from_value(v["patterns"].clone()).map_err(|e| e.to_string())
}

pub fn withdraw(ep: &Endpoint, uid: &str) -> Result<(), String> {
    match req(ep, "DELETE", &format!("patterns/{uid}")).call() {
        Ok(_) => Ok(()),
        // already gone
        Err(ureq::Error::Status(404, _)) => Ok(()),
        Err(e) => Err(err_text(e)),
    }
}

fn upload(ep: &Endpoint, ups: &[Upload]) -> Result<Vec<(String, UploadStatus)>, String> {
    let body = serde_json::json!({
        "patterns": ups.iter().map(|u| serde_json::json!({"uid": u.uid, "author": u.author, "data": u.data})).collect::<Vec<_>>()
    });
    let resp = req(ep, "POST", "patterns").send_json(body).map_err(err_text)?;
    let v: serde_json::Value = serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in v["results"].as_array().cloned().unwrap_or_default() {
        let uid = r["uid"].as_str().unwrap_or_default().to_owned();
        let Some(u) = ups.iter().find(|u| u.uid == uid) else { continue };
        let st = match r["status"].as_str().unwrap_or_default() {
            "created" | "updated" | "unchanged" => UploadStatus::Done(u.local_checksum.clone(), r["checksum"].as_str().unwrap_or_default().to_owned()),
            "forbidden" => UploadStatus::Forbidden,
            _ => UploadStatus::Failed(r["error"].as_str().unwrap_or("rejected").to_owned()),
        };
        out.push((uid, st));
    }
    Ok(out)
}

/// withdraws, uploads, then fetches the whole library
pub fn run(job: Job) -> SyncResult {
    let mut res = SyncResult::default();
    for uid in &job.withdraw {
        match withdraw(&job.ep, uid) {
            Ok(()) => res.withdrawn.push(uid.clone()),
            Err(e) => {
                res.error = Some(e);
                return res;
            }
        }
    }
    for chunk in job.uploads.chunks(100) {
        match upload(&job.ep, chunk) {
            Ok(r) => res.uploaded.extend(r),
            Err(e) => {
                res.error = Some(e);
                return res;
            }
        }
    }
    match fetch(&job.ep) {
        Ok(r) => res.remote = Some(r),
        Err(e) => res.error = Some(e),
    }
    res
}
