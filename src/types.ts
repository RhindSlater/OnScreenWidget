// TypeScript mirror of the Rust `model.rs` types. Kept in sync manually.
// Tagged unions use the `type` discriminator matching serde's tag.

export type WindowMode = "overlay" | "dock";

export type Health = "up" | "down" | "warn" | "unknown";

export type DockOrientation = "horizontal" | "vertical";
export type DockSide = "top" | "bottom" | "left" | "right";
export type DockAlign = "centered" | "cornered";
export type DockCorner = "start" | "end";

export interface WindowConfig {
  always_on_top: boolean;
  docked: boolean;
  dock_orientation: DockOrientation;
  dock_side: DockSide;
  dock_align: DockAlign;
  dock_corner: DockCorner;
  dock_monitor: number | null;
  default_poll_secs: number;
  opacity: number;
  border_effect: BorderEffect;
}

export type BorderEffect =
  | "static"
  | "orbit"
  | "breathing"
  | "pulse"
  | "rainbow"
  | "none";

export type MonitorKind =
  | { type: "tcp_ping"; host: string; port: number }
  | { type: "http_status"; url: string; expected_status: number }
  | { type: "minecraft"; host: string; port: number }
  | { type: "jellyfin"; base_url: string }
  | { type: "price"; url: string; css_selector: string; currency?: string | null }
  | { type: "youtube"; sources: YouTubeSource[] }
  | {
      type: "twitch";
      channel_login: string;
      client_id: string;
      token_secret_ref: string;
    }
  | {
      type: "email";
      imap_host: string;
      imap_port: number;
      username: string;
      password_secret_ref: string;
    }
  | { type: "ai_credits"; provider: string; api_key_secret_ref: string }
  | {
      type: "discord";
      guild_id: string;
      channels: DiscordChannel[];
      default_channel_id: string;
    }
  | {
      type: "api";
      url: string;
      method: string;
      auth_header?: string | null;
      auth_token_secret_ref?: string | null;
      json_path?: string | null;
      prefix?: string | null;
      suffix?: string | null;
    }
  | { type: "metrics"; metrics: MetricSpec[] }
  | { type: "graph"; metrics: MetricSpec[] }
  | { type: "bluetooth"; devices: BtDeviceSpec[]; low_threshold: number }
  | { type: "media" }
  | { type: "kiro"; low_threshold: number }
  | { type: "slideshow"; folder: string; interval_secs: number }
  | { type: "counter"; step: number }
  | { type: "clock" }
  | { type: "water"; goal_cups: number };

/** Water tile state: cups logged in the last 24 hours. */
export interface WaterState {
  cups: number;
  next_expiry_ms: number | null;
}

/** A configured Bluetooth device slot. */
export interface BtDeviceSpec {
  name_match: string;
  icon?: string | null;
  label?: string | null;
}

/** A detected Bluetooth device (for the editor dropdown). */
export interface BtDeviceInfo {
  name: string;
  battery?: number | null;
  connected: boolean;
}

export type MetricSource =
  | { type: "app_cpu"; process: string }
  | { type: "app_memory"; process: string }
  | { type: "system_cpu" }
  | { type: "system_memory" }
  | { type: "temperature"; component: string }
  | { type: "hardware_sensor"; sensor: string }
  | { type: "lhm_sensor"; sensor: string }
  | { type: "disk"; mount: string; metric: "free" | "used" | "total" | "percent" }
  | { type: "network_throughput"; interface: string; direction: "in" | "out" };

/// An available HWiNFO hardware sensor (from list_hardware_sensors).
export interface HardwareSensorInfo {
  label: string;
  unit: string;
  type_name: string;
}

export type ThresholdOp = "gt" | "lt" | "eq" | "gte" | "lte";

export interface ThresholdStop {
  value: number;
  color: string;
  op?: ThresholdOp;
}

export interface MetricSpec {
  source: MetricSource;
  label: string;
  unit: string;
  thresholds: ThresholdStop[];
  line_color?: string;
}

export interface DiscordChannel {
  label: string;
  channel_id: string;
}

export interface YouTubeSource {
  kind: "channel" | "playlist";
  id: string;
}

export type ActionSpec =
  | { type: "open_url"; url: string }
  | { type: "ssh_terminal"; host: string; user?: string | null; port: number }
  | {
      type: "ssh_command";
      host: string;
      user?: string | null;
      port: number;
      command: string;
    }
  | { type: "spawn_terminal"; program: string; args: string[] }
  | { type: "launch_app"; path: string; args: string[] }
  | {
      type: "http_request";
      method: string;
      url: string;
      auth_header?: string | null;
      auth_token_secret_ref?: string | null;
      body?: string | null;
    }
  | { type: "frontend"; name: string };

export interface ClickActions {
  single?: ActionSpec | null;
  double?: ActionSpec | null;
  triple?: ActionSpec | null;
}

export interface Tile {
  id: string;
  label: string;
  icon?: string | null;
  kind: MonitorKind;
  actions: ClickActions;
  poll_secs?: number | null;
  poll_ms?: number | null;
  order: number;
  enabled: boolean;
  details_always: boolean;
  size: TileSize;
}

/** Grid footprint, width×height (columns×rows). */
export type TileSize = "1x1" | "2x2" | "1x2" | "2x1";

export interface AppConfig {
  version: number;
  window: WindowConfig;
  tiles: Tile[];
}

export interface TileStatus {
  tile_id: string;
  health: Health;
  detail: string;
  badge?: number | null;
  value?: number | null;
  link?: string | null;
  metrics?: MetricReadout[];
  border_color?: string | null;
  flash?: boolean;
  graph_points?: GraphPoint[];
  bt_devices?: BtDeviceReadout[];
  media?: MediaReadout | null;
  checked_at: string;
}

/** Now-playing media info for a media tile. */
export interface MediaReadout {
  title: string;
  artist: string;
  app: string;
  playing: boolean;
  active: boolean;
  art: string;
}

/** One Bluetooth device cell in a bluetooth tile. */
export interface BtDeviceReadout {
  label: string;
  icon: string;
  battery?: number | null;
  charging: boolean;
  low: boolean;
  connected: boolean;
  stale?: boolean;
}

/** Counter tile totals: today (resets daily) and lifetime. */
export interface CounterCounts {
  today: number;
  lifetime: number;
}

export interface GraphPoint {
  label: string;
  color: string;
  value: number;
}

export interface MetricReadout {
  label: string;
  value_text: string;
}

export interface PricePoint {
  at: string;
  value: number;
}
