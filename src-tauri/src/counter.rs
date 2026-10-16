//! Persistent daily counters for Counter (fitness) tiles, kept in their own
//! file so editing tiles in settings can never overwrite a count.
//!
//! Each counter has a "today" value that resets when the local date changes,
//! and a lifetime total that never resets.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const FILE: &str = "counters.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    /// Local date (YYYY-MM-DD) that `today` belongs to.
    #[serde(default)]
    day: String,
    #[serde(default)]
    today: i64,
    #[serde(default)]
    lifetime: i64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Counts {
    pub today: i64,
    pub lifetime: i64,
}

fn today_str() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn store() -> &'static Mutex<HashMap<String, Entry>> {
    static S: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    S.get_or_init(|| {
        let raw = crate::config::config_dir()
            .ok()
            .and_then(|d| std::fs::read_to_string(d.join(FILE)).ok())
            .unwrap_or_default();
        let map = serde_json::from_str::<HashMap<String, Entry>>(&raw)
            .or_else(|_| {
                // Older format: plain id -> number. Treat it as today's count
                // and the lifetime total.
                serde_json::from_str::<HashMap<String, i64>>(&raw).map(|m| {
                    m.into_iter()
                        .map(|(k, v)| (k, Entry { day: today_str(), today: v, lifetime: v }))
                        .collect()
                })
            })
            .unwrap_or_default();
        Mutex::new(map)
    })
}

fn persist(map: &HashMap<String, Entry>) {
    if let Ok(dir) = crate::config::config_dir() {
        if let Ok(json) = serde_json::to_string(map) {
            let _ = std::fs::write(dir.join(FILE), json);
        }
    }
}

/// Roll `today` over to zero when the date has changed.
fn rolled<'a>(map: &'a mut HashMap<String, Entry>, id: &str) -> &'a mut Entry {
    let e = map.entry(id.to_string()).or_default();
    let now = today_str();
    if e.day != now {
        e.day = now;
        e.today = 0;
    }
    e
}

pub fn get(id: &str) -> Counts {
    let mut map = store().lock().unwrap();
    let before = map.get(id).map(|e| e.day.clone());
    let e = rolled(&mut map, id);
    let out = Counts { today: e.today, lifetime: e.lifetime };
    if before.as_deref() != Some(e.day.as_str()) {
        persist(&map);
    }
    out
}

/// Add `delta` (may be negative to undo). Today never goes below 0, and only
/// what was actually removed from today is removed from the lifetime total.
pub fn add(id: &str, delta: i64) -> Counts {
    let mut map = store().lock().unwrap();
    let e = rolled(&mut map, id);
    let new_today = (e.today + delta).max(0);
    e.lifetime = (e.lifetime + (new_today - e.today)).max(0);
    e.today = new_today;
    let out = Counts { today: e.today, lifetime: e.lifetime };
    persist(&map);
    out
}

pub fn reset(id: &str) {
    let mut map = store().lock().unwrap();
    let e = rolled(&mut map, id);
    e.lifetime = (e.lifetime - e.today).max(0);
    e.today = 0;
    persist(&map);
}
