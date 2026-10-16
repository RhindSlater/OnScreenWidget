//! Append-only price history, persisted as JSON keyed by tile id.
//!
//! Used by the price tracker to render a chart on click and to decide whether a
//! price has dropped / gone on sale relative to its recent history.

use crate::config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub at: String, // RFC3339
    pub value: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriceHistory {
    /// tile_id -> ordered points (oldest first).
    #[serde(default)]
    pub series: HashMap<String, Vec<PricePoint>>,
}

const MAX_POINTS: usize = 2000;

/// Current UTC time as an RFC3339 string, without pulling in `chrono`.
/// Format: `YYYY-MM-DDTHH:MM:SSZ` (seconds precision is plenty for prices).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // Civil date from unix seconds (days since epoch), Howard Hinnant's algorithm.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // days -> y/m/d
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0,399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1,12]
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, m, d, hh, mm, ss
    )
}

pub fn load() -> Result<PriceHistory, String> {
    let path = config::history_path()?;
    if !path.exists() {
        return Ok(PriceHistory::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read history: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse history: {e}"))
}

pub fn save(history: &PriceHistory) -> Result<(), String> {
    let path = config::history_path()?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string(history).map_err(|e| format!("serialize history: {e}"))?;
    fs::write(&tmp, raw).map_err(|e| format!("write history: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename history: {e}"))?;
    Ok(())
}

/// Record a price point for a tile. Returns comparison info against the tile's
/// **base price** — the first price ever recorded for it. The base is fixed:
/// it never changes once set, so highlighting means "cheaper than when you
/// started tracking it".
pub fn record(tile_id: &str, value: f64) -> Result<PriceChange, String> {
    let mut hist = load()?;
    let series = hist.series.entry(tile_id.to_string()).or_default();

    let previous = series.last().map(|p| p.value);
    // Base price = the first point already recorded (None on the very first poll).
    let base = series.first().map(|p| p.value);

    series.push(PricePoint {
        at: now_rfc3339(),
        value,
    });
    if series.len() > MAX_POINTS {
        let overflow = series.len() - MAX_POINTS;
        series.drain(0..overflow);
    }
    save(&hist)?;

    let dropped_from_prev = previous.map(|p| value < p).unwrap_or(false);
    // Below base only once a base exists (i.e. not on the first-ever poll).
    let below_base = base.map(|b| value < b).unwrap_or(false);

    Ok(PriceChange {
        base,
        previous,
        current: value,
        dropped_from_prev,
        below_base,
    })
}

/// Fetch the full series for a tile (for the history chart).
pub fn series_for(tile_id: &str) -> Result<Vec<PricePoint>, String> {
    let hist = load()?;
    Ok(hist.series.get(tile_id).cloned().unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChange {
    /// The tile's fixed base price (first ever recorded). None on first poll.
    pub base: Option<f64>,
    pub previous: Option<f64>,
    pub current: f64,
    pub dropped_from_prev: bool,
    /// True when the current price is below the base price.
    pub below_base: bool,
}
