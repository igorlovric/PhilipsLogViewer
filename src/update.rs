//! Update check and self-update from the GitHub releases of the project.
//!
//! The release workflow publishes `PhilipsLogViewer.exe` and `PhilipsLogViewer.exe.sha256`.
//! Updating: download next to the running exe (`<exe>.new`), verify the SHA-256,
//! rename the running exe to `<exe>.old` (Windows allows renaming a running exe),
//! move the new one to the original name and start it. `<exe>.old` is removed on the next start.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const REPO: &str = "igorlovric/PhilipsLogViewer";
const EXE_ASSET: &str = "PhilipsLogViewer.exe";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub exe_url: String,
    pub sha_url: Option<String>,
    pub size: u64,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("PhilipsLogViewer/{}", crate::version::VERSION))
        .try_proxy_from_env(true)
        .build()
}

/// "v1.10.2" → (1, 10, 2)
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(['.', '-', '+']).map(|p| p.parse::<u32>());
    let a = it.next()?.ok()?;
    let b = it.next().and_then(|r| r.ok()).unwrap_or(0);
    let c = it.next().and_then(|r| r.ok()).unwrap_or(0);
    Some((a, b, c))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

/// latest published release (drafts and pre-releases are not returned by GitHub)
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = agent().get(&url).set("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |name: &str| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let exe = find(EXE_ASSET)
        .or_else(|| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".exe"))))
        .ok_or("the release has no exe")?;
    let sha = find(&format!("{EXE_ASSET}.sha256"));
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_owned(),
        notes: v["body"].as_str().unwrap_or_default().trim().to_owned(),
        page: v["html_url"].as_str().unwrap_or_default().to_owned(),
        exe_url: exe["browser_download_url"].as_str().unwrap_or_default().to_owned(),
        sha_url: sha.and_then(|a| a["browser_download_url"].as_str()).map(str::to_owned),
        size: exe["size"].as_u64().unwrap_or(0),
    })
}

pub fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn new_path(exe: &Path) -> PathBuf {
    with_suffix(exe, ".new")
}

fn old_path(exe: &Path) -> PathBuf {
    with_suffix(exe, ".old")
}

/// removes what an earlier update left behind (the replaced exe, an unfinished download)
pub fn cleanup() {
    if let Ok(exe) = current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(new_path(&exe));
    }
}

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// downloads the exe of `rel` to `<exe>.new` and checks its SHA-256; returns the path of the new file
pub fn download(rel: &Release, prog: &Arc<Progress>) -> Result<PathBuf, String> {
    let exe = current_exe()?;
    let target = new_path(&exe);
    let ag = agent();
    let expected = match &rel.sha_url {
        Some(u) => {
            let s = ag.get(u).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
            Some(s.split_whitespace().next().unwrap_or_default().to_ascii_lowercase())
        }
        None => None,
    };
    let resp = ag.get(&rel.exe_url).call().map_err(|e| e.to_string())?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(rel.size);
    prog.total.store(total, Ordering::Relaxed);
    let mut rd = resp.into_reader();
    let mut f = std::fs::File::create(&target).map_err(|e| format!("cannot write next to the program ({}): {e}", target.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let res = (|| {
        loop {
            if prog.cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_owned());
            }
            let n = rd.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            hasher.update(&buf[..n]);
            done += n as u64;
            prog.done.store(done, Ordering::Relaxed);
        }
        f.flush().map_err(|e| e.to_string())?;
        if total > 0 && done != total {
            return Err(format!("download incomplete ({done} of {total} bytes)"));
        }
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if let Some(exp) = &expected {
            if *exp != got {
                return Err("the downloaded file is damaged (checksum does not match)".to_owned());
            }
        }
        Ok(())
    })();
    drop(f);
    if let Err(e) = res {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(target)
}

/// puts the downloaded exe in place of the running one (the running one becomes `<exe>.old`)
pub fn install(new_file: &Path) -> Result<PathBuf, String> {
    let exe = current_exe()?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("cannot replace the program: {e}"))?;
    if let Err(e) = std::fs::rename(new_file, &exe) {
        // put the old one back
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("cannot replace the program: {e}"));
    }
    Ok(exe)
}

/// starts the (new) program, optionally with a log to open
pub fn restart(exe: &Path, log: Option<&Path>) -> Result<(), String> {
    let mut c = std::process::Command::new(exe);
    if let Some(l) = log {
        c.arg(l);
    }
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("v1.7.0", "1.6.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(!is_newer("1.6.0", "1.6.0"));
        assert!(!is_newer("v1.5.9", "1.6.0"));
        assert!(!is_newer("garbage", "1.6.0"));
        assert_eq!(parse_version("v2"), Some((2, 0, 0)));
    }
}
