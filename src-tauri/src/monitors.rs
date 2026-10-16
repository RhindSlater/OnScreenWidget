//! Monitor implementations. Each `MonitorKind` resolves to a `TileStatus`.
//!
//! All checks are async and time-bounded so a dead host never blocks the poll
//! loop. Results are plain data the frontend renders.

use crate::model::{
    BtDeviceSpec, DiscordChannel, DiskMetric, MetricSource, MetricSpec, MonitorKind, ThresholdStop,
};
use crate::secrets;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const NET_TIMEOUT: Duration = Duration::from_secs(8);

/// Health classification shown as the tile color/state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Up,
    Down,
    /// Reachable but degraded / warning (e.g. unexpected status code).
    Warn,
    /// Could not determine (misconfig, missing secret, etc.).
    Unknown,
}

/// Result of a single monitor check, emitted to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileStatus {
    pub tile_id: String,
    pub health: Health,
    /// Human-readable one-line summary (e.g. "online • 3/20 players", "$42.99").
    pub detail: String,
    /// Optional numeric badge (unread count, upload count, player count...).
    #[serde(default)]
    pub badge: Option<i64>,
    /// Optional primary metric value (price, credits...) for history/compare.
    #[serde(default)]
    pub value: Option<f64>,
    /// Optional link associated with the tile (e.g. the latest YouTube video URL)
    /// that a frontend click action can open.
    #[serde(default)]
    pub link: Option<String>,
    /// For Metrics tiles: the per-metric readouts to display (label + value).
    #[serde(default)]
    pub metrics: Vec<MetricReadout>,
    /// For Metrics tiles: the border colour (hex) from the highest-severity
    /// metric's gradient position.
    #[serde(default)]
    pub border_color: Option<String>,
    /// For Metrics tiles: flash the border when a metric reaches its final
    /// threshold's condition.
    #[serde(default)]
    pub flash: bool,
    /// For Graph tiles: the current value + line colour for each metric this
    /// sample. The frontend appends these to a rolling buffer to draw lines.
    #[serde(default)]
    pub graph_points: Vec<GraphPoint>,
    /// For Bluetooth tiles: one readout per device cell.
    #[serde(default)]
    pub bt_devices: Vec<BtDeviceReadout>,
    /// For Media tiles: now-playing title/artist/app + playing flag.
    #[serde(default)]
    pub media: Option<MediaReadout>,
    /// RFC3339 timestamp of this check.
    pub checked_at: String,
}

/// Now-playing media info for a Media tile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaReadout {
    pub title: String,
    pub artist: String,
    pub app: String,
    /// Actively playing (vs paused).
    pub playing: bool,
    /// A media session exists at all (something loaded).
    pub active: bool,
    /// Album/media artwork as a data URI (base64), empty if none.
    pub art: String,
}

/// One Bluetooth device cell: icon, name, battery %, charging + low flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtDeviceReadout {
    /// Display label (device name or configured override).
    pub label: String,
    /// Icon (emoji or image path) — auto by kind unless overridden.
    pub icon: String,
    /// Battery percentage 0–100, or None if the device didn't report one.
    pub battery: Option<u8>,
    /// True while the device is charging (only some devices report this).
    pub charging: bool,
    /// True when battery is at/below the tile's low threshold.
    pub low: bool,
    /// False when a configured device isn't currently connected/found.
    pub connected: bool,
    /// True when battery is a last-known (not fresh) value — shown faded.
    #[serde(default)]
    pub stale: bool,
}

/// One metric's display readout for a Metrics tile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricReadout {
    pub label: String,
    pub value_text: String,
}

/// One graph line's current sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPoint {
    pub label: String,
    pub color: String,
    pub value: f64,
}

impl TileStatus {
    fn new(tile_id: &str, health: Health, detail: impl Into<String>) -> Self {
        Self {
            tile_id: tile_id.to_string(),
            health,
            detail: detail.into(),
            badge: None,
            value: None,
            link: None,
            metrics: Vec::new(),
            border_color: None,
            flash: false,
            graph_points: Vec::new(),
            bt_devices: Vec::new(),
            media: None,
            checked_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Dispatch a monitor check to its implementation.
pub async fn check(tile_id: &str, kind: &MonitorKind) -> TileStatus {
    match kind {
        MonitorKind::TcpPing { host, port } => tcp_ping(tile_id, host, *port).await,
        MonitorKind::HttpStatus {
            url,
            expected_status,
        } => http_status(tile_id, url, *expected_status).await,
        MonitorKind::Minecraft { host, port } => minecraft(tile_id, host, *port).await,
        MonitorKind::Jellyfin { base_url } => jellyfin(tile_id, base_url).await,
        MonitorKind::Price {
            url,
            css_selector,
            currency,
        } => price(tile_id, url, css_selector, currency.as_deref()).await,
        MonitorKind::YouTube {
            sources,
            channel_id: _,
            playlist_id: _,
        } => youtube(tile_id, sources).await,
        MonitorKind::Twitch {
            channel_login,
            client_id,
            token_secret_ref,
        } => twitch(tile_id, channel_login, client_id, token_secret_ref).await,
        MonitorKind::Email {
            imap_host,
            imap_port,
            username,
            password_secret_ref,
        } => email(tile_id, imap_host, *imap_port, username, password_secret_ref).await,
        MonitorKind::AiCredits {
            provider,
            api_key_secret_ref,
        } => ai_credits(tile_id, provider, api_key_secret_ref).await,
        MonitorKind::Discord {
            guild_id,
            channels,
            default_channel_id: _,
        } => discord(tile_id, guild_id, channels).await,
        MonitorKind::Api {
            url,
            method,
            auth_header,
            auth_token_secret_ref,
            json_path,
            prefix,
            suffix,
        } => {
            api_tile(
                tile_id,
                url,
                method,
                auth_header.as_deref(),
                auth_token_secret_ref.as_deref(),
                json_path.as_deref(),
                prefix.as_deref(),
                suffix.as_deref(),
            )
            .await
        }
        MonitorKind::Metrics { metrics } => metrics_tile(tile_id, metrics).await,
        MonitorKind::Graph { metrics } => graph_tile(tile_id, metrics).await,
        MonitorKind::Bluetooth { devices, low_threshold } => {
            bluetooth_tile(tile_id, devices, *low_threshold).await
        }
        MonitorKind::Media => media_tile(tile_id).await,
        MonitorKind::Kiro { low_threshold } => kiro_usage(tile_id, *low_threshold).await,
        MonitorKind::Slideshow { folder, .. } => slideshow_check(tile_id, folder).await,
        MonitorKind::Clock => TileStatus::new(tile_id, Health::Up, ""),
        MonitorKind::Water { .. } => TileStatus::new(tile_id, Health::Up, ""),
        MonitorKind::Counter { .. } => TileStatus::new(
            tile_id,
            Health::Up,
            crate::counter::get(tile_id).today.to_string(),
        ),
    }
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(NET_TIMEOUT)
        .user_agent("OnScreenWidget/0.1")
        // Don't chase redirects: health checks only need proof the server
        // answered, and following a redirect can lead to an internal address
        // (e.g. Jellyfin's LocalAddress behind NAT) this machine can't reach.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build http client")
}

// --- TCP ping (NAS reachability, generic ports) -----------------------------

async fn tcp_ping(tile_id: &str, host: &str, port: u16) -> TileStatus {
    let addr = format!("{host}:{port}");
    match timeout(NET_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => TileStatus::new(tile_id, Health::Up, format!("reachable • {addr}")),
        Ok(Err(e)) => TileStatus::new(tile_id, Health::Down, format!("unreachable • {e}")),
        Err(_) => TileStatus::new(tile_id, Health::Down, "timeout"),
    }
}

// --- HTTP status (website / generic HTTP health) ----------------------------

async fn http_status(tile_id: &str, url: &str, expected: u16) -> TileStatus {
    match http_client().get(url).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if code == expected {
                TileStatus::new(tile_id, Health::Up, format!("HTTP {code}"))
            } else if resp.status().is_success() {
                TileStatus::new(tile_id, Health::Warn, format!("HTTP {code} (expected {expected})"))
            } else if resp.status().is_redirection() {
                // Server answered with a redirect (e.g. http→https). Reachable.
                TileStatus::new(tile_id, Health::Up, format!("HTTP {code} (redirect)"))
            } else {
                TileStatus::new(tile_id, Health::Down, format!("HTTP {code}"))
            }
        }
        Err(e) => TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e))),
    }
}

// --- Jellyfin (public system info, no auth) ---------------------------------

async fn jellyfin(tile_id: &str, base_url: &str) -> TileStatus {
    let url = format!("{}/System/Info/Public", base_url.trim_end_matches('/'));
    match http_client().get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(j) => {
                    let name = j.get("ServerName").and_then(|v| v.as_str()).unwrap_or("Jellyfin");
                    let ver = j.get("Version").and_then(|v| v.as_str()).unwrap_or("?");
                    TileStatus::new(tile_id, Health::Up, format!("{name} • v{ver}"))
                }
                Err(_) => TileStatus::new(tile_id, Health::Warn, "online • bad response"),
            }
        }
        // A redirect (3xx) still proves the server answered. We don't follow it
        // (it may point at an unreachable internal address behind NAT), but we
        // treat it as reachable.
        Ok(resp) if resp.status().is_redirection() => {
            TileStatus::new(tile_id, Health::Up, "online • (redirect)")
        }
        Ok(resp) => TileStatus::new(tile_id, Health::Down, format!("HTTP {}", resp.status().as_u16())),
        Err(e) => TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e))),
    }
}

// --- Minecraft Server List Ping (Java edition) ------------------------------
//
// Implements the handshake + status request of the Minecraft protocol and
// parses the JSON status (MOTD, player counts, version).

async fn minecraft(tile_id: &str, host: &str, port: u16) -> TileStatus {
    match timeout(NET_TIMEOUT, minecraft_ping(host, port)).await {
        Ok(Ok(info)) => {
            let mut status = TileStatus::new(
                tile_id,
                Health::Up,
                format!("{}/{} players • {}", info.online, info.max, info.version),
            );
            status.badge = Some(info.online as i64);
            status
        }
        Ok(Err(e)) => TileStatus::new(tile_id, Health::Down, format!("offline • {e}")),
        Err(_) => TileStatus::new(tile_id, Health::Down, "timeout"),
    }
}

struct McInfo {
    online: i64,
    max: i64,
    version: String,
}

/// Minimal implementation of the Minecraft Server List Ping protocol.
async fn minecraft_ping(host: &str, port: u16) -> Result<McInfo, String> {
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| format!("connect: {e}"))?;

    // Handshake packet (next state = 1 for status).
    let mut data = Vec::new();
    write_varint(&mut data, 0x00); // packet id
    write_varint(&mut data, 47); // protocol version (any works for status)
    write_string(&mut data, host);
    data.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut data, 1); // next state: status

    let mut packet = Vec::new();
    write_varint(&mut packet, data.len() as i32);
    packet.extend_from_slice(&data);
    stream.write_all(&packet).await.map_err(|e| format!("write handshake: {e}"))?;

    // Status request packet (empty body, id 0x00).
    let req = [1u8, 0x00];
    stream.write_all(&req).await.map_err(|e| format!("write request: {e}"))?;

    // Read response: length varint, then packet id varint, then string.
    let _resp_len = read_varint(&mut stream).await?;
    let _packet_id = read_varint(&mut stream).await?;
    let json_len = read_varint(&mut stream).await? as usize;
    let mut buf = vec![0u8; json_len];
    stream.read_exact(&mut buf).await.map_err(|e| format!("read body: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_slice(&buf).map_err(|e| format!("parse status: {e}"))?;
    let online = json
        .pointer("/players/online")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let max = json
        .pointer("/players/max")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let version = json
        .pointer("/version/name")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();

    Ok(McInfo { online, max, version })
}

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut temp = (value & 0b0111_1111) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            temp |= 0b1000_0000;
        }
        buf.push(temp);
        if value == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

async fn read_varint(stream: &mut TcpStream) -> Result<i32, String> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await.map_err(|e| format!("read varint: {e}"))?;
        let value = (byte[0] & 0b0111_1111) as i32;
        result |= value << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err("varint too big".into());
        }
        if byte[0] & 0b1000_0000 == 0 {
            break;
        }
    }
    Ok(result)
}

// --- Price tracker (CSS selector extraction) --------------------------------

async fn price(tile_id: &str, url: &str, selector: &str, currency: Option<&str>) -> TileStatus {
    let html = match http_client().get(url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(t) => t,
            Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("read body • {}", short_err(&e))),
        },
        Ok(resp) => return TileStatus::new(tile_id, Health::Down, format!("HTTP {}", resp.status().as_u16())),
        Err(e) => return TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e))),
    };

    match extract_price(&html, selector) {
        Some((raw, value)) => {
            let sym = currency.unwrap_or("");
            let mut status = TileStatus::new(tile_id, Health::Up, format!("{sym}{raw}"));
            status.value = Some(value);
            status
        }
        None => TileStatus::new(tile_id, Health::Warn, "price not found • check selector"),
    }
}

/// Extract the first element matching `selector` and parse a numeric price from
/// its text. Returns (raw_text, parsed_value).
///
/// Dependency-free matcher (no `scraper`/`html5ever`). Supports the common
/// price-selector forms: a single compound simple-selector made of an optional
/// tag plus any number of `.class` / `#id` parts, e.g. `.price`, `span.amount`,
/// `#product-price`, `div#p.price.sale`. Descendant chains (`a b .price`) use
/// only the LAST compound (the target), which is correct for virtually all
/// price selectors. Unsupported CSS (`:nth-child`, `[attr=…]`, combinators `>`)
/// returns None → the tile shows "check selector".
fn extract_price(html: &str, selector: &str) -> Option<(String, f64)> {
    let sel = SimpleSelector::parse(selector)?;
    let text = sel.first_match_text(html)?;
    let text = text.trim().to_string();
    let value = parse_price_number(&text)?;
    Some((text, value))
}

/// A single compound CSS selector: optional tag + sets of required classes/id.
struct SimpleSelector {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
}

impl SimpleSelector {
    /// Parse the LAST whitespace-separated compound of a selector. Returns None
    /// for selectors containing unsupported CSS syntax.
    fn parse(selector: &str) -> Option<SimpleSelector> {
        // Reject any unsupported CSS syntax anywhere in the selector
        // (combinators, attribute/pseudo/universal selectors).
        if selector.contains('>')
            || selector.contains('+')
            || selector.contains('~')
            || selector.contains('[')
            || selector.contains(':')
            || selector.contains('*')
        {
            return None;
        }
        let last = selector.split_whitespace().last()?;
        let mut tag: Option<String> = None;
        let mut classes = Vec::new();
        let mut id: Option<String> = None;
        // Split into tokens on '.' and '#' boundaries while keeping the marker.
        let mut i = 0;
        let bytes = last.as_bytes();
        // Leading tag name (chars until first '.' or '#').
        let mut start = 0;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'#' {
            i += 1;
        }
        if i > start {
            tag = Some(last[start..i].to_ascii_lowercase());
        }
        // Remaining .class / #id parts.
        while i < bytes.len() {
            let marker = bytes[i];
            i += 1;
            start = i;
            while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'#' {
                i += 1;
            }
            let part = &last[start..i];
            if part.is_empty() {
                return None; // malformed, e.g. ".."
            }
            match marker {
                b'.' => classes.push(part.to_string()),
                b'#' => id = Some(part.to_string()),
                _ => return None,
            }
        }
        if tag.is_none() && classes.is_empty() && id.is_none() {
            return None;
        }
        Some(SimpleSelector { tag, classes, id })
    }

    /// Find the first element matching this selector and return its inner text
    /// (tags stripped). Scans opening tags linearly; good enough for price pages.
    fn first_match_text(&self, html: &str) -> Option<String> {
        let bytes = html.as_bytes();
        let mut pos = 0;
        while let Some(rel) = html[pos..].find('<') {
            let lt = pos + rel;
            // Skip comments / closing tags / declarations.
            if html[lt..].starts_with("<!--") {
                let end = html[lt..].find("-->").map(|e| lt + e + 3).unwrap_or(bytes.len());
                pos = end;
                continue;
            }
            if html[lt + 1..].starts_with('/') || html[lt + 1..].starts_with('!') {
                pos = lt + 1;
                continue;
            }
            // Find end of the opening tag.
            let gt = match html[lt..].find('>') {
                Some(e) => lt + e,
                None => break,
            };
            let tag_str = &html[lt + 1..gt]; // e.g. `span class="price" id="x"`
            let self_closing = tag_str.trim_end().ends_with('/');
            if self.matches_tag(tag_str) && !self_closing {
                // Capture inner HTML up to the matching close tag (first close of
                // the same tag name — sufficient for leaf price elements).
                let name = tag_name(tag_str);
                let inner_start = gt + 1;
                let close = format!("</{name}");
                let inner_end = html[inner_start..]
                    .find(&close)
                    .map(|e| inner_start + e)
                    .unwrap_or(bytes.len());
                let inner = &html[inner_start..inner_end];
                return Some(strip_tags(inner));
            }
            pos = gt + 1;
        }
        None
    }

    /// Does an opening-tag string satisfy tag + all classes + id?
    fn matches_tag(&self, tag_str: &str) -> bool {
        let name = tag_name(tag_str);
        if let Some(t) = &self.tag {
            if !name.eq_ignore_ascii_case(t) {
                return false;
            }
        }
        if let Some(want_id) = &self.id {
            match attr_value(tag_str, "id") {
                Some(v) if v == *want_id => {}
                _ => return false,
            }
        }
        if !self.classes.is_empty() {
            let class_attr = attr_value(tag_str, "class").unwrap_or_default();
            let have: Vec<&str> = class_attr.split_whitespace().collect();
            for want in &self.classes {
                if !have.iter().any(|c| c == want) {
                    return false;
                }
            }
        }
        true
    }
}

/// Tag name from an opening-tag body (`span class=…` → `span`).
fn tag_name(tag_str: &str) -> String {
    tag_str
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Extract an attribute value (double or single quoted) from an opening tag.
fn attr_value(tag_str: &str, attr: &str) -> Option<String> {
    let lower = tag_str.to_ascii_lowercase();
    let mut from = 0;
    loop {
        let idx = lower[from..].find(attr)? + from;
        // Ensure it's a standalone attribute (preceded by whitespace or start).
        let ok_before = idx == 0 || tag_str.as_bytes()[idx - 1].is_ascii_whitespace();
        let after = idx + attr.len();
        let eq = tag_str[after..].trim_start();
        if ok_before && eq.starts_with('=') {
            let rest = eq[1..].trim_start();
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = rest[1..].find(quote)? + 1;
                return Some(rest[1..end].to_string());
            }
        }
        from = after;
        if from >= lower.len() {
            return None;
        }
    }
}

/// Remove any nested tags from a fragment, returning concatenated text.
fn strip_tags(fragment: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in fragment.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Pull a float out of a messy price string like "$1,299.00" or "€ 42,99".
fn parse_price_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    // Heuristic: if both separators present, assume last one is the decimal.
    let normalized = if cleaned.contains('.') && cleaned.contains(',') {
        if cleaned.rfind('.') > cleaned.rfind(',') {
            cleaned.replace(',', "")
        } else {
            cleaned.replace('.', "").replace(',', ".")
        }
    } else if cleaned.contains(',') {
        // Treat lone comma as decimal only if it looks like cents (2 trailing digits).
        if cleaned.rsplit(',').next().map(|x| x.len()) == Some(2) {
            cleaned.replace(',', ".")
        } else {
            cleaned.replace(',', "")
        }
    } else {
        cleaned
    };
    normalized.parse::<f64>().ok()
}

// --- YouTube (public RSS feeds — no API key needed) -------------------------

async fn youtube(tile_id: &str, sources: &[crate::model::YouTubeSource]) -> TileStatus {
    use crate::model::YouTubeSourceKind;

    if sources.is_empty() {
        return TileStatus::new(tile_id, Health::Unknown, "add a channel or playlist");
    }

    // Fetch each source's feed and collect (published, title, NotifyItem).
    let mut collected: Vec<(String, String, crate::notify::NotifyItem)> = Vec::new();
    let mut any_ok = false;
    let mut latest_title = String::new();
    let mut latest_pub = String::new();

    for src in sources {
        if src.id.is_empty() {
            continue;
        }
        let url = match src.kind {
            YouTubeSourceKind::Playlist => {
                format!("https://www.youtube.com/feeds/videos.xml?playlist_id={}", src.id)
            }
            YouTubeSourceKind::Channel => {
                format!("https://www.youtube.com/feeds/videos.xml?channel_id={}", src.id)
            }
        };
        let xml = match http_client().get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(t) => t,
                Err(_) => continue,
            },
            _ => continue,
        };
        any_ok = true;

        // Walk every <entry> and pull videoId, title, published.
        let mut rest = xml.as_str();
        while let Some(start) = rest.find("<entry>") {
            let after = &rest[start + "<entry>".len()..];
            let end = match after.find("</entry>") {
                Some(e) => e,
                None => break,
            };
            let entry = &after[..end];
            if let Some(id) = extract_between(entry, "<yt:videoId>", "</yt:videoId>") {
                let title = extract_between(entry, "<title>", "</title>")
                    .unwrap_or("new video")
                    .to_string();
                let published = extract_between(entry, "<published>", "</published>")
                    .unwrap_or("")
                    .to_string();
                if published > latest_pub {
                    latest_pub = published.clone();
                    latest_title = title.clone();
                }
                collected.push((
                    published,
                    title.clone(),
                    crate::notify::NotifyItem {
                        id: id.to_string(),
                        url: format!("https://www.youtube.com/watch?v={id}"),
                        title,
                    },
                ));
            }
            rest = &after[end + "</entry>".len()..];
        }
    }

    if !any_ok {
        return TileStatus::new(tile_id, Health::Down, "feeds unreachable");
    }

    // Sort newest-first by RFC3339 published timestamp (lexicographic works).
    collected.sort_by(|a, b| b.0.cmp(&a.0));
    let items: Vec<crate::notify::NotifyItem> =
        collected.iter().map(|(_, _, it)| it.clone()).collect();

    // Update the persistent notification stack; badge = unseen count.
    let unseen = crate::notify::record_feed(tile_id, &items);
    let _ = latest_title; // no longer used for the detail directly

    // The description reflects what a single click will do:
    //  * notifications present → the next video's title (top of the stack);
    //  * none → a neutral message (single click opens YouTube home).
    let detail = if unseen > 0 {
        match crate::notify::peek_title(tile_id) {
            Some(t) if !t.is_empty() => format!("▶ {t}"),
            _ => format!("{unseen} new"),
        }
    } else {
        "no new uploads • opens YouTube".to_string()
    };
    let mut status = TileStatus::new(tile_id, Health::Up, detail);
    status.badge = Some(unseen);
    status
}

// --- Twitch (Helix streams endpoint) ----------------------------------------

async fn twitch(tile_id: &str, login: &str, client_id: &str, token_ref: &str) -> TileStatus {
    let token = match secrets::get_secret(token_ref) {
        Ok(Some(t)) => t,
        Ok(None) => return TileStatus::new(tile_id, Health::Unknown, "no token • set in settings"),
        Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("secret • {e}")),
    };
    let url = format!("https://api.twitch.tv/helix/streams?user_login={login}");
    let resp = http_client()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(token)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(j) => {
                let live = j.get("data").and_then(|d| d.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
                if live {
                    let viewers = j
                        .pointer("/data/0/viewer_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let mut status = TileStatus::new(tile_id, Health::Up, format!("LIVE • {viewers} viewers"));
                    status.badge = Some(viewers);
                    status
                } else {
                    TileStatus::new(tile_id, Health::Down, "offline")
                }
            }
            Err(e) => TileStatus::new(tile_id, Health::Unknown, format!("parse • {}", short_err(&e))),
        },
        Ok(r) => TileStatus::new(tile_id, Health::Warn, format!("HTTP {}", r.status().as_u16())),
        Err(e) => TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e))),
    }
}

// --- Email (IMAP unread count) ----------------------------------------------
//
// NOTE: a full IMAP client is added in a later phase. For now this reports
// Unknown with guidance so the tile and config path are exercised end-to-end.

async fn email(tile_id: &str, _host: &str, _port: u16, _user: &str, pw_ref: &str) -> TileStatus {
    match secrets::get_secret(pw_ref) {
        Ok(Some(_)) => TileStatus::new(tile_id, Health::Unknown, "IMAP check pending (phase 4)"),
        Ok(None) => TileStatus::new(tile_id, Health::Unknown, "no password • set in settings"),
        Err(e) => TileStatus::new(tile_id, Health::Unknown, format!("secret • {e}")),
    }
}

// --- AI credits -------------------------------------------------------------
//
// Provider-specific balance endpoints vary; this dispatches per provider and
// is extended as endpoints are confirmed.

async fn ai_credits(tile_id: &str, provider: &str, key_ref: &str) -> TileStatus {
    let _key = match secrets::get_secret(key_ref) {
        Ok(Some(k)) => k,
        Ok(None) => return TileStatus::new(tile_id, Health::Unknown, "no api key • set in settings"),
        Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("secret • {e}")),
    };
    TileStatus::new(
        tile_id,
        Health::Unknown,
        format!("{provider} balance check pending (phase 4)"),
    )
}

// --- AWS Kiro usage ---------------------------------------------------------
//
// Reads plan credits by running the Kiro CLI's `/usage` report in
// non-interactive mode. The CLI authenticates via the already-logged-in AWS
// Builder ID, so no API key is needed. Output is ANSI-decorated and shaped like:
//
//   Estimated Usage | resets on 2026-11-01 | KIRO PRO MAX
//   Credits (2834.13 of 5000 covered in plan), 56.7%
//
// We parse used/limit from the "Credits (X of Y covered in plan)" line and show
// the remaining credits. The call is blocking (spawns a process), so it runs on
// the blocking pool off the async runtime.

async fn kiro_usage(tile_id: &str, low_threshold: f64) -> TileStatus {
    let out = tokio::task::spawn_blocking(run_kiro_usage).await;
    let raw = match out {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return TileStatus::new(
                tile_id,
                Health::Unknown,
                format!("kiro cli • {e}"),
            )
        }
        Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("join • {e}")),
    };

    let clean = strip_ansi(&raw);
    match parse_kiro_credits(&clean) {
        Some((used, limit, plan)) => {
            let remaining = (limit - used).max(0.0);
            let health = if remaining <= low_threshold {
                Health::Warn
            } else {
                Health::Up
            };
            let plan_sfx = plan
                .map(|p| format!(" • {p}"))
                .unwrap_or_default();
            TileStatus {
                value: Some(remaining),
                badge: Some(remaining.round() as i64),
                ..TileStatus::new(
                    tile_id,
                    health,
                    format!("{} left • {} / {}{}", fmt_credits(remaining), fmt_credits(used), fmt_credits(limit), plan_sfx),
                )
            }
        }
        None => {
            // CLI ran but we couldn't find the credits line — surface a hint.
            let hint = if clean.to_lowercase().contains("login")
                || clean.to_lowercase().contains("not logged")
                || clean.to_lowercase().contains("authenticat")
            {
                "not logged in • run `kiro-cli login` in WSL"
            } else {
                "usage not found in CLI output"
            };
            TileStatus::new(tile_id, Health::Unknown, hint)
        }
    }
}

/// Run the Kiro CLI `/usage` report and return its combined stdout+stderr.
///
/// On Windows the logged-in Kiro CLI lives inside **WSL** (e.g.
/// `~/.local/bin/kiro-cli`), not as a native Windows binary — so we invoke it
/// through `wsl.exe` using a login shell (`bash -lc`) so the user's PATH is set
/// up and `kiro-cli`/`kiro` resolves. On Linux/macOS we run the binary directly.
/// Blocking — call via spawn_blocking.
fn run_kiro_usage() -> Result<String, String> {
    use std::process::Command;

    // The command run inside the shell: try `kiro-cli`, fall back to `kiro`.
    const INNER: &str = "if command -v kiro-cli >/dev/null 2>&1; then \
                         kiro-cli chat --no-interactive /usage; \
                         else kiro chat --no-interactive /usage; fi";

    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("wsl.exe");
        // `wsl.exe bash -lc "<INNER>"` — login shell loads PATH (~/.local/bin).
        c.args(["bash", "-lc", INNER]);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("bash");
        c.args(["-lc", INNER]);
        c
    };

    match cmd.output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push('\n');
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            Ok(s)
        }
        Err(e) => Err(format!("wsl/kiro launch failed: {e}")),
    }
}

/// Strip ANSI/VT escape sequences (CSI `ESC [ … letter`) from CLI output.
fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC: skip an optional '[' then params until a final letter byte.
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() && !(bytes[i] as char).is_ascii_alphabetic() {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1; // consume the final letter
                }
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Parse "(USED of LIMIT covered in plan)" and the plan name from the cleaned
/// CLI `/usage` output. Returns (used, limit, plan_name).
fn parse_kiro_credits(clean: &str) -> Option<(f64, f64, Option<String>)> {
    // Find "( <num> of <num> " — robust to surrounding wording/punctuation.
    let lower = clean;
    let open = lower.find('(')?;
    let rest = &lower[open + 1..];
    // Pull the two numbers around " of ".
    let of = rest.find(" of ")?;
    let used_str: String = rest[..of].chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let after = &rest[of + 4..];
    let limit_str: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ' ')
        .collect::<String>()
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let used: f64 = used_str.parse().ok()?;
    let limit: f64 = limit_str.parse().ok()?;

    // Plan name: last segment after a '|' on the "Estimated Usage | … | PLAN"
    // line, if present.
    let plan = clean
        .lines()
        .find(|l| l.contains('|'))
        .and_then(|l| l.rsplit('|').next())
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    Some((used, limit, plan))
}

/// Format a credit amount compactly: integer if whole, else one decimal.
fn fmt_credits(v: f64) -> String {
    if (v.fract()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

// --- Discord (public guild widget.json voice presence) ----------------------
//
// Requires the server Widget to be enabled (Server Settings -> Widget). The
// endpoint returns a `channels` array (voice channels) and a `members` array
// where connected voice members have a `channel_id` field. We filter to the
// user's listed channels, count members per channel, and summarize.

async fn discord(tile_id: &str, guild_id: &str, channels: &[DiscordChannel]) -> TileStatus {
    let url = format!("https://discord.com/api/guilds/{guild_id}/widget.json");
    let resp = http_client().get(&url).send().await;
    let json = match resp {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(j) => j,
            Err(e) => {
                return TileStatus::new(tile_id, Health::Unknown, format!("parse • {}", short_err(&e)))
            }
        },
        Ok(r) if r.status().as_u16() == 403 => {
            return TileStatus::new(
                tile_id,
                Health::Warn,
                "widget disabled • enable in Server Settings → Widget",
            )
        }
        Ok(r) => {
            return TileStatus::new(tile_id, Health::Down, format!("HTTP {}", r.status().as_u16()))
        }
        Err(e) => {
            return TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e)))
        }
    };

    // Map voice channel id -> name from the widget's channels[] array.
    let mut channel_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(chs) = json.get("channels").and_then(|c| c.as_array()) {
        for c in chs {
            if let (Some(id), Some(name)) = (
                c.get("id").and_then(|v| v.as_str()),
                c.get("name").and_then(|v| v.as_str()),
            ) {
                channel_names.insert(id.to_string(), name.to_string());
            }
        }
    }

    // Collect connected voice members grouped by channel id. Bots and online
    // members without a `channel_id` are ignored (they're not in a call).
    let members = json
        .get("members")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let mut per_channel: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for m in &members {
        if let Some(cid) = m.get("channel_id").and_then(|v| v.as_str()) {
            let name = m
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("someone")
                .to_string();
            per_channel.entry(cid.to_string()).or_default().push(name);
        }
    }

    // If the user configured a specific channel filter, restrict to those ids;
    // otherwise watch ALL voice channels that currently have people.
    let filter_ids: Option<std::collections::HashSet<&str>> = if channels.is_empty() {
        None
    } else {
        Some(channels.iter().map(|c| c.channel_id.as_str()).collect())
    };

    let mut total = 0i64;
    let mut parts: Vec<String> = Vec::new();
    // Stable, readable ordering by channel name.
    let mut entries: Vec<(&String, &Vec<String>)> = per_channel.iter().collect();
    entries.sort_by(|a, b| {
        let an = channel_names.get(a.0).map(|s| s.as_str()).unwrap_or(a.0.as_str());
        let bn = channel_names.get(b.0).map(|s| s.as_str()).unwrap_or(b.0.as_str());
        an.cmp(bn)
    });
    for (cid, names) in entries {
        if names.is_empty() {
            continue;
        }
        if let Some(ref ids) = filter_ids {
            if !ids.contains(cid.as_str()) {
                continue;
            }
        }
        // Prefer the widget's channel name; fall back to a configured label, then the id.
        let label = channel_names
            .get(cid)
            .cloned()
            .or_else(|| {
                channels
                    .iter()
                    .find(|c| &c.channel_id == cid)
                    .map(|c| c.label.clone())
            })
            .unwrap_or_else(|| cid.clone());
        total += names.len() as i64;
        parts.push(format!("{label}: {}", names.join(", ")));
    }

    if total > 0 {
        let mut status = TileStatus::new(tile_id, Health::Up, parts.join(" • "));
        status.badge = Some(total);
        return status;
    }

    let mut status = TileStatus::new(tile_id, Health::Down, "no one in call");
    status.badge = Some(0);
    status
}

// --- Generic API tile -------------------------------------------------------
//
// Fetches a URL and displays a value pulled from the JSON response via a
// dot/bracket path. Response layout is not fixed: you point the tile at the
// field you want (e.g. "data.0.value"); if the body isn't JSON or the path is
// empty, the raw body (truncated) is shown.

#[allow(clippy::too_many_arguments)]
async fn api_tile(
    tile_id: &str,
    url: &str,
    method: &str,
    auth_header: Option<&str>,
    auth_token_secret_ref: Option<&str>,
    json_path: Option<&str>,
    prefix: Option<&str>,
    suffix: Option<&str>,
) -> TileStatus {
    let m = match reqwest::Method::from_bytes(method.to_uppercase().as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            return TileStatus::new(tile_id, Health::Unknown, format!("bad method: {method}"))
        }
    };
    let mut req = http_client().request(m, url);

    // Optional auth token from the keychain.
    if let (Some(header), Some(secret_ref)) = (auth_header, auth_token_secret_ref) {
        if !header.is_empty() && !secret_ref.is_empty() {
            match secrets::get_secret(secret_ref) {
                Ok(Some(token)) => req = req.header(header, token),
                Ok(None) => {
                    return TileStatus::new(tile_id, Health::Unknown, "no api token • set in settings")
                }
                Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("secret • {e}")),
            }
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return TileStatus::new(tile_id, Health::Down, format!("error • {}", short_err(&e))),
    };
    let status_code = resp.status();
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return TileStatus::new(tile_id, Health::Unknown, format!("read • {}", short_err(&e))),
    };

    // Extract the value: JSON path if the body parses as JSON, else raw body.
    let value_text = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(json) => {
            let picked = match json_path {
                Some(p) if !p.is_empty() => json_path_get(&json, p),
                _ => Some(json.clone()),
            };
            match picked {
                Some(v) => json_value_to_string(&v),
                None => "path not found".to_string(),
            }
        }
        Err(_) => body.trim().chars().take(80).collect::<String>(),
    };

    let detail = format!(
        "{}{}{}",
        prefix.unwrap_or(""),
        value_text,
        suffix.unwrap_or("")
    );
    let health = if status_code.is_success() {
        Health::Up
    } else {
        Health::Warn
    };
    TileStatus::new(tile_id, health, detail)
}

/// Navigate a dot/bracket path like `data.items.0.name` into a JSON value.
fn json_path_get(root: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut cur = root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        // Support both "items.0" and "items[0]" styles.
        let seg = seg.trim_end_matches(']');
        let (key, bracket_idx) = match seg.split_once('[') {
            Some((k, i)) => (k, Some(i)),
            None => (seg, None),
        };

        // First descend by key if there is a non-empty key part.
        if !key.is_empty() {
            if let Ok(n) = key.parse::<usize>() {
                // Pure numeric key = array index.
                cur = cur.get(n)?;
            } else {
                cur = cur.get(key)?;
            }
        }

        // Then apply any bracket index, e.g. items[0].
        if let Some(i) = bracket_idx {
            let n: usize = i.parse().ok()?;
            cur = cur.get(n)?;
        }
    }
    Some(cur.clone())
}

/// Render a JSON value as a compact display string.
fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string().chars().take(80).collect(),
    }
}

// --- Metrics & Graph (shared sampling) --------------------------------------

/// Shared sysinfo System, refreshed on each sample. CPU% needs two refreshes
/// spaced apart; the poll interval provides that spacing.
fn system() -> &'static Mutex<sysinfo::System> {
    static SYS: OnceLock<Mutex<sysinfo::System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(sysinfo::System::new_all()))
}

/// Cached CPU/memory/core sample + the instant it was taken. sysinfo needs at
/// least `MINIMUM_CPU_UPDATE_INTERVAL` (~200ms) between refreshes to report a
/// correct CPU %. Sub-second metrics/graph polling would otherwise refresh too
/// fast and massively over-report CPU, so we only re-measure past that interval
/// and reuse the last good values in between. This also stops a full
/// process-list refresh from running many times per second.
fn cpu_cache() -> &'static Mutex<Option<(std::time::Instant, f64, f64, f64)>> {
    static C: OnceLock<Mutex<Option<(std::time::Instant, f64, f64, f64)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// Shared network sampler: cumulative counters + the instant of the last read,
/// used to derive MB/s deltas between samples.
fn net_state() -> &'static Mutex<Option<(std::collections::HashMap<String, (u64, u64)>, std::time::Instant)>> {
    static NET: OnceLock<Mutex<Option<(std::collections::HashMap<String, (u64, u64)>, std::time::Instant)>>> =
        OnceLock::new();
    NET.get_or_init(|| Mutex::new(None))
}

/// One pass of system sampling, reused by metrics & graph tiles.
struct Samplers {
    core_count: f64,
    sys_cpu: f64,
    sys_mem_pct: f64,
    self_pid: u32,
    components: sysinfo::Components,
    /// Full LibreHardwareMonitor reading snapshot for this sample. Empty if the
    /// LHM web server isn't reachable.
    hw_readings: Vec<crate::lhm::Reading>,
    disks: sysinfo::Disks,
    /// interface name -> (rx MB/s, tx MB/s)
    net_rates: std::collections::HashMap<String, (f64, f64)>,
}

fn gather_samplers() -> Samplers {
    use sysinfo::{Components, ProcessRefreshKind, RefreshKind};
    let (core_count, sys_cpu, sys_mem_pct) = {
        // Only perform the expensive CPU + process refresh when enough time has
        // passed for sysinfo to compute an accurate %. Otherwise reuse the last
        // good values. This keeps readings Task-Manager-accurate and avoids a
        // refresh storm under sub-second polling.
        const MIN_INTERVAL: Duration = Duration::from_millis(250);
        let mut cache = cpu_cache().lock().unwrap();
        let fresh_enough = cache
            .as_ref()
            .map(|(t, ..)| t.elapsed() < MIN_INTERVAL)
            .unwrap_or(false);
        if fresh_enough {
            let (_, cores, scpu, smem) = cache.as_ref().unwrap();
            (*cores, *scpu, *smem)
        } else {
            let mut sys = system().lock().unwrap();
            sys.refresh_specifics(
                RefreshKind::new()
                    .with_cpu(sysinfo::CpuRefreshKind::everything())
                    .with_memory(sysinfo::MemoryRefreshKind::everything())
                    .with_processes(ProcessRefreshKind::new().with_cpu().with_memory()),
            );
            let cores = sys.cpus().len().max(1) as f64;
            let scpu = sys.global_cpu_usage() as f64;
            let total = sys.total_memory() as f64;
            let used = sys.used_memory() as f64;
            let smem = if total > 0.0 { used / total * 100.0 } else { 0.0 };
            *cache = Some((std::time::Instant::now(), cores, scpu, smem));
            (cores, scpu, smem)
        }
    };

    // Network throughput: delta of cumulative rx/tx bytes over elapsed time.
    let net_rates = {
        let networks = sysinfo::Networks::new_with_refreshed_list();
        let mut cur: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
        for (name, data) in &networks {
            cur.insert(name.clone(), (data.total_received(), data.total_transmitted()));
        }
        let now = std::time::Instant::now();
        let mut rates: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
        let mut guard = net_state().lock().unwrap();
        if let Some((prev, prev_t)) = guard.as_ref() {
            let dt = now.duration_since(*prev_t).as_secs_f64().max(1e-6);
            for (name, (rx, tx)) in &cur {
                if let Some((prx, ptx)) = prev.get(name) {
                    let d_rx = rx.saturating_sub(*prx) as f64 / (1024.0 * 1024.0) / dt;
                    let d_tx = tx.saturating_sub(*ptx) as f64 / (1024.0 * 1024.0) / dt;
                    rates.insert(name.clone(), (d_rx, d_tx));
                }
            }
        }
        *guard = Some((cur, now));
        rates
    };

    Samplers {
        core_count,
        sys_cpu,
        sys_mem_pct,
        self_pid: std::process::id(),
        components: Components::new_with_refreshed_list(),
        hw_readings: hwinfo_readings_cached(),
        disks: sysinfo::Disks::new_with_refreshed_list(),
        net_rates,
    }
}

/// Cached LibreHardwareMonitor reading snapshot. LHM is read over local HTTP,
/// so we cache for ~750ms to avoid hammering its web server under sub-second
/// metrics/graph polling.
fn hwinfo_readings_cached() -> Vec<crate::lhm::Reading> {
    static CACHE: OnceLock<Mutex<Option<(std::time::Instant, Vec<crate::lhm::Reading>)>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some((t, v)) = guard.as_ref() {
        if t.elapsed() < Duration::from_millis(750) {
            return v.clone();
        }
    }
    let fresh = crate::lhm::read_all_blocking();
    *guard = Some((std::time::Instant::now(), fresh.clone()));
    fresh
}

/// Compute a single metric's current value from the sampled state.
fn sample_value(spec: &MetricSpec, s: &Samplers) -> Option<f64> {
    use sysinfo::Pid;
    match &spec.source {
        MetricSource::AppCpu { process } | MetricSource::AppMemory { process } => {
            let sys = system().lock().unwrap();
            let needle = process.trim().to_lowercase();
            let want_cpu = matches!(spec.source, MetricSource::AppCpu { .. });
            if needle.is_empty() {
                let p = sys.process(Pid::from_u32(s.self_pid))?;
                return Some(if want_cpu {
                    p.cpu_usage() as f64 / s.core_count
                } else {
                    p.memory() as f64 / (1024.0 * 1024.0)
                });
            }
            let mut cpu = 0.0;
            let mut mem = 0.0;
            let mut matched = false;
            for p in sys.processes().values() {
                if p.name().to_string_lossy().to_lowercase().contains(&needle) {
                    matched = true;
                    cpu += p.cpu_usage() as f64;
                    mem += p.memory() as f64 / (1024.0 * 1024.0);
                }
            }
            if !matched {
                return None;
            }
            Some(if want_cpu { cpu / s.core_count } else { mem })
        }
        MetricSource::SystemCpu => Some(s.sys_cpu),
        MetricSource::SystemMemory => Some(s.sys_mem_pct),
        MetricSource::Temperature { component } => {
            // Prefer LibreHardwareMonitor temperatures (CPU/GPU/VRAM/hotspot/
            // mobo/etc.); fall back to the OS sensors sysinfo exposes if LHM
            // isn't reachable.
            temperature_from_hwinfo(&s.hw_readings, component)
                .or_else(|| temperature_for(&s.components, component))
        }
        MetricSource::HardwareSensor { sensor } | MetricSource::LhmSensor { sensor } => {
            crate::lhm::value_for(&s.hw_readings, sensor)
        }
        MetricSource::Disk { mount, metric } => disk_value(&s.disks, mount, *metric),
        MetricSource::NetworkThroughput { interface, direction } => {
            let needle = interface.trim().to_lowercase();
            let mut sum = 0.0;
            let mut any = false;
            for (name, (rx, tx)) in &s.net_rates {
                if needle.is_empty() || name.to_lowercase().contains(&needle) {
                    any = true;
                    sum += match direction {
                        crate::model::NetDir::In => *rx,
                        crate::model::NetDir::Out => *tx,
                    };
                }
            }
            if any {
                Some(sum)
            } else {
                // First sample (no prior delta) → 0 so the graph starts cleanly.
                Some(0.0)
            }
        }
    }
}

/// Format a metric value for display (disk GB→TB over 1000, else 2 decimals).
fn format_metric(spec: &MetricSpec, value: Option<f64>) -> String {
    let is_disk_size = matches!(
        &spec.source,
        MetricSource::Disk { metric, .. } if !matches!(metric, DiskMetric::Percent)
    );
    let is_hw = matches!(
        &spec.source,
        MetricSource::HardwareSensor { .. } | MetricSource::LhmSensor { .. }
    );
    match value {
        Some(v) if is_disk_size && v > 1000.0 => format!("{}: {:.2}TB", spec.label, v / 1000.0),
        // Hardware sensors: keep readouts compact — integers for large values
        // (RPM, MHz, W), 1–2 decimals for small ones (V, A, fractional °C).
        Some(v) if is_hw => {
            let a = v.abs();
            let num = if a >= 100.0 {
                format!("{:.0}", v)
            } else if a >= 10.0 {
                format!("{:.1}", v)
            } else {
                format!("{:.2}", v)
            };
            format!("{}: {}{}", spec.label, num, spec.unit)
        }
        Some(v) => format!("{}: {:.2}{}", spec.label, v, spec.unit),
        None => format!("{}: n/a", spec.label),
    }
}

/// From specs + their current values, compute the border colour (highest
/// severity gradient) and whether to flash (any metric meets its FINAL
/// list-order threshold; single-threshold metrics never flash).
fn border_and_flash(specs: &[MetricSpec], values: &[Option<f64>]) -> (Option<String>, bool) {
    let mut best_severity = -1.0_f64;
    let mut border: Option<String> = None;
    let mut flash = false;
    for (spec, value) in specs.iter().zip(values.iter()) {
        if let Some(v) = *value {
            let (sev, color) = severity_and_color(v, &spec.thresholds);
            if sev > best_severity {
                best_severity = sev;
                border = color;
            }
            if spec.thresholds.len() >= 2 {
                if let Some(final_stop) = spec.thresholds.last() {
                    if final_stop.op.eval(v, final_stop.value) {
                        flash = true;
                        border = Some(final_stop.color.clone());
                    }
                }
            }
        }
    }
    (border, flash)
}

async fn metrics_tile(tile_id: &str, specs: &[MetricSpec]) -> TileStatus {
    let s = gather_samplers();
    let specs4: Vec<&MetricSpec> = specs.iter().take(4).collect();
    let values: Vec<Option<f64>> = specs4.iter().map(|sp| sample_value(sp, &s)).collect();

    let readouts: Vec<MetricReadout> = specs4
        .iter()
        .zip(values.iter())
        .map(|(sp, v)| MetricReadout {
            label: sp.label.clone(),
            value_text: format_metric(sp, *v),
        })
        .collect();

    let owned: Vec<MetricSpec> = specs4.iter().map(|sp| (*sp).clone()).collect();
    let (border, flash) = border_and_flash(&owned, &values);

    let mut status = TileStatus::new(tile_id, Health::Up, "");
    status.detail = readouts
        .iter()
        .map(|r| r.value_text.clone())
        .collect::<Vec<_>>()
        .join(" • ");
    status.metrics = readouts;
    status.border_color = border;
    status.flash = flash;
    status
}

async fn graph_tile(tile_id: &str, specs: &[MetricSpec]) -> TileStatus {
    let s = gather_samplers();
    let values: Vec<Option<f64>> = specs.iter().map(|sp| sample_value(sp, &s)).collect();

    let points: Vec<GraphPoint> = specs
        .iter()
        .zip(values.iter())
        .map(|(sp, v)| GraphPoint {
            label: sp.label.clone(),
            color: sp.line_color.clone(),
            value: v.unwrap_or(0.0),
        })
        .collect();

    let (border, flash) = border_and_flash(specs, &values);

    let mut status = TileStatus::new(tile_id, Health::Up, "");
    status.detail = specs
        .iter()
        .zip(values.iter())
        .map(|(sp, v)| format_metric(sp, *v))
        .collect::<Vec<_>>()
        .join(" • ");
    status.graph_points = points;
    status.border_color = border;
    status.flash = flash;
    status
}

/// Guess a device-type emoji from the device name.
fn bt_icon_for(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("wh-") || n.contains("headphone") || n.contains("headset")
        || n.contains("buds") || n.contains("airpods") || n.contains("sony")
    {
        "🎧"
    } else if n.contains("mouse") || n.contains("mx master") || n.contains("mx anywhere") {
        "🖱️"
    } else if n.contains("keyboard") || n.contains("keys") || n.contains("keeb")
        || n.contains("epomaker") || n.contains("feker") || n.contains("galaxy")
    {
        "⌨️"
    } else if n.contains("dualsense") || n.contains("dualshock") || n.contains("controller")
        || n.contains("gamepad")
    {
        "🎮"
    } else {
        "🔵"
    }
}

/// Build a Bluetooth tile: one cell per device added in the tile settings.
async fn bluetooth_tile(tile_id: &str, specs: &[BtDeviceSpec], low: u8) -> TileStatus {
    // read_devices() shells out to PowerShell (blocking). Run it on the blocking
    // thread pool so it never stalls a tokio async worker (which would freeze
    // window commands and other tiles).
    let found = tokio::task::spawn_blocking(crate::bluetooth::read_devices)
        .await
        .unwrap_or_default();

    // Show only the devices the user added in the tile settings, in order,
    // matched by name. A configured device that isn't connected shows offline.
    let mut cells: Vec<BtDeviceReadout> = Vec::new();
    {
        let mut used = vec![false; found.len()];
        for spec in specs.iter().filter(|s| !s.name_match.trim().is_empty()) {
            let needle = spec.name_match.trim().to_lowercase();
            // Prefer an exact name match, then a substring match.
            let matched_idx = found
                .iter()
                .enumerate()
                .position(|(i, d)| !used[i] && d.name.to_lowercase() == needle)
                .or_else(|| {
                    found.iter().enumerate().position(|(i, d)| {
                        !used[i] && d.name.to_lowercase().contains(&needle)
                    })
                });
            match matched_idx {
                Some(i) => {
                    used[i] = true;
                    let d = &found[i];
                    cells.push(make_cell(
                        &d.name,
                        d.battery,
                        d.charging,
                        spec.icon.clone(),
                        spec.label.clone(),
                        low,
                        d.stale,
                    ));
                }
                None => {
                    // Configured but not connected: a disconnected placeholder.
                    let label = spec
                        .label
                        .clone()
                        .filter(|l| !l.is_empty())
                        .unwrap_or_else(|| {
                            if spec.name_match.trim().is_empty() {
                                "—".to_string()
                            } else {
                                spec.name_match.clone()
                            }
                        });
                    let icon = spec
                        .icon
                        .clone()
                        .filter(|i| !i.is_empty())
                        .unwrap_or_else(|| bt_icon_for(&spec.name_match).to_string());
                    cells.push(BtDeviceReadout {
                        label,
                        icon,
                        battery: None,
                        charging: false,
                        low: false,
                        connected: false,
                        stale: false,
                    });
                }
            }
        }
    }

    let detail = cells
        .iter()
        .map(|c| match (c.connected, c.battery) {
            (true, Some(b)) => format!("{} {}%", c.label, b),
            (true, None) => format!("{} —", c.label),
            (false, _) => format!("{} (disconnected)", c.label),
        })
        .collect::<Vec<_>>()
        .join(" • ");

    let any = cells.iter().any(|c| c.battery.is_some());
    let mut status = TileStatus::new(
        tile_id,
        if any { Health::Up } else { Health::Unknown },
        detail,
    );
    status.bt_devices = cells;
    status
}

/// Assemble a single device cell with icon/label/low resolution.
fn make_cell(
    name: &str,
    battery: Option<u8>,
    charging: bool,
    icon_override: Option<String>,
    label_override: Option<String>,
    low: u8,
    stale: bool,
) -> BtDeviceReadout {
    let label = label_override
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| name.to_string());
    let icon = icon_override
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| bt_icon_for(name).to_string());
    let is_low = battery.map(|b| b <= low).unwrap_or(false);
    BtDeviceReadout {
        label,
        icon,
        battery,
        charging,
        low: is_low,
        connected: true,
        stale,
    }
}

/// Build a Media tile from the current system media session.
async fn media_tile(tile_id: &str) -> TileStatus {
    let np = crate::media::now_playing();
    let detail = if np.active {
        if np.artist.is_empty() {
            np.title.clone()
        } else {
            format!("{} — {}", np.title, np.artist)
        }
    } else {
        "Nothing playing".to_string()
    };
    let mut status = TileStatus::new(
        tile_id,
        if np.active { Health::Up } else { Health::Unknown },
        detail,
    );
    status.media = Some(MediaReadout {
        title: np.title,
        artist: np.artist,
        app: np.app,
        playing: np.playing,
        active: np.active,
        art: np.art,
    });
    status
}

/// Readings for the editor dropdowns. The bundled sensor helper needs a moment
/// to produce its first frame after launch, so wait briefly instead of showing
/// an empty list (runs on the blocking pool, never the UI thread).
fn editor_readings() -> Vec<crate::lhm::Reading> {
    for _ in 0..14 {
        let r = hwinfo_readings_cached();
        if !r.is_empty() || !crate::lhm::helper_available() {
            return r;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Vec::new()
}

/// List temperature sensor labels for the editor dropdown. Prefers
/// LibreHardwareMonitor (CPU Core Max, GPU Core/Hot Spot/Memory, mobo, RAM,
/// etc.); falls back to the OS sensors sysinfo exposes when LHM isn't running.
/// Uses the shared 750ms LHM cache so repeated opens / the two editor fetches
/// don't each pay the HTTP round-trip.
pub fn temperature_component_labels() -> Vec<String> {
    let readings = editor_readings();
    let mut labels: Vec<String> = readings
        .iter()
        .filter(|r| crate::lhm::is_celsius(&r.unit))
        .map(|r| r.label.clone())
        .collect();
    labels.sort();
    labels.dedup();
    if crate::lhm::cpu_core_max(&readings).is_some() {
        labels.push("CPU Core Max".to_string());
    }
    if !labels.is_empty() {
        return labels;
    }
    // LHM unavailable → fall back to whatever the OS exposes.
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut labels: Vec<String> = components
        .iter()
        .map(|c| c.label().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    labels.sort();
    labels.dedup();
    labels
}

/// List all available LibreHardwareMonitor sensors for the editor's
/// "Hardware sensor" dropdown. Uses the shared LHM cache.
pub fn hardware_sensor_list() -> Vec<crate::lhm::SensorInfo> {
    let mut infos: Vec<crate::lhm::SensorInfo> = editor_readings()
        .into_iter()
        .map(|r| crate::lhm::SensorInfo {
            label: r.label,
            unit: r.unit,
        })
        .collect();
    infos.sort_by(|a, b| a.label.cmp(&b.label));
    infos.dedup_by(|a, b| a.label == b.label);
    infos
}

/// Resolve a temperature from the LHM sample. `needle` matches an exact label
/// first, then a case-insensitive substring; empty = first temperature. The
/// special label "CPU Core Max" returns the hottest CPU-core reading.
fn temperature_from_hwinfo(readings: &[crate::lhm::Reading], needle: &str) -> Option<f64> {
    let n = needle.trim();
    if n.eq_ignore_ascii_case("CPU Core Max") {
        return crate::lhm::cpu_core_max(readings);
    }
    let temps: Vec<&crate::lhm::Reading> = readings
        .iter()
        .filter(|r| crate::lhm::is_celsius(&r.unit))
        .collect();
    if temps.is_empty() {
        return None;
    }
    if n.is_empty() {
        return temps.first().map(|r| r.value);
    }
    if let Some(r) = temps.iter().find(|r| r.label.eq_ignore_ascii_case(n)) {
        return Some(r.value);
    }
    let nl = n.to_lowercase();
    temps
        .iter()
        .find(|r| r.label.to_lowercase().contains(&nl))
        .map(|r| r.value)
}

/// Find a component temperature whose label contains `needle` (case-insensitive);
/// empty needle = first component. Returns °C.
fn temperature_for(components: &sysinfo::Components, needle: &str) -> Option<f64> {
    let needle = needle.to_lowercase();
    for c in components {
        let label = c.label().to_lowercase();
        if needle.is_empty() || label.contains(&needle) {
            // sysinfo 0.32: temperature() returns f32 Option in some versions.
            let t = c.temperature();
            if t.is_finite() && t > 0.0 {
                return Some(t as f64);
            }
        }
    }
    None
}

/// Resolve a disk figure. `mount` can be:
///   * a UNC path like `\\server\share` (works for network drives even when the
///     drive-letter mapping isn't visible to this session/elevation),
///   * a drive letter like `C:` / `Z:\`,
///   * a mount point substring (Linux), or empty = first local disk.
/// Returns GB (free/used/total) or a 0–100 percentage.
fn disk_value(disks: &sysinfo::Disks, mount: &str, metric: DiskMetric) -> Option<f64> {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let m = mount.trim();

    // Direct path query first (handles UNC network shares and any drive path on
    // Windows, bypassing the disk-list/session limitations).
    if !m.is_empty() {
        if let Some((total, avail)) = query_path_space(m) {
            let total = total as f64;
            let avail = avail as f64;
            let used = (total - avail).max(0.0);
            return Some(match metric {
                DiskMetric::Free => avail / GB,
                DiskMetric::Used => used / GB,
                DiskMetric::Total => total / GB,
                DiskMetric::Percent => {
                    if total > 0.0 {
                        used / total * 100.0
                    } else {
                        0.0
                    }
                }
            });
        }
    }

    // Fallback: match against the enumerated local disk list by substring.
    let needle = m.to_lowercase();
    for d in disks {
        let mp = d.mount_point().to_string_lossy().to_lowercase();
        let name = d.name().to_string_lossy().to_lowercase();
        if needle.is_empty() || mp.contains(&needle) || name.contains(&needle) {
            let total = d.total_space() as f64;
            let avail = d.available_space() as f64;
            let used = (total - avail).max(0.0);
            return Some(match metric {
                DiskMetric::Free => avail / GB,
                DiskMetric::Used => used / GB,
                DiskMetric::Total => total / GB,
                DiskMetric::Percent => {
                    if total > 0.0 {
                        used / total * 100.0
                    } else {
                        0.0
                    }
                }
            });
        }
    }
    None
}

/// Query free/total bytes for a path directly from the OS. Works for UNC paths
/// and drive letters. Returns (total_bytes, available_bytes).
#[cfg(target_os = "windows")]
fn query_path_space(path: &str) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    // GetDiskFreeSpaceExW needs a directory path; ensure a trailing backslash
    // for drive letters (e.g. "Z:" -> "Z:\"). UNC roots work as-is.
    let mut p = path.to_string();
    if p.len() == 2 && p.ends_with(':') {
        p.push('\\');
    }
    let wide: Vec<u16> = std::ffi::OsStr::new(&p)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }

    let mut avail: u64 = 0;
    let mut total: u64 = 0;
    let mut free: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free)
    };
    if ok != 0 && total > 0 {
        Some((total, avail))
    } else {
        None
    }
}

/// Non-Windows: no direct path query; rely on the sysinfo disk list.
#[cfg(not(target_os = "windows"))]
fn query_path_space(_path: &str) -> Option<(u64, u64)> {
    None
}


/// Given a value and ordered threshold stops, return (severity, color).
///  * severity ∈ [0,1] = normalized position across the stop value range,
///    used to pick the highest-severity metric for the border.
///  * color = gradient-interpolated hex between the two surrounding stops.
/// With a single stop: severity is 1.0 if value ≥ stop else 0.0; color = stop.
fn severity_and_color(value: f64, thresholds: &[ThresholdStop]) -> (f64, Option<String>) {
    if thresholds.is_empty() {
        return (0.0, None);
    }
    // Sort a copy by value ascending.
    let mut stops: Vec<&ThresholdStop> = thresholds.iter().collect();
    stops.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));

    let first = stops[0];
    let last = stops[stops.len() - 1];

    if stops.len() == 1 {
        let sev = if value >= first.value { 1.0 } else { 0.0 };
        return (sev, Some(first.color.clone()));
    }

    // Below the first stop → severity 0, colour = first.
    if value <= first.value {
        return (0.0, Some(first.color.clone()));
    }
    // At/above the last stop → severity 1, colour = last.
    if value >= last.value {
        return (1.0, Some(last.color.clone()));
    }

    // Find the bracketing pair and interpolate.
    let mut lo = stops[0];
    let mut hi = stops[0];
    for w in stops.windows(2) {
        if value >= w[0].value && value <= w[1].value {
            lo = w[0];
            hi = w[1];
            break;
        }
    }
    let span = (hi.value - lo.value).max(1e-9);
    let t = ((value - lo.value) / span).clamp(0.0, 1.0);
    let color = lerp_hex(&lo.color, &hi.color, t);
    // Severity normalized across the whole stop range.
    let total_span = (last.value - first.value).max(1e-9);
    let sev = ((value - first.value) / total_span).clamp(0.0, 1.0);
    (sev, Some(color))
}

/// Linearly interpolate two "#rrggbb" colours. Falls back to `a` on parse error.
fn lerp_hex(a: &str, b: &str, t: f64) -> String {
    match (parse_hex(a), parse_hex(b)) {
        (Some((ar, ag, ab)), Some((br, bg, bb))) => {
            let r = (ar as f64 + (br as f64 - ar as f64) * t).round() as u8;
            let g = (ag as f64 + (bg as f64 - ag as f64) * t).round() as u8;
            let bl = (ab as f64 + (bb as f64 - ab as f64) * t).round() as u8;
            format!("#{r:02x}{g:02x}{bl:02x}")
        }
        _ => a.to_string(),
    }
}

fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

// --- helpers ----------------------------------------------------------------

fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timeout".to_string()
    } else if e.is_connect() {
        "connection refused".to_string()
    } else if e.is_redirect() {
        "too many redirects".to_string()
    } else {
        // Keep more of the message so proxy/redirect/TLS issues are diagnosable.
        e.to_string().chars().take(120).collect()
    }
}

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = haystack.find(start)? + start.len();
    let e = haystack[s..].find(end)? + s;
    Some(&haystack[s..e])
}

/// Collect every `<yt:videoId>` value from a YouTube RSS feed, in document
/// order (newest first, as YouTube returns them).
#[allow(dead_code)]
fn all_video_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = xml;
    while let Some(id) = extract_between(rest, "<yt:videoId>", "</yt:videoId>") {
        ids.push(id.to_string());
        // Advance past this match to find the next.
        if let Some(pos) = rest.find("</yt:videoId>") {
            rest = &rest[pos + "</yt:videoId>".len()..];
        } else {
            break;
        }
    }
    ids
}

// --- Slideshow ---------------------------------------------------------------

async fn slideshow_check(tile_id: &str, folder: &str) -> TileStatus {
    let folder = folder.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&folder);
        if !path.is_dir() {
            return Err(format!("not a folder: {folder}"));
        }
        let count = std::fs::read_dir(path)
            .map_err(|e| format!("{e}"))?
            .filter_map(|e| e.ok())
            .filter(|e| is_media_file(&e.path()))
            .count();
        Ok(count)
    })
    .await;
    match result {
        Ok(Ok(0)) => TileStatus::new(tile_id, Health::Warn, "folder empty (no images/videos)"),
        Ok(Ok(n)) => TileStatus::new(tile_id, Health::Up, format!("{n} files")),
        Ok(Err(e)) => TileStatus::new(tile_id, Health::Down, e),
        Err(e) => TileStatus::new(tile_id, Health::Unknown, format!("join: {e}")),
    }
}

/// Check if a file path has a recognized image or video extension.
fn is_media_file(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico"
            | "mp4" | "webm" | "ogg" | "mov" | "mkv" | "avi"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiro_parses_real_usage_output() {
        let raw = "Estimated Usage | resets on 2026-11-01 | KIRO PRO MAX\n\
                   Credits (2834.13 of 5000 covered in plan), 56.7%\n\
                   Your plan is managed by your organization's administrator.\n";
        let (used, limit, plan) = parse_kiro_credits(raw).expect("should parse");
        assert_eq!(used, 2834.13);
        assert_eq!(limit, 5000.0);
        assert_eq!(plan.as_deref(), Some("KIRO PRO MAX"));
        assert!((limit - used - 2165.87).abs() < 0.001);
    }

    #[test]
    fn kiro_parses_codexbar_style_output() {
        let raw = "| KIRO FREE\nMonthly credits:\n(0.00 of 50 covered in plan)\n";
        let (used, limit, plan) = parse_kiro_credits(raw).expect("should parse");
        assert_eq!(used, 0.0);
        assert_eq!(limit, 50.0);
        assert_eq!(plan.as_deref(), Some("KIRO FREE"));
    }

    #[test]
    fn kiro_rejects_unrelated_output() {
        assert!(parse_kiro_credits("no credits here").is_none());
    }

    #[test]
    fn strip_ansi_removes_escapes() {
        let s = "\x1b[1mCredits\x1b[0m (10 of 20 covered in plan)";
        assert_eq!(strip_ansi(s), "Credits (10 of 20 covered in plan)");
    }

    #[test]
    fn selector_class_match() {
        let html = r#"<html><body><span class="foo">x</span><div class="price">$42.99</div></body></html>"#;
        assert_eq!(extract_price(html, ".price"), Some(("$42.99".to_string(), 42.99)));
    }

    #[test]
    fn selector_tag_class_compound() {
        let html = r#"<p class="amount">no</p><span class="amount sale">$1,299.00</span>"#;
        // span.amount should skip the <p> and match the <span>.
        assert_eq!(
            extract_price(html, "span.amount"),
            Some(("$1,299.00".to_string(), 1299.00))
        );
    }

    #[test]
    fn selector_id_match_strips_nested_tags() {
        let html = r#"<div id="product-price"><span>€</span> 42,99</div>"#;
        let (raw, val) = extract_price(html, "#product-price").unwrap();
        assert_eq!(val, 42.99);
        assert!(raw.contains("42,99"));
    }

    #[test]
    fn selector_descendant_uses_last_compound() {
        let html = r#"<div class="card"><span class="price">$10.00</span></div>"#;
        assert_eq!(extract_price(html, ".card .price"), Some(("$10.00".to_string(), 10.00)));
    }

    #[test]
    fn selector_rejects_unsupported_css() {
        // Attribute/pseudo/combinator selectors are unsupported → None.
        assert!(SimpleSelector::parse(".price:nth-child(2)").is_none());
        assert!(SimpleSelector::parse("div > .price").is_none());
        assert!(SimpleSelector::parse("[data-price]").is_none());
    }

    #[test]
    fn parses_simple_price() {
        assert_eq!(parse_price_number("$42.99"), Some(42.99));
    }

    #[test]
    fn parses_thousands_separator() {
        assert_eq!(parse_price_number("$1,299.00"), Some(1299.00));
    }

    #[test]
    fn parses_euro_decimal_comma() {
        assert_eq!(parse_price_number("€ 42,99"), Some(42.99));
    }

    #[test]
    fn parses_euro_thousands_dot_decimal_comma() {
        assert_eq!(parse_price_number("1.299,00"), Some(1299.00));
    }

    #[test]
    fn returns_none_for_no_digits() {
        assert_eq!(parse_price_number("out of stock"), None);
    }

    #[test]
    fn extracts_price_from_html() {
        let html = r#"<html><body><span class="price">$19.95</span></body></html>"#;
        let got = extract_price(html, ".price");
        assert_eq!(got, Some(("$19.95".to_string(), 19.95)));
    }

    #[test]
    fn extract_between_works() {
        assert_eq!(extract_between("<title>Hi</title>", "<title>", "</title>"), Some("Hi"));
    }
}
