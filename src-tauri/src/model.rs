//! Shared data model for tiles, actions, and configuration.
//!
//! Everything the frontend persists flows through these types. Secrets are
//! **not** stored here — they live in the OS keychain and are referenced by a
//! `secret_ref` key (see `secrets.rs`).

use serde::{Deserialize, Serialize};

/// Top-level persisted configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub tiles: Vec<Tile>,
}

fn default_version() -> u32 {
    1
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            window: WindowConfig::default(),
            tiles: Vec::new(),
        }
    }
}

/// Window display mode & placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Keep the widget above other windows (the old "overlay" behavior).
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    /// Snap the widget to the bottom-centre of the screen and skip the taskbar
    /// (the old "dock" behavior). Independent of `always_on_top`. Defaults on.
    #[serde(default = "default_true")]
    pub docked: bool,
    /// Dock orientation: horizontal (bar) or vertical (column). Default horizontal.
    #[serde(default)]
    pub dock_orientation: DockOrientation,
    /// Which screen edge to dock to. For horizontal: Top/Bottom (default Bottom).
    /// For vertical: Left/Right (default Left). Stored explicitly.
    #[serde(default = "default_dock_side")]
    pub dock_side: DockSide,
    /// Alignment along the edge: Centered (default) or Cornered.
    #[serde(default)]
    pub dock_align: DockAlign,
    /// When cornered, which end of the edge: Start or End. For horizontal docks
    /// Start=left/End=right; for vertical docks Start=top/End=bottom.
    #[serde(default)]
    pub dock_corner: DockCorner,
    /// Preferred monitor index (0-based). None = current/primary monitor.
    #[serde(default)]
    pub dock_monitor: Option<usize>,
    /// Legacy field: older configs stored a single `mode` ("overlay"/"dock").
    /// Read on load to migrate into the two booleans; not written back.
    #[serde(default, skip_serializing)]
    pub mode: Option<WindowMode>,
    /// Global poll interval default (seconds) applied to tiles without an override.
    #[serde(default = "default_poll")]
    pub default_poll_secs: u64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    /// Animated border style applied to all tiles.
    #[serde(default)]
    pub border_effect: BorderEffect,
}

/// How the coloured tile border is drawn/animated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BorderEffect {
    /// No animation — a plain solid border in the status colour.
    Static,
    /// A colour comet slowly orbits the perimeter (default).
    #[default]
    Orbit,
    /// The whole border gently fades in and out (breathing).
    Breathing,
    /// A sharper on/off glow pulse.
    Pulse,
    /// The border hue continuously cycles through the spectrum.
    Rainbow,
    /// No border colour at all (neutral edge only).
    None,
}

/// Dock orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DockOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Which screen edge the widget docks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Top,
    Bottom,
    Left,
    Right,
}

fn default_dock_side() -> DockSide {
    DockSide::Bottom
}

/// Alignment of the dock along its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DockAlign {
    #[default]
    Centered,
    Cornered,
}

/// Which end of the edge when cornered. For horizontal: Start=left, End=right.
/// For vertical: Start=top, End=bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DockCorner {
    #[default]
    Start,
    End,
}

fn default_true() -> bool {
    true
}
fn default_poll() -> u64 {
    60
}
fn default_opacity() -> f64 {
    0.95
}

impl WindowConfig {
    /// Fold a legacy `mode` into the boolean toggles, and keep dock_side
    /// consistent with the orientation. Call after deserialization.
    pub fn migrate_legacy(&mut self) {
        if let Some(mode) = self.mode.take() {
            match mode {
                WindowMode::Overlay => {
                    self.always_on_top = true;
                    self.docked = false;
                }
                WindowMode::Dock => {
                    self.always_on_top = false;
                    self.docked = true;
                }
            }
        }
        // Ensure the side matches the orientation (horizontal⇒top/bottom,
        // vertical⇒left/right); fix up if a stale/invalid combo was stored.
        self.normalize_dock_side();
    }

    /// Coerce dock_side into a valid value for the current orientation.
    pub fn normalize_dock_side(&mut self) {
        match self.dock_orientation {
            DockOrientation::Horizontal => {
                if !matches!(self.dock_side, DockSide::Top | DockSide::Bottom) {
                    self.dock_side = DockSide::Bottom;
                }
            }
            DockOrientation::Vertical => {
                if !matches!(self.dock_side, DockSide::Left | DockSide::Right) {
                    self.dock_side = DockSide::Left;
                }
            }
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            always_on_top: true,
            docked: true,
            dock_orientation: DockOrientation::Horizontal,
            dock_side: DockSide::Bottom,
            dock_align: DockAlign::Centered,
            dock_corner: DockCorner::Start,
            dock_monitor: None,
            mode: None,
            default_poll_secs: default_poll(),
            opacity: default_opacity(),
            border_effect: BorderEffect::default(),
        }
    }
}

/// Legacy single-mode enum, retained only to migrate old configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Overlay,
    Dock,
}

/// A single monitored item on the grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    pub id: String,
    pub label: String,
    /// Optional path/URL to a custom icon. If None the UI shows a default per kind.
    #[serde(default)]
    pub icon: Option<String>,
    /// The monitor configuration — determines what is checked and how.
    pub kind: MonitorKind,
    /// What happens on 1 / 2 / 3 clicks.
    #[serde(default)]
    pub actions: ClickActions,
    /// Per-tile poll override (seconds). Falls back to global default.
    #[serde(default)]
    pub poll_secs: Option<u64>,
    /// Optional millisecond poll interval. Honored ONLY for metrics tiles,
    /// enabling sub-second refresh (min 100ms). Other tiles ignore this.
    #[serde(default)]
    pub poll_ms: Option<u64>,
    /// UI ordering.
    #[serde(default)]
    pub order: u32,
    /// Disabled tiles keep their config but are not shown in the widget and are
    /// not polled. Toggle from Settings. Defaults to enabled (true) so existing
    /// configs are unaffected.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Show this tile's detail text always, instead of only on hover.
    #[serde(default)]
    pub details_always: bool,
    /// Grid footprint of the tile. Default is a 1×1 square cell.
    #[serde(default)]
    pub size: TileSize,
}

/// How many grid cells a tile occupies, as width×height (columns×rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TileSize {
    /// 1 wide, 1 tall — the default single square cell.
    #[default]
    #[serde(rename = "1x1")]
    S1x1,
    /// 2 wide, 2 tall — a big square (size of 4 cells).
    #[serde(rename = "2x2")]
    S2x2,
    /// 1 wide, 2 tall — a tall tile.
    #[serde(rename = "1x2")]
    S1x2,
    /// 2 wide, 1 tall — a wide tile.
    #[serde(rename = "2x1")]
    S2x1,
}

impl Tile {
    /// Fold legacy single-source YouTube fields (`channel_id`/`playlist_id`)
    /// into the new `sources` list. Safe to call repeatedly.
    pub fn migrate_legacy(&mut self) {
        if let MonitorKind::YouTube {
            sources,
            channel_id,
            playlist_id,
        } = &mut self.kind
        {
            if sources.is_empty() {
                if let Some(ch) = channel_id.take().filter(|s| !s.is_empty()) {
                    sources.push(YouTubeSource {
                        kind: YouTubeSourceKind::Channel,
                        id: ch,
                    });
                }
                if let Some(pl) = playlist_id.take().filter(|s| !s.is_empty()) {
                    sources.push(YouTubeSource {
                        kind: YouTubeSourceKind::Playlist,
                        id: pl,
                    });
                }
            }
            // Clear legacy fields regardless so they aren't carried around.
            *channel_id = None;
            *playlist_id = None;
        }
    }
}

/// The monitor "plugin" kind and its parameters. Tagged enum keeps the frontend
/// config simple and type-safe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MonitorKind {
    /// Raw TCP connect check (used for NAS reachability, generic ports).
    TcpPing { host: String, port: u16 },
    /// HTTP(S) check — healthy when status is in the expected range (default 200).
    HttpStatus {
        url: String,
        #[serde(default = "default_expected_status")]
        expected_status: u16,
    },
    /// Minecraft Server List Ping (Java edition).
    Minecraft {
        host: String,
        #[serde(default = "default_mc_port")]
        port: u16,
    },
    /// Jellyfin server — hits /System/Info/Public (no auth required).
    Jellyfin { base_url: String },
    /// Product price tracker via CSS selector extraction.
    Price {
        url: String,
        css_selector: String,
        #[serde(default)]
        currency: Option<String>,
    },
    /// YouTube upload tracker via public RSS feeds. Scans multiple sources
    /// (channels and/or playlists) together and aggregates new uploads.
    #[serde(rename = "youtube", alias = "you_tube")]
    YouTube {
        /// The sources to scan (channels/playlists).
        #[serde(default)]
        sources: Vec<YouTubeSource>,
        /// Legacy single-source fields (migrated into `sources` on load).
        #[serde(default, skip_serializing)]
        channel_id: Option<String>,
        #[serde(default, skip_serializing)]
        playlist_id: Option<String>,
    },
    /// Twitch live tracker (requires client id + token secret_ref).
    Twitch {
        channel_login: String,
        client_id: String,
        /// Key into the OS keychain for the OAuth app token.
        token_secret_ref: String,
    },
    /// Discord voice-channel presence via the public guild widget.json endpoint.
    /// Requires the server's Widget to be enabled (Server Settings → Widget).
    Discord {
        /// The guild (server) ID.
        guild_id: String,
        /// Voice channels to watch. Badge = total people across these channels.
        channels: Vec<DiscordChannel>,
        /// Channel ID that a single-click should open by default.
        default_channel_id: String,
    },
    /// Generic API tile: fetch a URL and display a value extracted from the
    /// JSON response via a dot/bracket path (e.g. "data.items.0.name"). If the
    /// response isn't JSON or the path is empty, the raw body is shown.
    Api {
        url: String,
        #[serde(default = "default_http_method")]
        method: String,
        /// Optional header to send an auth token under (e.g. "Authorization").
        #[serde(default)]
        auth_header: Option<String>,
        /// Keychain key for the auth token value.
        #[serde(default)]
        auth_token_secret_ref: Option<String>,
        /// Dot/bracket path into the JSON response. Empty = whole body.
        #[serde(default)]
        json_path: Option<String>,
        /// Optional text shown before the value (e.g. "CPU ").
        #[serde(default)]
        prefix: Option<String>,
        /// Optional text shown after the value (e.g. "%").
        #[serde(default)]
        suffix: Option<String>,
    },
    /// System metrics tile: up to 4 always-visible metrics (CPU/memory/temps),
    /// each with threshold stops that colour the tile border by highest severity.
    Metrics {
        #[serde(default)]
        metrics: Vec<MetricSpec>,
    },
    /// Graph tile: a rolling multi-line chart of metric values over time. Each
    /// metric is a coloured line; the border flashes on its final threshold.
    Graph {
        #[serde(default)]
        metrics: Vec<MetricSpec>,
    },
    /// System media (SMTC) controls tile: shows what's playing (YouTube/Tidal/
    /// Spotify/…) with prev / play-pause / next. Glows while playing. No config.
    Media,
    /// Bluetooth/wireless battery tile: a nested 2×2 grid of up to 4 devices,
    /// each showing a type icon + battery %. Per-cell border flashes when the
    /// battery is at/below `low_threshold`, highlights while charging, and is
    /// static otherwise.
    Bluetooth {
        /// Up to 4 configured device slots. Empty = auto-discover connected
        /// devices that report a battery level.
        #[serde(default)]
        devices: Vec<BtDeviceSpec>,
        /// Low-battery threshold (%) that triggers the flashing border.
        #[serde(default = "default_bt_low")]
        low_threshold: u8,
    },
    /// AWS Kiro usage tile: shows plan credits remaining by shelling out to the
    /// Kiro CLI (`kiro-cli chat --no-interactive "/usage"`), which uses the
    /// already-logged-in AWS Builder ID — no API key needed. Warns when the
    /// remaining credits fall at/below `low_threshold`.
    Kiro {
        /// Warn threshold: highlight when remaining plan credits ≤ this.
        #[serde(default = "default_kiro_low")]
        low_threshold: f64,
    },
    /// Slideshow tile: cycles through images and videos in a local folder.
    /// The frontend handles the rotation; the backend provides the file list.
    Slideshow {
        /// Absolute path to the folder containing images/videos.
        folder: String,
        /// Seconds between slides. Default 30.
        #[serde(default = "default_slideshow_interval")]
        interval_secs: u64,
    },
    /// Fitness counter: shows a large number; each click adds `step` (default
    /// 5). The count is stored separately (counters.json), not in this config.
    Counter {
        #[serde(default = "default_counter_step")]
        step: u32,
    },
    /// Date & time tile. Clicking it opens the Windows calendar flyout.
    Clock,
    /// Water intake: each click logs one cup, which counts for 24 hours (a
    /// rolling window, not reset at midnight). Progress is against `goal_cups`.
    Water {
        #[serde(default = "default_water_goal")]
        goal_cups: u32,
    },
}

fn default_water_goal() -> u32 {
    12
}

fn default_counter_step() -> u32 {
    5
}

fn default_bt_low() -> u8 {
    20
}

fn default_kiro_low() -> f64 {
    500.0
}

fn default_slideshow_interval() -> u64 {
    30
}

/// A configured Bluetooth device slot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BtDeviceSpec {
    /// Case-insensitive substring matched against the device name (e.g. "mx
    /// master", "wh-1000"). Empty = any (fills remaining auto-discovered slots).
    #[serde(default)]
    pub name_match: String,
    /// Optional icon override (emoji or image path). Empty = auto by device kind.
    #[serde(default)]
    pub icon: Option<String>,
    /// Optional display-name override. Empty = the device's own name.
    #[serde(default)]
    pub label: Option<String>,
}

/// One metric shown on a Metrics tile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    pub source: MetricSource,
    /// Display label (e.g. "CPU", "RAM", "GPU °C").
    pub label: String,
    /// Unit suffix shown after the value (e.g. "%", "MB", "°C").
    #[serde(default)]
    pub unit: String,
    /// Threshold stops (value → colour), ordered low→high. One minimum; more
    /// form a gradient the border interpolates across by severity.
    #[serde(default)]
    pub thresholds: Vec<ThresholdStop>,
    /// Line colour on a Graph tile (hex). Ignored by the Metrics tile.
    #[serde(default = "default_line_color")]
    pub line_color: String,
}

fn default_line_color() -> String {
    "#60a5fa".to_string()
}

/// Where a metric's value comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MetricSource {
    /// A process's CPU usage as a % of total system capacity. `process` is a
    /// case-insensitive substring match against process names; empty = this app.
    AppCpu {
        #[serde(default)]
        process: String,
    },
    /// A process's memory usage in megabytes. `process` matches a process name;
    /// empty = this app.
    AppMemory {
        #[serde(default)]
        process: String,
    },
    /// System-wide CPU utilization (%).
    SystemCpu,
    /// System-wide memory utilization (%).
    SystemMemory,
    /// A hardware component temperature in °C. `component` is a substring match
    /// against the sensor label (e.g. "cpu", "gpu"); empty = first available.
    Temperature {
        #[serde(default)]
        component: String,
    },
    /// Any HWiNFO hardware sensor (fan RPM, voltage, clock, power, usage,
    /// current, temperature…). `sensor` matches the full sensor label (exact,
    /// then substring); empty = first available. The unit is taken from HWiNFO.
    HardwareSensor {
        #[serde(default)]
        sensor: String,
    },
    /// Any LibreHardwareMonitor sensor, read over LHM's local web server
    /// (`http://127.0.0.1:8085/data.json`). `sensor` matches the full sensor
    /// path (exact, then substring); empty = first available. Unit from LHM.
    LhmSensor {
        #[serde(default)]
        sensor: String,
    },
    /// Disk space for a drive/mount. `mount` matches a mount point or drive
    /// (e.g. "C:" on Windows, "/" on Linux, a mapped network drive letter);
    /// empty = first disk. `metric` chooses which number to show.
    Disk {
        #[serde(default)]
        mount: String,
        #[serde(default)]
        metric: DiskMetric,
    },
    /// Network throughput in MB/s for an interface. `interface` is a
    /// case-insensitive substring match on the adapter name; empty = sum of all.
    NetworkThroughput {
        #[serde(default)]
        interface: String,
        #[serde(default)]
        direction: NetDir,
    },
}

/// Network direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetDir {
    /// Inbound (received).
    #[default]
    In,
    /// Outbound (transmitted).
    Out,
}

/// Which disk figure to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiskMetric {
    /// Free space (GB).
    #[default]
    Free,
    /// Used space (GB).
    Used,
    /// Total capacity (GB).
    Total,
    /// Used percentage (0–100).
    Percent,
}

/// A threshold stop for the gradient (value → colour). `op` is the comparison
/// used to decide the "final threshold reached → flash" condition for the
/// highest stop (and is available per-stop); defaults to `>=`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdStop {
    pub value: f64,
    pub color: String,
    #[serde(default)]
    pub op: ThresholdOp,
}

/// Comparison operator for a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThresholdOp {
    /// value > threshold
    #[serde(rename = "gt")]
    Gt,
    /// value < threshold
    #[serde(rename = "lt")]
    Lt,
    /// value == threshold (within a small epsilon)
    #[serde(rename = "eq")]
    Eq,
    /// value >= threshold (default)
    #[serde(rename = "gte")]
    #[default]
    Gte,
    /// value <= threshold
    #[serde(rename = "lte")]
    Lte,
}

impl ThresholdOp {
    /// Evaluate `value <op> threshold`.
    pub fn eval(self, value: f64, threshold: f64) -> bool {
        match self {
            ThresholdOp::Gt => value > threshold,
            ThresholdOp::Lt => value < threshold,
            ThresholdOp::Eq => (value - threshold).abs() < 1e-6,
            ThresholdOp::Gte => value >= threshold,
            ThresholdOp::Lte => value <= threshold,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordChannel {
    pub label: String,
    pub channel_id: String,
}

/// One YouTube source to scan: either a channel or a playlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeSource {
    /// "channel" or "playlist".
    pub kind: YouTubeSourceKind,
    /// The channel ID (UC…) or playlist ID (PL…).
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum YouTubeSourceKind {
    Channel,
    Playlist,
}

fn default_expected_status() -> u16 {
    200
}
fn default_mc_port() -> u16 {
    25565
}

/// Click actions configurable per tile. Each maps to an `ActionSpec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClickActions {
    #[serde(default)]
    pub single: Option<ActionSpec>,
    #[serde(default)]
    pub double: Option<ActionSpec>,
    #[serde(default)]
    pub triple: Option<ActionSpec>,
}

/// A concrete action to perform in response to a click.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSpec {
    /// Open a URL in the default browser.
    OpenUrl { url: String },
    /// Open a terminal and SSH into a host.
    SshTerminal {
        host: String,
        #[serde(default)]
        user: Option<String>,
        #[serde(default = "default_ssh_port")]
        port: u16,
    },
    /// Run a command over SSH (non-interactive), e.g. restart a service.
    SshCommand {
        host: String,
        #[serde(default)]
        user: Option<String>,
        #[serde(default = "default_ssh_port")]
        port: u16,
        command: String,
    },
    /// Open a terminal running an arbitrary program (used for AI-agent tiles).
    SpawnTerminal {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// Launch an application/executable directly (no terminal window).
    LaunchApp {
        /// Absolute path to the executable (e.g. an .exe on Windows, .app/binary on macOS).
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// Fire an HTTP request in the background (no browser popup). Used to ping a
    /// webhook listener that runs a server-side command (e.g. restart a service).
    /// The auth token is read from the OS keychain via `auth_token_secret_ref`.
    HttpRequest {
        #[serde(default = "default_http_method")]
        method: String,
        url: String,
        /// Header name to send the auth token under (e.g. "X-Auth-Token").
        #[serde(default)]
        auth_header: Option<String>,
        /// Keychain key for the auth token value.
        #[serde(default)]
        auth_token_secret_ref: Option<String>,
        /// Optional request body.
        #[serde(default)]
        body: Option<String>,
    },
    /// Frontend-handled action (e.g. open latest YouTube video, show price history).
    /// The `name` is interpreted by the UI, not the backend.
    Frontend { name: String },
}

fn default_ssh_port() -> u16 {
    22
}

fn default_http_method() -> String {
    "POST".to_string()
}

/// A set of example tiles seeded on first run so the grid isn't empty. These
/// use placeholder hosts/URLs the user should edit. Nothing here contains
/// secrets.
pub fn example_tiles() -> Vec<Tile> {
    vec![
        Tile {
            id: "mc-main".into(),
            label: "Minecraft".into(),
            icon: None,
            kind: MonitorKind::Minecraft {
                host: "mc.example.com".into(),
                port: 25565,
            },
            actions: ClickActions {
                single: Some(ActionSpec::SshTerminal {
                    host: "mc.example.com".into(),
                    user: Some("admin".into()),
                    port: 22,
                }),
                double: None,
                triple: Some(ActionSpec::SshCommand {
                    host: "mc.example.com".into(),
                    user: Some("admin".into()),
                    port: 22,
                    command: "sudo systemctl restart minecraft".into(),
                }),
            },
            poll_secs: Some(30),
            poll_ms: None,
            order: 0,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "nas".into(),
            label: "NAS".into(),
            icon: None,
            kind: MonitorKind::TcpPing {
                host: "192.168.1.10".into(),
                port: 445,
            },
            actions: ClickActions {
                single: Some(ActionSpec::OpenUrl {
                    url: "http://192.168.1.10".into(),
                }),
                double: None,
                triple: None,
            },
            poll_secs: Some(60),
            poll_ms: None,
            order: 1,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "jellyfin".into(),
            label: "Jellyfin".into(),
            icon: None,
            kind: MonitorKind::Jellyfin {
                base_url: "http://192.168.1.10:8096".into(),
            },
            actions: ClickActions {
                single: Some(ActionSpec::OpenUrl {
                    url: "http://192.168.1.10:8096".into(),
                }),
                double: None,
                triple: None,
            },
            poll_secs: Some(60),
            poll_ms: None,
            order: 2,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "website".into(),
            label: "My Site".into(),
            icon: None,
            kind: MonitorKind::HttpStatus {
                url: "https://example.com".into(),
                expected_status: 200,
            },
            actions: ClickActions {
                single: Some(ActionSpec::OpenUrl {
                    url: "https://example.com".into(),
                }),
                double: None,
                triple: None,
            },
            poll_secs: Some(120),
            poll_ms: None,
            order: 3,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "yt".into(),
            label: "YouTube".into(),
            icon: None,
            kind: MonitorKind::YouTube {
                // Example: Google Developers channel.
                sources: vec![YouTubeSource {
                    kind: YouTubeSourceKind::Channel,
                    id: "UC_x5XG1OV2P6uZZ5FSM9Ttw".into(),
                }],
                channel_id: None,
                playlist_id: None,
            },
            actions: ClickActions {
                // Single click: open next new video (pop) or YouTube home.
                single: Some(ActionSpec::Frontend {
                    name: "open_latest_video".into(),
                }),
                double: None,
                // Triple click: clear all notifications.
                triple: Some(ActionSpec::Frontend {
                    name: "clear_badge".into(),
                }),
            },
            poll_secs: Some(300),
            poll_ms: None,
            order: 4,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "discord-call".into(),
            label: "Basement".into(),
            icon: None,
            kind: MonitorKind::Discord {
                guild_id: "559332167604502548".into(),
                // Watch only the Basement voice channel; badge = active users there.
                channels: vec![DiscordChannel {
                    label: "Basement".into(),
                    channel_id: "744415792254156831".into(),
                }],
                // Deep-link target for single-click (Basement voice channel).
                default_channel_id: "744415792254156831".into(),
            },
            actions: ClickActions {
                // Single-click opens Discord at the Basement channel.
                single: Some(ActionSpec::OpenUrl {
                    url: "discord://-/channels/559332167604502548/744415792254156831".into(),
                }),
                double: None,
                triple: None,
            },
            poll_secs: Some(30),
            poll_ms: None,
            order: 5,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
        Tile {
            id: "metrics".into(),
            label: "Widget".into(),
            icon: None,
            kind: MonitorKind::Metrics {
                metrics: vec![
                    MetricSpec {
                        source: MetricSource::AppCpu { process: String::new() },
                        label: "CPU".into(),
                        unit: "%".into(),
                        thresholds: vec![
                            ThresholdStop { value: 25.0, color: "#4ade80".into(), op: ThresholdOp::Gte },
                            ThresholdStop { value: 60.0, color: "#fbbf24".into(), op: ThresholdOp::Gte },
                            ThresholdStop { value: 90.0, color: "#f87171".into(), op: ThresholdOp::Gte },
                        ],
                        line_color: "#60a5fa".into(),
                    },
                    MetricSpec {
                        source: MetricSource::AppMemory { process: String::new() },
                        label: "RAM".into(),
                        unit: "MB".into(),
                        thresholds: vec![
                            ThresholdStop { value: 100.0, color: "#4ade80".into(), op: ThresholdOp::Gte },
                            ThresholdStop { value: 300.0, color: "#fbbf24".into(), op: ThresholdOp::Gte },
                            ThresholdStop { value: 600.0, color: "#f87171".into(), op: ThresholdOp::Gte },
                        ],
                        line_color: "#f472b6".into(),
                    },
                ],
            },
            actions: ClickActions::default(),
            poll_secs: Some(2),
            poll_ms: Some(500),
            order: 6,
            enabled: true,
            details_always: false,
            size: TileSize::S1x1,
        },
    ]
}
