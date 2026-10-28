//! Global config (next to the exe) and per-log companion file (next to the log).
//! Both are pretty-printed JSON so they are easy to read and to copy to a colleague.

use crate::app::Col;
use crate::patterns::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "PhilipsLogViewer.config";
/// name used up to 1.7.0 – renamed to CONFIG_FILE when found
pub const OLD_CONFIG_FILE: &str = "philips-log-viewer.config.json";
pub const LOG_EXT: &str = "plv.json";

// ------------------------------------------------------------------ global config

fn default_columns() -> Vec<Col> {
    Col::ALL.iter().copied().filter(|c| c.default_visible()).collect()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GlobalConfig {
    #[serde(default = "default_columns")]
    pub visible_columns: Vec<Col>,
    #[serde(default)]
    pub last_dir: Option<PathBuf>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    /// columns the program knew when the config was saved (new ones get switched on)
    #[serde(default)]
    pub known_columns: Vec<Col>,
    #[serde(default)]
    pub details_dock: DetailsDock,
    #[serde(default = "default_details_size")]
    pub details_size: f32,
    #[serde(default = "default_details_width")]
    pub details_width: f32,
    #[serde(default)]
    pub details_raw: bool,
    /// program version that last wrote this file (for "What's new")
    #[serde(default)]
    pub app_version: String,
    /// descriptions of error codes / event IDs (shared with colleagues through this file)
    #[serde(default)]
    pub codes: BTreeMap<String, CodeNote>,
    /// chart series the user picked ("event name › key"), reused for the next log
    #[serde(default)]
    pub chart_series: Vec<String>,
    /// visible columns per kind of log (e.g. "CT log database"); kinds not listed use visible_columns
    #[serde(default)]
    pub column_sets: BTreeMap<String, Vec<Col>>,
    /// row background per event category (None = no color); new categories are added when a log contains them
    #[serde(default = "default_category_colors")]
    pub category_colors: Vec<CategoryColor>,
    /// background of selected rows
    #[serde(default = "default_selected_color")]
    pub selected_row_color: [u8; 3],
    /// look for a new version on GitHub at startup
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// download a new version in the background and install it when the program closes
    #[serde(default)]
    pub auto_update: bool,
    /// version the user chose not to be offered again
    #[serde(default)]
    pub skipped_version: String,
    /// name shown with shared patterns (default: Windows user name, confirmed at first start)
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub author_asked: bool,
    /// sync the shared pattern library at startup and after changes
    #[serde(default = "yes")]
    pub share_sync: bool,
    /// override of the built-in server address (empty = built-in), e.g. for testing
    #[serde(default)]
    pub share_server: String,
    /// override of the built-in API key (empty = built-in)
    #[serde(default)]
    pub share_api_key: String,
    /// admin key: allows withdrawing any shared pattern
    #[serde(default)]
    pub share_admin_key: String,
    /// own patterns to withdraw from the server at the next sync (unshared or deleted)
    #[serde(default)]
    pub share_pending_withdraw: Vec<String>,
    /// shared patterns of colleagues already seen in the "Available" list
    #[serde(default)]
    pub share_seen: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CategoryColor {
    pub name: String,
    #[serde(default)]
    pub color: Option<[u8; 3]>,
}

pub fn default_selected_color() -> [u8; 3] {
    [0xc4, 0xdc, 0xff]
}

/// the colors the program has always used
pub fn default_category_colors() -> Vec<CategoryColor> {
    [
        ("Fatal", Some([0xdc, 0xb0, 0xff])),
        ("Error", Some([0xff, 0xa3, 0xa3])),
        ("Warning", Some([0xff, 0xe8, 0x7a])),
        ("Information", Some([0xe2, 0xf7, 0xe2])),
        ("UserMessage", Some([0x8c, 0xcf, 0x9c])),
        ("Command", None),
    ]
    .into_iter()
    .map(|(n, c)| CategoryColor { name: n.to_owned(), color: c })
    .collect()
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct CodeNote {
    pub description: String,
    #[serde(default)]
    pub comment: String,
}

fn default_details_width() -> f32 {
    620.0
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DetailsDock {
    #[default]
    Bottom,
    Left,
    Right,
    Window,
}

fn default_details_size() -> f32 {
    220.0
}

impl Default for GlobalConfig {
    fn default() -> Self {
        GlobalConfig {
            visible_columns: default_columns(),
            last_dir: None,
            patterns: vec![],
            known_columns: Col::ALL.to_vec(),
            details_dock: DetailsDock::Bottom,
            details_size: default_details_size(),
            details_width: default_details_width(),
            details_raw: false,
            app_version: String::new(),
            codes: BTreeMap::new(),
            chart_series: vec![],
            column_sets: BTreeMap::new(),
            category_colors: default_category_colors(),
            selected_row_color: default_selected_color(),
            check_updates: true,
            auto_update: false,
            skipped_version: String::new(),
            author_name: String::new(),
            author_asked: false,
            share_sync: true,
            share_server: String::new(),
            share_api_key: String::new(),
            share_admin_key: String::new(),
            share_pending_withdraw: vec![],
            share_seen: vec![],
        }
    }
}

fn exe_dir_config() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|d| d.join(CONFIG_FILE))
}

fn appdata_config() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("PhilipsLogViewer").join(CONFIG_FILE)
}

/// Config lives next to the exe; if that folder is not writable (e.g. Program Files)
/// it falls back to %APPDATA%\PhilipsLogViewer.
pub fn config_path() -> PathBuf {
    if let Some(p) = exe_dir_config() {
        if p.exists() {
            return p;
        }
        if let Some(dir) = p.parent() {
            let probe = dir.join(".plv_write_test");
            if std::fs::write(&probe, b"").is_ok() {
                let _ = std::fs::remove_file(&probe);
                return p;
            }
        }
    }
    appdata_config()
}

/// renames a config with the old name (next to the exe and in %APPDATA%) to the new name;
/// returns an old file that could not be renamed (read-only folder), to be read instead
fn migrate_old_config() -> Option<PathBuf> {
    let mut unrenamed = None;
    for new in exe_dir_config().into_iter().chain([appdata_config()]) {
        let old = new.with_file_name(OLD_CONFIG_FILE);
        if old.exists() && !new.exists() && std::fs::rename(&old, &new).is_err() && unrenamed.is_none() {
            unrenamed = Some(old);
        }
    }
    unrenamed
}

pub fn load_config() -> GlobalConfig {
    let old = migrate_old_config();
    let p = config_path();
    let p = if p.exists() {
        p
    } else if appdata_config().exists() {
        appdata_config()
    } else {
        old.unwrap_or(p)
    };
    let mut c: GlobalConfig = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let known = if c.known_columns.is_empty() {
        // configs from before Component/Package existed
        Col::ALL.iter().copied().filter(|x| !matches!(x, Col::Component | Col::Package)).collect()
    } else {
        c.known_columns.clone()
    };
    for col in Col::ALL {
        if !known.contains(&col) && col.default_visible() && !c.visible_columns.contains(&col) {
            c.visible_columns.push(col);
        }
    }
    c.visible_columns = Col::ALL.iter().copied().filter(|x| c.visible_columns.contains(x)).collect();
    c.known_columns = Col::ALL.to_vec();
    c
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn save_config(c: &GlobalConfig) -> Result<(), String> {
    write_json(&config_path(), c)
}

// ------------------------------------------------------------------ per-log file

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BookmarkRec {
    /// value of the Index attribute in the log
    pub index: i64,
    pub time: String,
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub comment: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ViewState {
    #[serde(default)]
    pub sort_column: Option<Col>,
    #[serde(default = "yes")]
    pub sort_ascending: bool,
    /// event categories switched off (e.g. "Information")
    #[serde(default)]
    pub hidden_categories: Vec<String>,
    /// column -> text filter
    #[serde(default)]
    pub text_filters: BTreeMap<String, String>,
    /// column -> selected values
    #[serde(default)]
    pub value_filters: BTreeMap<String, Vec<String>>,
    /// [from, to] as "YYYY-MM-DD HH:MM:SS.mmm"
    #[serde(default)]
    pub time_range: Option<[String; 2]>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub search_only: bool,
    #[serde(default)]
    pub bookmarks_only: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LogFileData {
    /// program version that wrote this file
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub log_file: String,
    #[serde(default)]
    pub saved: String,
    /// free notes about this log
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub bookmarks: Vec<BookmarkRec>,
    #[serde(default)]
    pub view: ViewState,
}

pub fn load_log_data(file: &Path) -> Option<LogFileData> {
    let s = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn save_log_data(file: &Path, d: &LogFileData) -> Result<(), String> {
    write_json(file, d)
}
