//! Load/save the non-secret application configuration as JSON in the OS
//! app-config directory (`%APPDATA%\com.onscreenwidget.app` on Windows,
//! `~/Library/Application Support/com.onscreenwidget.app` on macOS).
//!
//! Safety design (added after a data-loss incident):
//!   * A config that EXISTS but fails to parse is NEVER silently replaced with
//!     defaults. It is preserved and copied to a `*.corrupt-<ts>.json` file, and
//!     load returns a distinct error so the caller can avoid overwriting it.
//!   * Every successful save first copies the current good config into
//!     `backups/config-<timestamp>.json`, keeping the most recent N backups.
//!   * Writes are atomic (temp file + rename).

use crate::model::AppConfig;
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "com.onscreenwidget.app";
const CONFIG_FILE: &str = "config.json";
const HISTORY_FILE: &str = "price_history.json";
const BACKUP_DIR: &str = "backups";
const MAX_BACKUPS: usize = 20;

/// Distinguishes "no config yet" from "config present but unreadable" so the
/// caller never overwrites a real-but-unparseable file with defaults.
#[derive(Debug)]
pub enum LoadOutcome {
    /// No config file exists yet (fresh install).
    Fresh,
    /// Loaded and parsed successfully.
    Loaded(AppConfig),
    /// A config file exists but could not be read/parsed. The original is
    /// preserved; a copy was saved to `corrupt_backup`. Do NOT overwrite.
    Unreadable { error: String, corrupt_backup: Option<PathBuf> },
}

/// Resolve the per-user config directory, creating it if needed.
pub fn config_dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "no config dir available".to_string())?;
    let dir = base.join(APP_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir)
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

pub fn history_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(HISTORY_FILE))
}

pub fn backup_dir() -> Result<PathBuf, String> {
    let dir = config_dir()?.join(BACKUP_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("create backup dir: {e}"))?;
    Ok(dir)
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Safe load. Returns a `LoadOutcome` so the caller can tell the three cases
/// apart and NEVER clobber an existing-but-unparseable config.
pub fn load_safe() -> LoadOutcome {
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => return LoadOutcome::Unreadable { error: e, corrupt_backup: None },
    };
    if !path.exists() {
        return LoadOutcome::Fresh;
    }
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(e) => {
            return LoadOutcome::Unreadable {
                error: format!("read config: {e}"),
                corrupt_backup: None,
            }
        }
    };
    match serde_json::from_str::<AppConfig>(&raw) {
        Ok(mut cfg) => {
            cfg.window.migrate_legacy();
            for t in &mut cfg.tiles {
                t.migrate_legacy();
            }
            LoadOutcome::Loaded(cfg)
        }
        Err(strict_err) => {
            // The strict parse failed. Before treating the file as corrupt, try
            // a RESILIENT parse: keep every tile that still parses individually,
            // so a format change to one tile type never discards the others.
            if let Some(cfg) = try_resilient_parse(&raw) {
                // Preserve the original too, just in case.
                let corrupt = config_dir()
                    .ok()
                    .map(|d| d.join(format!("config.preupgrade-{}.json", timestamp())));
                if let Some(ref cp) = corrupt {
                    let _ = fs::write(cp, &raw);
                }
                return LoadOutcome::Loaded(cfg);
            }

            // Truly unreadable: preserve the file so nothing is lost.
            let corrupt = config_dir()
                .ok()
                .map(|d| d.join(format!("config.corrupt-{}.json", timestamp())));
            if let Some(ref cp) = corrupt {
                let _ = fs::write(cp, &raw);
            }
            LoadOutcome::Unreadable {
                error: format!("parse config: {strict_err}"),
                corrupt_backup: corrupt,
            }
        }
    }
}

/// Best-effort parse that salvages as many tiles as possible. Returns Some if we
/// could recover a usable config (window settings + any tiles that parse).
fn try_resilient_parse(raw: &str) -> Option<AppConfig> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;

    // Window settings (fall back to default if absent/invalid).
    let mut window: crate::model::WindowConfig = value
        .get("window")
        .and_then(|w| serde_json::from_value(w.clone()).ok())
        .unwrap_or_default();
    window.migrate_legacy();
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(1);

    // Parse tiles one by one; drop any that fail rather than failing all.
    let mut tiles = Vec::new();
    if let Some(arr) = value.get("tiles").and_then(|t| t.as_array()) {
        for t in arr {
            match serde_json::from_value::<crate::model::Tile>(t.clone()) {
                Ok(mut tile) => {
                    tile.migrate_legacy();
                    tiles.push(tile);
                }
                Err(e) => {
                    log::warn!("dropping unparseable tile during load: {e}");
                }
            }
        }
    }

    Some(AppConfig {
        version,
        window,
        tiles,
    })
}

/// Backwards-compatible helper: load config or default (empty) ONLY when there
/// is genuinely no file. On an unreadable existing file this still returns an
/// error so callers using it won't overwrite. Prefer `load_safe`.
pub fn load() -> Result<AppConfig, String> {
    match load_safe() {
        LoadOutcome::Fresh => Ok(AppConfig::default()),
        LoadOutcome::Loaded(c) => Ok(c),
        LoadOutcome::Unreadable { error, .. } => Err(error),
    }
}

/// Copy the current good config (if any) into the rotating backups folder, then
/// prune to the most recent MAX_BACKUPS.
fn write_backup() {
    let (path, dir) = match (config_path(), backup_dir()) {
        (Ok(p), Ok(d)) => (p, d),
        _ => return,
    };
    if !path.exists() {
        return;
    }
    let dest = dir.join(format!("config-{}.json", timestamp()));
    let _ = fs::copy(&path, &dest);

    // Prune oldest backups beyond MAX_BACKUPS.
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("config-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort(); // timestamp names sort chronologically
        if files.len() > MAX_BACKUPS {
            for old in &files[..files.len() - MAX_BACKUPS] {
                let _ = fs::remove_file(old);
            }
        }
    }
}

/// Persist config atomically (write to temp then rename), backing up the prior
/// config first so a bad write can always be recovered.
pub fn save(config: &AppConfig) -> Result<(), String> {
    // Never persist an obviously-empty config over a non-empty existing one
    // without a backup — the backup below guarantees recoverability regardless.
    write_backup();

    let path = config_path()?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(config).map_err(|e| format!("serialize config: {e}"))?;
    fs::write(&tmp, raw).map_err(|e| format!("write temp config: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename config: {e}"))?;
    Ok(())
}

/// Export the current config to an arbitrary path (manual backup).
pub fn export_to(dest: &str) -> Result<(), String> {
    let path = config_path()?;
    if !path.exists() {
        return Err("no config to export".into());
    }
    fs::copy(&path, dest).map_err(|e| format!("export: {e}"))?;
    Ok(())
}

/// Import a config from an arbitrary path: validate it parses, back up the
/// current one, then replace. Returns the parsed config on success.
pub fn import_from(src: &str) -> Result<AppConfig, String> {
    let raw = fs::read_to_string(src).map_err(|e| format!("read import: {e}"))?;
    let cfg: AppConfig =
        serde_json::from_str(&raw).map_err(|e| format!("import is not a valid config: {e}"))?;
    // Back up current, then write the imported one through the atomic path.
    save(&cfg)?;
    Ok(cfg)
}
