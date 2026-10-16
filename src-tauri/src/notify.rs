//! Persistent per-tile notification state so badge counts and the unwatched
//! upload "stack" survive overlay reloads.
//!
//! Model: for each tile we keep
//!   * `known`  — every item id we've ever observed (to detect genuinely new
//!                uploads; the first scan seeds this with no notifications), and
//!   * `stack`  — the unseen new uploads (newest first) as {id,url}; the badge
//!                is the stack length. Clicking pops the top (opens it).
//!
//! Stored as JSON in the app config dir alongside `config.json`.

use crate::config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;

const FILE: &str = "notifications.json";
const MAX_KNOWN: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyItem {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TileNotify {
    /// Every item id ever observed (bounds growth via MAX_KNOWN).
    #[serde(default)]
    pub known: Vec<String>,
    /// Unseen new uploads, newest first.
    #[serde(default)]
    pub stack: Vec<NotifyItem>,
    /// True once we've done the first scan (so we don't notify for the backlog).
    #[serde(default)]
    pub initialized: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifyStore {
    #[serde(default)]
    pub tiles: HashMap<String, TileNotify>,
}

fn path() -> Result<std::path::PathBuf, String> {
    Ok(config::config_dir()?.join(FILE))
}

pub fn load() -> NotifyStore {
    let p = match path() {
        Ok(p) => p,
        Err(_) => return NotifyStore::default(),
    };
    if !p.exists() {
        return NotifyStore::default();
    }
    fs::read_to_string(&p)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(store: &NotifyStore) -> Result<(), String> {
    let p = path()?;
    let tmp = p.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(store).map_err(|e| format!("serialize notify: {e}"))?;
    fs::write(&tmp, raw).map_err(|e| format!("write notify: {e}"))?;
    fs::rename(&tmp, &p).map_err(|e| format!("rename notify: {e}"))?;
    Ok(())
}

/// Record the current combined feed (newest-first `items`) for a tile and
/// return the current unseen-stack size (the badge).
///
/// First scan: seed `known` with everything, no notifications (badge stays at
/// whatever the stack already had, normally 0). Later scans: any id not in
/// `known` is a brand-new upload → pushed onto the stack (newest first).
pub fn record_feed(tile_id: &str, items: &[NotifyItem]) -> i64 {
    let mut store = load();
    let entry = store.tiles.entry(tile_id.to_string()).or_default();
    let known_owned: HashSet<String> = entry.known.iter().cloned().collect();

    if !entry.initialized {
        // Seed known with current feed; do not notify for the existing backlog.
        for it in items {
            if !known_owned.contains(&it.id) {
                entry.known.push(it.id.clone());
            }
        }
        entry.initialized = true;
    } else {
        // New uploads = feed items we've never seen. Iterate the feed in reverse
        // so the newest ends up at the FRONT of the stack.
        for it in items.iter().rev() {
            if !known_owned.contains(&it.id) {
                entry.known.push(it.id.clone());
                if !entry.stack.iter().any(|s| s.id == it.id) {
                    entry.stack.insert(0, it.clone());
                }
            }
        }
    }

    // Bound known history.
    if entry.known.len() > MAX_KNOWN {
        let overflow = entry.known.len() - MAX_KNOWN;
        entry.known.drain(0..overflow);
    }

    let count = entry.stack.len() as i64;
    let _ = save(&store);
    count
}

/// Pop the next unseen upload (top of the stack = newest). Returns its URL and
/// the remaining count. Returns None when the stack is empty.
pub fn pop_next(tile_id: &str) -> Option<(String, i64)> {
    let mut store = load();
    let entry = store.tiles.entry(tile_id.to_string()).or_default();
    if entry.stack.is_empty() {
        let _ = save(&store);
        return None;
    }
    let item = entry.stack.remove(0);
    let remaining = entry.stack.len() as i64;
    let _ = save(&store);
    Some((item.url, remaining))
}

/// Title of the item at the top of the stack (the one a single-click will
/// open), or None when the stack is empty.
pub fn peek_title(tile_id: &str) -> Option<String> {
    load()
        .tiles
        .get(tile_id)
        .and_then(|t| t.stack.first())
        .map(|it| it.title.clone())
}

/// Clear all notifications for a tile (empty the stack). Returns 0.
pub fn clear(tile_id: &str) -> i64 {
    let mut store = load();
    let entry = store.tiles.entry(tile_id.to_string()).or_default();
    entry.stack.clear();
    let _ = save(&store);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> NotifyItem {
        NotifyItem {
            id: id.to_string(),
            url: format!("https://y/{id}"),
            title: format!("Video {id}"),
        }
    }

    // Pure-logic test of the stack transitions on a TileNotify (no disk).
    fn apply(entry: &mut TileNotify, feed: &[NotifyItem]) -> i64 {
        let known_owned: HashSet<String> = entry.known.iter().cloned().collect();
        if !entry.initialized {
            for it in feed {
                if !entry.known.contains(&it.id) {
                    entry.known.push(it.id.clone());
                }
            }
            entry.initialized = true;
        } else {
            for it in feed.iter().rev() {
                if !known_owned.contains(&it.id) {
                    entry.known.push(it.id.clone());
                    if !entry.stack.iter().any(|s| s.id == it.id) {
                        entry.stack.insert(0, it.clone());
                    }
                }
            }
        }
        entry.stack.len() as i64
    }

    #[test]
    fn first_scan_no_notifications() {
        let mut e = TileNotify::default();
        let n = apply(&mut e, &[item("c"), item("b"), item("a")]);
        assert_eq!(n, 0);
        assert_eq!(e.known.len(), 3);
    }

    #[test]
    fn new_upload_adds_to_stack_newest_first() {
        let mut e = TileNotify::default();
        apply(&mut e, &[item("b"), item("a")]); // seed
        // "d" then "c" appear (feed newest-first: d,c,b,a)
        let n = apply(&mut e, &[item("d"), item("c"), item("b"), item("a")]);
        assert_eq!(n, 2);
        // newest ("d") should be on top of the stack
        assert_eq!(e.stack[0].id, "d");
        assert_eq!(e.stack[1].id, "c");
    }

    #[test]
    fn pop_removes_top_and_decrements() {
        let mut e = TileNotify::default();
        apply(&mut e, &[item("a")]);
        apply(&mut e, &[item("c"), item("b"), item("a")]); // c,b new → stack [c,b]
        assert_eq!(e.stack.len(), 2);
        let top = e.stack.remove(0);
        assert_eq!(top.id, "c");
        assert_eq!(e.stack.len(), 1);
        assert_eq!(e.stack[0].id, "b");
    }

    #[test]
    fn reload_keeps_stack() {
        // Simulate: a populated stack persists; a reload re-scans the SAME feed
        // → nothing new (all ids known) → stack unchanged.
        let mut e = TileNotify::default();
        apply(&mut e, &[item("a")]);
        apply(&mut e, &[item("b"), item("a")]); // b new → stack [b]
        assert_eq!(e.stack.len(), 1);
        let n = apply(&mut e, &[item("b"), item("a")]); // same feed
        assert_eq!(n, 1); // still 1, not re-added
    }
}
