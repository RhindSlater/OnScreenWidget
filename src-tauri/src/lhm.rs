//! LibreHardwareMonitor (LHM) reader.
//!
//! LHM (and OpenHardwareMonitor) can run a small local web server that serves
//! every sensor as JSON at `http://127.0.0.1:8085/data.json`. Enable it in LHM:
//! Options → "Remote Web Server" → Run. Unlike HWiNFO's shared memory, there's
//! no 12-hour limit and no "sensors window must be open" quirk — if the web
//! server is running, the data is there.
//!
//! The JSON is a recursive tree: each node has `Text` (name), optional `Value`
//! (a string like "56.0 °C" / "1500 RPM" / "1.1 V"), and `Children`. We flatten
//! it to leaf readings labelled by their path (e.g.
//! "AMD Radeon RX 7900 XTX / Temperatures / GPU Hot Spot") with the numeric
//! value and unit parsed from `Value`.

use serde::Deserialize;

/// LHM web-server endpoint. Standard default port.
const LHM_URL: &str = "http://127.0.0.1:8085/data.json";

/// A single flattened LHM sensor reading.
#[derive(Debug, Clone)]
pub struct Reading {
    pub label: String,
    pub unit: String,
    pub value: f64,
}

/// Dropdown entry describing an available sensor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SensorInfo {
    pub label: String,
    pub unit: String,
}

/// Raw LHM JSON node.
#[derive(Debug, Deserialize)]
struct Node {
    #[serde(default)]
    #[serde(rename = "Text")]
    text: String,
    #[serde(default)]
    #[serde(rename = "Value")]
    value: String,
    #[serde(default)]
    #[serde(rename = "Children")]
    children: Vec<Node>,
}

/// Fetch + flatten all LHM readings. Returns empty on any error (server not
/// running, parse failure, etc.). This performs a blocking HTTP GET with a
/// short timeout; call it off the hot path / behind a cache.
pub fn read_all_blocking() -> Vec<Reading> {
    let native = helper::latest();
    if !native.is_empty() || helper::available() {
        return native;
    }
    let body = match http_get(LHM_URL) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let root: Node = match serde_json::from_str(&body) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    flatten(&root, &mut Vec::new(), &mut out);
    out
}

/// Built-in sensor source: spawns `sensor-helper.exe` (LibreHardwareMonitorLib,
/// which loads a signed ring0 driver for MSR / Super I/O / SMBus / SMART access,
/// the same mechanism HWMonitor uses) and keeps the newest full-precision
/// snapshot. Needs the app to run elevated for CPU core / board / RAM sensors.
mod helper {
    use super::Reading;
    use serde::Deserialize;
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    #[derive(Deserialize)]
    struct Frame {
        t: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        l: String,
        v: f64,
        u: String,
    }

    struct State {
        readings: Vec<Reading>,
        at: Option<Instant>,
        child: Option<Child>,
        failed_at: Option<Instant>,
    }

    fn state() -> &'static Mutex<State> {
        static S: OnceLock<Mutex<State>> = OnceLock::new();
        S.get_or_init(|| {
            Mutex::new(State { readings: Vec::new(), at: None, child: None, failed_at: None })
        })
    }

    fn locate() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        // Installed: next to the exe. Dev: exe lives in src-tauri\target\<profile>[\deps],
        // so also search a few ancestors for src-tauri\sensor-helper-bin.
        exe.ancestors()
            .skip(1)
            .take(5)
            .flat_map(|d| {
                [
                    d.join("sensor-helper-bin").join("sensor-helper.exe"),
                    d.join("sensor-helper.exe"),
                ]
            })
            .find(|p| p.is_file())
    }

    fn spawn() -> Option<Child> {
        let path = locate()?;
        let mut cmd = Command::new(path);
        cmd.arg("500")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().ok()?;
        let stdout = child.stdout.take()?;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(f) = serde_json::from_str::<Frame>(&line) {
                    let readings = f
                        .t
                        .into_iter()
                        // Without admin rights CPU sensors report a bogus 0 °C.
                        .filter(|i| !(i.u.ends_with('C') && i.v == 0.0))
                        .map(|i| Reading { label: i.l, value: i.v, unit: i.u })
                        .collect();
                    let mut s = state().lock().unwrap();
                    s.readings = readings;
                    s.at = Some(Instant::now());
                }
            }
        });
        Some(child)
    }

    /// Latest snapshot (starting / restarting the helper as needed). Empty if
    /// the helper is missing, dead, or stale; callers then fall back to HTTP.
    pub fn latest() -> Vec<Reading> {
        let mut s = state().lock().unwrap();
        let alive = match s.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        };
        // A live helper that stopped producing frames is hung: kill and restart.
        let hung = alive && s.at.map_or(false, |t| t.elapsed() > Duration::from_secs(10));
        if !alive || hung {
            if let Some(mut old) = s.child.take() {
                let _ = old.kill();
                let _ = old.wait(); // reap so no zombie handle lingers
            }
            let retry_ok = s.failed_at.map_or(true, |t| t.elapsed() > Duration::from_secs(30));
            if retry_ok {
                s.at = None;
                s.child = spawn();
                if s.child.is_none() {
                    s.failed_at = Some(Instant::now());
                }
            }
        }
        match s.at {
            Some(t) if t.elapsed() < Duration::from_secs(3) => s.readings.clone(),
            _ => Vec::new(),
        }
    }

    /// True once a helper binary exists, so callers can skip the HTTP fallback
    /// while the helper is still warming up.
    pub fn available() -> bool {
        locate().is_some()
    }

    /// Stop the helper (called on app exit).
    pub fn shutdown() {
        if let Some(mut c) = state().lock().unwrap().child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub fn shutdown_helper() {
    helper::shutdown();
}

pub fn helper_available() -> bool {
    helper::available()
}

/// True if the unit denotes degrees Celsius ("°C", "C", or the mojibake forms).
pub fn is_celsius(unit: &str) -> bool {
    let u = unit.trim();
    u == "°C" || u == "C" || u.ends_with("°C") || u.contains("\u{b0}C")
}

/// Hottest CPU-core temperature reading, if any (synthetic "CPU Core Max").
pub fn cpu_core_max(readings: &[Reading]) -> Option<f64> {
    let max = readings
        .iter()
        .filter(|r| is_celsius(&r.unit))
        .filter(|r| {
            let l = r.label.to_lowercase();
            let gpu = l.contains("gpu") || l.contains("radeon") || l.contains("geforce") || l.contains("nvidia");
            !gpu && r.value > 0.0
                && (l.contains("cpu core")
                    || l.contains("core #")
                    || l.contains("tdie")
                    || l.contains("tctl")
                    || l.contains("ccd")
                    || l.contains("cpu package"))
        })
        .map(|r| r.value)
        .filter(|v| v.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    if max.is_finite() {
        Some(max)
    } else {
        None
    }
}

/// Resolve a single sensor's value by exact path (case-insensitive) then
/// substring; empty = first available.
pub fn value_for(readings: &[Reading], needle: &str) -> Option<f64> {
    if readings.is_empty() {
        return None;
    }
    let n = needle.trim();
    if n.is_empty() {
        return readings.first().map(|r| r.value);
    }
    if let Some(r) = readings.iter().find(|r| r.label.eq_ignore_ascii_case(n)) {
        return Some(r.value);
    }
    let nl = n.to_lowercase();
    readings
        .iter()
        .find(|r| r.label.to_lowercase().contains(&nl))
        .map(|r| r.value)
}

/// Troubleshooting output: reachability + a sample of readings.
pub fn diagnostics() -> String {
    match http_get(LHM_URL) {
        None => format!(
            "[lhm diag v1]\n\
             Could not reach LibreHardwareMonitor at {LHM_URL}.\n\
             In LHM: Options → 'Remote Web Server' → Run (default port 8085).\n\
             Keep LHM running. No admin needed for the reader; LHM itself needs\n\
             admin to read sensors."
        ),
        Some(body) => {
            let readings = match serde_json::from_str::<Node>(&body) {
                Ok(root) => {
                    let mut out = Vec::new();
                    flatten(&root, &mut Vec::new(), &mut out);
                    out
                }
                Err(e) => return format!("[lhm diag v1]\nReached server but JSON parse failed: {e}"),
            };
            let mut s = format!(
                "[lhm diag v1]\nReached {LHM_URL}; {} reading(s).\n--- sample ---\n",
                readings.len()
            );
            for r in readings.iter().take(60) {
                s.push_str(&format!("{}: {:.2} {}\n", r.label, r.value, r.unit));
            }
            s
        }
    }
}

/// Recursively flatten the node tree into leaf readings with a path label.
fn flatten(node: &Node, path: &mut Vec<String>, out: &mut Vec<Reading>) {
    let has_text = !node.text.is_empty();
    if has_text {
        path.push(node.text.clone());
    }
    // A leaf with a numeric Value is a reading.
    if node.children.is_empty() {
        if let Some((value, unit)) = parse_value(&node.value) {
            // Drop the leading root node name (the computer name) from the label
            // if present, keeping it compact: "<hardware> / <group> / <sensor>".
            let label = if path.len() > 1 {
                path[1..].join(" / ")
            } else {
                path.join(" / ")
            };
            if !label.is_empty() {
                out.push(Reading { label, value, unit });
            }
        }
    } else {
        for c in &node.children {
            flatten(c, path, out);
        }
    }
    if has_text {
        path.pop();
    }
}

/// Parse an LHM value string like "56.0 °C", "1500 RPM", "1.1 V", "12,3 %"
/// into (number, unit). Returns None if there's no leading number.
fn parse_value(raw: &str) -> Option<(f64, String)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // LHM may use a comma decimal separator depending on locale.
    let norm = s.replacen(',', ".", 1);
    // Split the leading numeric token from the trailing unit.
    let mut split = norm.len();
    for (i, c) in norm.char_indices() {
        if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' {
            continue;
        } else {
            split = i;
            break;
        }
    }
    let (num_str, unit_str) = norm.split_at(split);
    let value: f64 = num_str.trim().parse().ok()?;
    let unit = unit_str.trim().to_string();
    Some((value, unit))
}

/// Minimal blocking HTTP GET with a short timeout, no external blocking client.
/// Blocking HTTP GET with a short timeout, no reqwest `blocking` feature.
///
/// IMPORTANT: this is called from inside Tauri's async runtime (via the poll
/// loop → metrics/graph sampling). Creating a tokio runtime and calling
/// `block_on` *inside* an existing runtime panics ("Cannot start a runtime from
/// within a runtime"). To stay safe regardless of caller context, we run the
/// request on a dedicated OS thread that owns its own current-thread runtime,
/// then join it. The 750ms sampler cache keeps this off the hot path.
fn http_get(url: &str) -> Option<String> {
    let url = url.to_string();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(600))
                .build()
                .ok()?;
            let resp = client.get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.text().await.ok()
        })
    })
    .join()
    .ok()
    .flatten()
}
