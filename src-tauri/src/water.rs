//! Rolling 24-hour water log for Water tiles. Each cup is stored with its
//! timestamp and counts for 24 hours from when it was added, so progress never
//! resets at midnight. Kept in its own file so editing tiles can't wipe it.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const FILE: &str = "water.json";
const WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct WaterState {
    /// Cups logged in the last 24 hours.
    pub cups: u32,
    /// Unix ms when the oldest counted cup expires (None when empty).
    pub next_expiry_ms: Option<i64>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn store() -> &'static Mutex<HashMap<String, Vec<i64>>> {
    static S: OnceLock<Mutex<HashMap<String, Vec<i64>>>> = OnceLock::new();
    S.get_or_init(|| {
        let map = crate::config::config_dir()
            .ok()
            .and_then(|d| std::fs::read_to_string(d.join(FILE)).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Mutex::new(map)
    })
}

fn persist(map: &HashMap<String, Vec<i64>>) {
    if let Ok(dir) = crate::config::config_dir() {
        if let Ok(json) = serde_json::to_string(map) {
            let _ = std::fs::write(dir.join(FILE), json);
        }
    }
}

/// Drop expired cups; returns true when anything was removed.
fn prune(list: &mut Vec<i64>, now: i64) -> bool {
    let before = list.len();
    list.retain(|t| now - *t < WINDOW_MS);
    list.len() != before
}

fn state_of(list: &[i64]) -> WaterState {
    WaterState {
        cups: list.len() as u32,
        next_expiry_ms: list.iter().min().map(|t| t + WINDOW_MS),
    }
}

pub fn get(id: &str) -> WaterState {
    let mut map = store().lock().unwrap();
    let now = now_ms();
    let list = map.entry(id.to_string()).or_default();
    if prune(list, now) {
        let s = state_of(list);
        persist(&map);
        return s;
    }
    state_of(list)
}

pub fn add(id: &str) -> WaterState {
    let mut map = store().lock().unwrap();
    let now = now_ms();
    let list = map.entry(id.to_string()).or_default();
    prune(list, now);
    list.push(now);
    let s = state_of(list);
    persist(&map);
    s
}

/// Remove the most recently added cup (undo a mis-click).
pub fn undo(id: &str) -> WaterState {
    let mut map = store().lock().unwrap();
    let now = now_ms();
    let list = map.entry(id.to_string()).or_default();
    prune(list, now);
    if let Some(i) = list.iter().enumerate().max_by_key(|(_, t)| **t).map(|(i, _)| i) {
        list.remove(i);
    }
    let s = state_of(list);
    persist(&map);
    s
}
