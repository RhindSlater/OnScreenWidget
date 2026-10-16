import { useEffect, useState } from "react";
import type { ActionSpec, BtDeviceInfo, BtDeviceSpec, HardwareSensorInfo, MonitorKind, Tile, TileSize } from "../types";
import { api } from "../api";
import { ActionEditor } from "./ActionEditor";
import { PathInput } from "./PathInput";

interface Props {
  initial: Tile;
  existingIds: string[]; // to prevent duplicate ids on create
  isNew: boolean;
  onSave: (tile: Tile) => void;
  onCancel: () => void;
}

type KindType = MonitorKind["type"];

const KIND_OPTIONS: { value: KindType; label: string }[] = [
  { value: "minecraft", label: "Minecraft server" },
  { value: "tcp_ping", label: "TCP / NAS reachability" },
  { value: "jellyfin", label: "Jellyfin server" },
  { value: "http_status", label: "Website / HTTP status" },
  { value: "price", label: "Price tracker" },
  { value: "youtube", label: "YouTube channel" },
  { value: "twitch", label: "Twitch channel" },
  { value: "email", label: "Email (IMAP) unread" },
  { value: "ai_credits", label: "AI credits" },
  { value: "discord", label: "Discord voice presence" },
  { value: "api", label: "API (custom endpoint)" },
  { value: "metrics", label: "System metrics" },
  { value: "graph", label: "Graph (metrics over time)" },
  { value: "bluetooth", label: "Bluetooth battery" },
  { value: "media", label: "Media controls" },
  { value: "kiro", label: "Kiro usage (credits)" },
  { value: "slideshow", label: "Slideshow (images/videos)" },
  { value: "counter", label: "Fitness counter (+5 per click)" },
  { value: "clock", label: "Date & time (opens calendar)" },
  { value: "water", label: "Water intake (cups per 24h)" },
];

/** Default kind payload when switching kind type. */
function defaultKind(type: KindType): MonitorKind {
  switch (type) {
    case "minecraft":
      return { type, host: "", port: 25565 };
    case "tcp_ping":
      return { type, host: "", port: 445 };
    case "jellyfin":
      return { type, base_url: "" };
    case "http_status":
      return { type, url: "", expected_status: 200 };
    case "price":
      return { type, url: "", css_selector: "", currency: "$" };
    case "youtube":
      return { type, sources: [{ kind: "channel", id: "" }] };
    case "twitch":
      return { type, channel_login: "", client_id: "", token_secret_ref: "" };
    case "email":
      return {
        type,
        imap_host: "",
        imap_port: 993,
        username: "",
        password_secret_ref: "",
      };
    case "ai_credits":
      return { type, provider: "openai", api_key_secret_ref: "" };
    case "discord":
      return { type, guild_id: "", channels: [], default_channel_id: "" };
    case "api":
      return {
        type,
        url: "",
        method: "GET",
        auth_header: "",
        auth_token_secret_ref: "",
        json_path: "",
        prefix: "",
        suffix: "",
      };
    case "metrics":
      return {
        type,
        metrics: [
          {
            source: { type: "app_cpu", process: "" },
            label: "CPU",
            unit: "%",
            thresholds: [
              { value: 25, color: "#4ade80" },
              { value: 60, color: "#fbbf24" },
              { value: 90, color: "#f87171" },
            ],
          },
          {
            source: { type: "app_memory", process: "" },
            label: "RAM",
            unit: "MB",
            thresholds: [
              { value: 100, color: "#4ade80" },
              { value: 300, color: "#fbbf24" },
              { value: 600, color: "#f87171" },
            ],
          },
        ],
      };
    case "graph":
      return {
        type,
        metrics: [
          {
            source: { type: "network_throughput", interface: "", direction: "in" },
            label: "In",
            unit: "MB/s",
            thresholds: [],
            line_color: "#4ade80",
          },
          {
            source: { type: "network_throughput", interface: "", direction: "out" },
            label: "Out",
            unit: "MB/s",
            thresholds: [],
            line_color: "#f472b6",
          },
        ],
      };
    case "bluetooth":
      return {
        type,
        devices: [
          { name_match: "", icon: null, label: null },
          { name_match: "", icon: null, label: null },
        ],
        low_threshold: 20,
      };
    case "media":
      return { type };
    case "kiro":
      return { type, low_threshold: 500 };
    case "slideshow":
      return { type, folder: "", interval_secs: 30 };
    case "counter":
      return { type, step: 5 };
    case "clock":
      return { type };
    case "water":
      return { type, goal_cups: 12 };
  }
}

export function TileEditor({ initial, existingIds, isNew, onSave, onCancel }: Props) {
  const [tile, setTile] = useState<Tile>(initial);
  const [error, setError] = useState<string | null>(null);
  // Available temperature sensors for the dropdown (fetched once).
  const [tempComponents, setTempComponents] = useState<string[]>([]);
  // HWiNFO troubleshooting output, shown on demand.
  const [hwDiag, setHwDiag] = useState<string | null>(null);
  // Clipboard-copied confirmation for the diagnose output.
  const [hwCopied, setHwCopied] = useState(false);
  // All available HWiNFO hardware sensors (every type) for the dropdown.
  const [hwSensors, setHwSensors] = useState<HardwareSensorInfo[]>([]);
  // Detected Bluetooth devices (for the bluetooth tile dropdown).
  const [btDevices, setBtDevices] = useState<BtDeviceInfo[]>([]);

  const refreshTempComponents = () => {
    api
      .listTemperatureComponents()
      .then(setTempComponents)
      .catch(() => setTempComponents([]));
    api
      .listHardwareSensors()
      .then(setHwSensors)
      .catch(() => setHwSensors([]));
  };

  // Copy text to the clipboard, with a fallback for webview contexts where the
  // async Clipboard API is unavailable or blocked.
  const copyToClipboard = async (text: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.focus();
        ta.select();
        document.execCommand("copy");
        document.body.removeChild(ta);
      }
      setHwCopied(true);
      window.setTimeout(() => setHwCopied(false), 2000);
    } catch {
      setHwCopied(false);
    }
  };

  // Hardened diagnose: always shows feedback, even if the backend command is
  // missing (stale build) or the invoke rejects. Also re-fetches both lists.
  // The full output is copied to the clipboard automatically.
  const runDiagnose = () => {
    setHwCopied(false);
    setHwDiag("Running LibreHardwareMonitor diagnostics…");
    void (async () => {
      const lines: string[] = [];
      try {
        const temps = await api.listTemperatureComponents();
        lines.push(`list_temperature_components → ${temps.length} label(s)`);
      } catch (e) {
        lines.push(`list_temperature_components ERROR: ${String(e)}`);
      }
      try {
        const sensors = await api.listHardwareSensors();
        setHwSensors(sensors);
        lines.push(`list_hardware_sensors → ${sensors.length} sensor(s)`);
      } catch (e) {
        lines.push(`list_hardware_sensors ERROR: ${String(e)}`);
      }
      try {
        const diag = await api.hwinfoDiagnostics();
        lines.push("", "--- hwinfo_diagnostics ---", diag);
      } catch (e) {
        lines.push(
          "",
          `hwinfo_diagnostics ERROR: ${String(e)}`,
          "(If this says the command is not found, the running build is stale —",
          " rebuild the backend with build-release.bat / tauri:dev.)",
        );
      }
      const output = lines.join("\n");
      setHwDiag(output);
      await copyToClipboard(output);
    })();
  };

  useEffect(() => {
    // Only metrics/graph tiles show the temperature/hardware-sensor dropdowns,
    // and those fetches hit LibreHardwareMonitor over HTTP (which can block for
    // ~1.5s each if LHM isn't running). Skip them entirely for other tile kinds
    // so opening a normal tile editor is instant.
    if (tile.kind.type === "metrics" || tile.kind.type === "graph") {
      refreshTempComponents();
    }
    if (tile.kind.type === "bluetooth") {
      refreshBtDevices();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tile.kind.type]);

  const refreshBtDevices = () => {
    api
      .listBluetoothDevices()
      .then(setBtDevices)
      .catch(() => setBtDevices([]));
  };

  const update = (patch: Partial<Tile>) => setTile((t) => ({ ...t, ...patch }));
  const updateKind = (patch: Record<string, unknown>) =>
    setTile((t) => ({ ...t, kind: { ...t.kind, ...patch } as MonitorKind }));
  const setAction = (slot: "single" | "double" | "triple", a: ActionSpec | null) =>
    setTile((t) => ({ ...t, actions: { ...t.actions, [slot]: a } }));

  const save = () => {
    if (!tile.id.trim()) return setError("ID is required.");
    if (!tile.label.trim()) return setError("Label is required.");
    if (isNew && existingIds.includes(tile.id))
      return setError("That ID is already in use.");
    setError(null);
    onSave(tile);
  };

  const k = tile.kind;

  return (
    <div className="editor">
      <div className="editor__head">
        <h3>{isNew ? "Add tile" : "Edit tile"}</h3>
      </div>

      {/* Identity */}
      <div className="field-row">
        <div className="field field--xs">
          <label>ID</label>
          <input
            value={tile.id}
            disabled={!isNew}
            placeholder="unique-id"
            onChange={(e) => update({ id: e.target.value })}
          />
        </div>
        <div className="field">
          <label>Label</label>
          <input
            value={tile.label}
            placeholder="Display name"
            onChange={(e) => update({ label: e.target.value })}
          />
        </div>
        <div className="field field--sm">
          <label>Icon (emoji or image)</label>
          <PathInput
            value={tile.icon ?? ""}
            placeholder="🟩 or image path"
            title="Select an icon image"
            filters={[
              { name: "Images", extensions: ["png", "svg", "jpg", "jpeg", "gif", "webp", "ico"] },
              { name: "All files", extensions: ["*"] },
            ]}
            onChange={(p) => update({ icon: p || null })}
          />
        </div>
        <div className="field field--sm">
          <label>Tile size</label>
          <select
            value={tile.size ?? "1x1"}
            onChange={(e) => update({ size: e.target.value as TileSize })}
          >
            <option value="1x1">1×1 (default)</option>
            <option value="2x1">2×1 (wide)</option>
            <option value="1x2">1×2 (tall)</option>
            <option value="2x2">2×2 (large)</option>
          </select>
        </div>
      </div>

      {/* Kind */}
      <div className="field">
        <label>Monitor type</label>
        <select
          value={k.type}
          onChange={(e) => update({ kind: defaultKind(e.target.value as KindType) })}
        >
          {KIND_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </div>

      {/* Kind-specific fields */}
      {(k.type === "minecraft" || k.type === "tcp_ping") && (
        <div className="field-row">
          <div className="field">
            <label>Host</label>
            <input
              value={k.host}
              placeholder="host or IP"
              onChange={(e) => updateKind({ host: e.target.value })}
            />
          </div>
          <div className="field field--xs">
            <label>Port</label>
            <input
              type="number"
              value={k.port}
              onChange={(e) => updateKind({ port: Number(e.target.value) || 0 })}
            />
          </div>
        </div>
      )}

      {k.type === "jellyfin" && (
        <div className="field">
          <label>Base URL</label>
          <input
            value={k.base_url}
            placeholder="http://192.168.1.10:8096"
            onChange={(e) => updateKind({ base_url: e.target.value })}
          />
        </div>
      )}

      {k.type === "http_status" && (
        <div className="field-row">
          <div className="field">
            <label>URL</label>
            <input
              value={k.url}
              placeholder="https://example.com"
              onChange={(e) => updateKind({ url: e.target.value })}
            />
          </div>
          <div className="field field--xs">
            <label>Expected</label>
            <input
              type="number"
              value={k.expected_status}
              onChange={(e) =>
                updateKind({ expected_status: Number(e.target.value) || 200 })
              }
            />
          </div>
        </div>
      )}

      {k.type === "price" && (
        <>
          <div className="field">
            <label>Product URL</label>
            <input
              value={k.url}
              placeholder="https://shop.com/item"
              onChange={(e) => updateKind({ url: e.target.value })}
            />
          </div>
          <div className="field-row">
            <div className="field">
              <label>CSS selector for price</label>
              <input
                value={k.css_selector}
                placeholder=".product-price"
                onChange={(e) => updateKind({ css_selector: e.target.value })}
              />
            </div>
            <div className="field field--xs">
              <label>Currency</label>
              <input
                value={k.currency ?? ""}
                placeholder="$"
                onChange={(e) => updateKind({ currency: e.target.value || null })}
              />
            </div>
          </div>
          <p className="hint">
            Tip: in your browser, right-click the price → Inspect → Copy
            selector, then paste it here.
          </p>
        </>
      )}

      {k.type === "youtube" && (
        <>
          <div className="editor__section" style={{ marginTop: 0 }}>
            Sources (channels &amp; playlists)
          </div>
          {k.sources.length === 0 && (
            <p className="hint">Add at least one channel or playlist below.</p>
          )}
          {k.sources.map((src, i) => (
            <div className="field-row" key={i} style={{ alignItems: "flex-end" }}>
              <div className="field field--sm">
                <label>Type</label>
                <select
                  value={src.kind}
                  onChange={(e) => {
                    const sources = k.sources.slice();
                    sources[i] = {
                      ...sources[i],
                      kind: e.target.value as "channel" | "playlist",
                    };
                    updateKind({ sources });
                  }}
                >
                  <option value="channel">Channel</option>
                  <option value="playlist">Playlist</option>
                </select>
              </div>
              <div className="field">
                <label>{src.kind === "channel" ? "Channel ID (UC…)" : "Playlist ID (PL…)"}</label>
                <input
                  value={src.id}
                  placeholder={
                    src.kind === "channel"
                      ? "UC_x5XG1OV2P6uZZ5FSM9Ttw"
                      : "PLxxxxxxxxxxxxxxxx"
                  }
                  onChange={(e) => {
                    const sources = k.sources.slice();
                    sources[i] = { ...sources[i], id: e.target.value };
                    updateKind({ sources });
                  }}
                />
              </div>
              <button
                type="button"
                className="btn btn--xs btn--danger"
                onClick={() =>
                  updateKind({ sources: k.sources.filter((_, j) => j !== i) })
                }
              >
                Remove
              </button>
            </div>
          ))}
          <button
            type="button"
            className="btn btn--xs"
            onClick={() =>
              updateKind({ sources: [...k.sources, { kind: "channel", id: "" }] })
            }
          >
            + Add source
          </button>
          <p className="hint">
            All sources are scanned together. New uploads stack into the badge;
            single-click opens the next new video (and removes it), or the
            YouTube home page when there are none. Channel ID is the{" "}
            <code>UC…</code> value; playlist ID is the <code>list=</code> value
            in a playlist URL.
          </p>
        </>
      )}

      {k.type === "twitch" && (
        <>
          <div className="field-row">
            <div className="field">
              <label>Channel login</label>
              <input
                value={k.channel_login}
                placeholder="streamername"
                onChange={(e) => updateKind({ channel_login: e.target.value })}
              />
            </div>
            <div className="field">
              <label>Client ID</label>
              <input
                value={k.client_id}
                placeholder="twitch app client id"
                onChange={(e) => updateKind({ client_id: e.target.value })}
              />
            </div>
          </div>
          <div className="field">
            <label>Token secret key</label>
            <input
              value={k.token_secret_ref}
              placeholder="e.g. twitch_token (store it in Secrets below)"
              onChange={(e) => updateKind({ token_secret_ref: e.target.value })}
            />
          </div>
        </>
      )}

      {k.type === "email" && (
        <>
          <div className="field-row">
            <div className="field">
              <label>IMAP host</label>
              <input
                value={k.imap_host}
                placeholder="imap.gmail.com"
                onChange={(e) => updateKind({ imap_host: e.target.value })}
              />
            </div>
            <div className="field field--xs">
              <label>Port</label>
              <input
                type="number"
                value={k.imap_port}
                onChange={(e) =>
                  updateKind({ imap_port: Number(e.target.value) || 993 })
                }
              />
            </div>
          </div>
          <div className="field">
            <label>Username</label>
            <input
              value={k.username}
              placeholder="you@example.com"
              onChange={(e) => updateKind({ username: e.target.value })}
            />
          </div>
          <div className="field">
            <label>Password secret key</label>
            <input
              value={k.password_secret_ref}
              placeholder="e.g. email_pw (store it in Secrets below)"
              onChange={(e) => updateKind({ password_secret_ref: e.target.value })}
            />
          </div>
        </>
      )}

      {k.type === "ai_credits" && (
        <>
          <div className="field">
            <label>Provider</label>
            <input
              value={k.provider}
              placeholder="openai / anthropic / …"
              onChange={(e) => updateKind({ provider: e.target.value })}
            />
          </div>
          <div className="field">
            <label>API key secret</label>
            <input
              value={k.api_key_secret_ref}
              placeholder="e.g. openai_key (store it in Secrets below)"
              onChange={(e) => updateKind({ api_key_secret_ref: e.target.value })}
            />
          </div>
        </>
      )}

      {k.type === "discord" && (
        <>
          <div className="field">
            <label>Server (guild) ID</label>
            <input
              value={k.guild_id}
              placeholder="right-click server → Copy Server ID"
              onChange={(e) => updateKind({ guild_id: e.target.value })}
            />
          </div>

          <div className="editor__section" style={{ marginTop: 10 }}>
            Voice channels to watch
          </div>
          {k.channels.length === 0 && (
            <p className="hint">
              Leave empty to watch <strong>all</strong> voice channels (names are
              resolved automatically). Add specific channels only if you want to
              limit which ones count.
            </p>
          )}
          {k.channels.map((ch, i) => (
            <div className="field-row" key={i} style={{ alignItems: "flex-end" }}>
              <div className="field field--sm">
                <label>Label</label>
                <input
                  value={ch.label}
                  placeholder="General"
                  onChange={(e) => {
                    const channels = k.channels.slice();
                    channels[i] = { ...channels[i], label: e.target.value };
                    updateKind({ channels });
                  }}
                />
              </div>
              <div className="field">
                <label>Channel ID</label>
                <input
                  value={ch.channel_id}
                  placeholder="right-click channel → Copy Channel ID"
                  onChange={(e) => {
                    const channels = k.channels.slice();
                    channels[i] = { ...channels[i], channel_id: e.target.value };
                    updateKind({ channels });
                  }}
                />
              </div>
              <button
                type="button"
                className="btn btn--xs btn--danger"
                onClick={() => {
                  const channels = k.channels.filter((_, j) => j !== i);
                  // If we removed the default, pick a new one.
                  const stillHasDefault = channels.some(
                    (c) => c.channel_id === k.default_channel_id
                  );
                  updateKind({
                    channels,
                    default_channel_id: stillHasDefault
                      ? k.default_channel_id
                      : channels[0]?.channel_id ?? "",
                  });
                }}
              >
                Remove
              </button>
            </div>
          ))}
          <button
            type="button"
            className="btn btn--xs"
            onClick={() => {
              const channels = [...k.channels, { label: "", channel_id: "" }];
              updateKind({ channels });
            }}
          >
            + Add channel
          </button>

          <div className="field" style={{ marginTop: 10 }}>
            <label>Default channel (opens on single click)</label>
            <select
              value={k.default_channel_id}
              onChange={(e) => updateKind({ default_channel_id: e.target.value })}
            >
              <option value="">— none —</option>
              {k.channels
                .filter((c) => c.channel_id)
                .map((c) => (
                  <option key={c.channel_id} value={c.channel_id}>
                    {c.label || c.channel_id}
                  </option>
                ))}
            </select>
          </div>
          <p className="hint">
            Requires the server Widget enabled (Server Settings → Widget).
            Single-click opens Discord at the default channel; set it under
            Click actions as “Open URL” with{" "}
            <code>discord://-/channels/{k.guild_id || "GUILD"}/CHANNEL</code> —
            or use the button below to auto-fill it.
          </p>
          <button
            type="button"
            className="btn btn--xs"
            disabled={!k.guild_id || !k.default_channel_id}
            onClick={() =>
              setTile((t) => ({
                ...t,
                actions: {
                  ...t.actions,
                  single: {
                    type: "open_url",
                    url: `discord://-/channels/${k.guild_id}/${k.default_channel_id}`,
                  },
                },
              }))
            }
          >
            Set single-click → open default channel
          </button>
        </>
      )}

      {k.type === "api" && (
        <>
          <div className="field-row">
            <div className="field field--xs">
              <label>Method</label>
              <select
                value={k.method}
                onChange={(e) => updateKind({ method: e.target.value })}
              >
                <option value="GET">GET</option>
                <option value="POST">POST</option>
                <option value="PUT">PUT</option>
              </select>
            </div>
            <div className="field">
              <label>URL</label>
              <input
                value={k.url}
                placeholder="https://api.example.com/status"
                onChange={(e) => updateKind({ url: e.target.value })}
              />
            </div>
          </div>
          <div className="field">
            <label>JSON path (optional)</label>
            <input
              value={k.json_path ?? ""}
              placeholder="data.items.0.value  (empty = whole response)"
              onChange={(e) => updateKind({ json_path: e.target.value || null })}
            />
          </div>
          <div className="field-row">
            <div className="field field--sm">
              <label>Prefix</label>
              <input
                value={k.prefix ?? ""}
                placeholder="e.g. CPU "
                onChange={(e) => updateKind({ prefix: e.target.value || null })}
              />
            </div>
            <div className="field field--sm">
              <label>Suffix</label>
              <input
                value={k.suffix ?? ""}
                placeholder="e.g. %"
                onChange={(e) => updateKind({ suffix: e.target.value || null })}
              />
            </div>
          </div>
          <div className="field-row">
            <div className="field field--sm">
              <label>Auth header</label>
              <input
                value={k.auth_header ?? ""}
                placeholder="Authorization"
                onChange={(e) => updateKind({ auth_header: e.target.value || null })}
              />
            </div>
            <div className="field">
              <label>Auth token secret key</label>
              <input
                value={k.auth_token_secret_ref ?? ""}
                placeholder="e.g. my_api_token (store in Secrets)"
                onChange={(e) =>
                  updateKind({ auth_token_secret_ref: e.target.value || null })
                }
              />
            </div>
          </div>
          <p className="hint">
            The tile shows the value at the JSON path (e.g.{" "}
            <code>data.temp</code> or <code>items.0.name</code>). Leave the path
            empty to show the raw response. No fixed response layout is required.
          </p>
        </>
      )}

      {k.type === "media" && (
        <div className="field">
          <p className="hint">
            Shows whatever is playing (YouTube, Tidal, Spotify…) with prev /
            play-pause / next controls. The tile glows purple while playing.
            No configuration needed.
          </p>
        </div>
      )}

      {k.type === "kiro" && (
        <div className="field field--sm">
          <label>Low-credit warning (≤)</label>
          <input
            type="number"
            min={0}
            value={k.low_threshold}
            onChange={(e) =>
              updateKind({ low_threshold: Number(e.target.value) || 0 })
            }
          />
          <p className="hint">
            Shows your AWS Kiro plan <strong>credits remaining</strong>, read via
            the Kiro CLI (no API key — uses your Builder ID login). On Windows it
            runs the CLI inside <strong>WSL</strong> (<code>wsl.exe</code>), so
            the Kiro CLI must be installed and logged in there. The tile
            highlights when remaining credits drop to this value or below. Polls
            every ~30 min.
          </p>
        </div>
      )}

      {k.type === "water" && (
        <div className="field field--xs">
          <label>Goal (cups per 24h)</label>
          <input
            type="number"
            min={1}
            max={64}
            value={k.goal_cups}
            onChange={(e) =>
              updateKind({ goal_cups: Math.min(64, Math.max(1, Number(e.target.value) || 12)) })
            }
          />
          <p className="hint">
            Each cup counts for 24 hours from when you add it. Click the tile to add a cup;
            right-click removes the last one. Tile colour changes every quarter of the goal.
          </p>
        </div>
      )}

      {k.type === "counter" && (
        <div className="field field--xs">
          <label>Add per click</label>
          <input
            type="number"
            min={1}
            value={k.step}
            onChange={(e) => updateKind({ step: Math.max(1, Number(e.target.value) || 5) })}
          />
          <p className="hint">Right-click the tile to undo one step.</p>
        </div>
      )}

      {k.type === "slideshow" && (
        <>
          <div className="field">
            <label>Folder (absolute path to images/videos)</label>
            <PathInput
              value={k.folder}
              placeholder="C:\Users\you\Pictures\Slideshow"
              title="Select slideshow folder"
              directory
              onChange={(p) => updateKind({ folder: p })}
            />
          </div>
          <div className="field field--sm">
            <label>Interval (seconds)</label>
            <input
              type="number"
              min={1}
              value={k.interval_secs}
              onChange={(e) =>
                updateKind({ interval_secs: Math.max(1, Number(e.target.value) || 30) })
              }
            />
          </div>
          <p className="hint">
            Cycles through all images and videos (png, jpg, gif, svg, webp, mp4,
            webm, mov…) in the folder. The tile displays each file full-bleed for
            the configured interval before advancing to the next. Best at 2×2
            size.
          </p>
        </>
      )}

      {k.type === "bluetooth" && (
        <>
          <div className="editor__section" style={{ marginTop: 0 }}>
            Devices shown on this tile
          </div>
          <p className="hint">
            Tick the devices to show. Only ticked devices appear on the tile.{" "}
            {btDevices.length === 0 &&
              "No devices detected right now — connect one and click Refresh."}{" "}
            <button
              type="button"
              className="btn btn--xs"
              onClick={refreshBtDevices}
            >
              Refresh devices
            </button>
          </p>
          {(() => {
            // Detected devices plus any saved ones that aren't detected now.
            const names = [
              ...btDevices.map((d) => d.name),
              ...k.devices
                .map((d) => d.name_match)
                .filter((n) => n && !btDevices.some((d) => d.name === n)),
            ];
            return names.map((name) => {
              const dev = btDevices.find((d) => d.name === name);
              const idx = k.devices.findIndex((d) => d.name_match === name);
              const spec = idx >= 0 ? k.devices[idx] : null;
              const status = !dev
                ? "saved, not detected"
                : dev.connected
                  ? dev.battery != null
                    ? `${dev.battery}%`
                    : "connected"
                  : "offline";
              const patch = (p: Partial<BtDeviceSpec>) => {
                const devices = k.devices.slice();
                devices[idx] = { ...devices[idx], ...p };
                updateKind({ devices });
              };
              return (
                <div className="field-row" key={name}>
                  <label style={{ display: "flex", gap: 6, alignItems: "center", flex: 1 }}>
                    <input
                      type="checkbox"
                      checked={!!spec}
                      onChange={(e) => {
                        if (e.target.checked) {
                          updateKind({
                            devices: [
                              ...k.devices,
                              { name_match: name, icon: null, label: null },
                            ],
                          });
                        } else {
                          updateKind({ devices: k.devices.filter((_, i) => i !== idx) });
                        }
                      }}
                    />
                    <span>
                      {name} <span className="hint">— {status}</span>
                    </span>
                  </label>
                  {spec && (
                    <>
                      <div className="field field--sm">
                        <input
                          value={spec.label ?? ""}
                          placeholder="label"
                          onChange={(e) => patch({ label: e.target.value || null })}
                        />
                      </div>
                      <div className="field field--xs">
                        <input
                          value={spec.icon ?? ""}
                          placeholder="icon"
                          onChange={(e) => patch({ icon: e.target.value || null })}
                        />
                      </div>
                    </>
                  )}
                </div>
              );
            });
          })()}
          <div className="field field--xs" style={{ marginTop: 8 }}>
            <label>Low battery % (flash)</label>
            <input
              type="number"
              min={1}
              max={99}
              value={k.low_threshold}
              onChange={(e) =>
                updateKind({
                  low_threshold: Math.min(99, Math.max(1, Number(e.target.value) || 20)),
                })
              }
            />
          </div>
        </>
      )}

      {(k.type === "metrics" || k.type === "graph") && (
        <>
          <div className="editor__section" style={{ marginTop: 0 }}>
            {k.type === "graph" ? "Graph lines" : "Metrics (up to 4)"}
          </div>
          {k.metrics.map((m, i) => (
            <div
              key={i}
              style={{
                border: "1px solid rgba(255,255,255,0.08)",
                borderRadius: 8,
                padding: 8,
                marginBottom: 8,
              }}
            >
              <div className="field-row">
                <div className="field field--sm">
                  <label>Source</label>
                  <select
                    value={m.source.type}
                    onChange={(e) => {
                      const t = e.target.value;
                      let source: typeof m.source;
                      if (t === "temperature") {
                        source = { type: "temperature", component: "" };
                      } else if (t === "hardware_sensor") {
                        source = { type: "hardware_sensor", sensor: "" };
                      } else if (t === "disk") {
                        source = { type: "disk", mount: "", metric: "free" };
                      } else if (t === "network_throughput") {
                        source = { type: "network_throughput", interface: "", direction: "in" };
                      } else if (t === "app_cpu" || t === "app_memory") {
                        source = { type: t, process: "" };
                      } else {
                        source = { type: t as "system_cpu" | "system_memory" };
                      }
                      const metrics = k.metrics.slice();
                      metrics[i] = { ...metrics[i], source };
                      updateKind({ metrics });
                    }}
                  >
                    <option value="app_cpu">App CPU %</option>
                    <option value="app_memory">App memory (MB)</option>
                    <option value="system_cpu">System CPU %</option>
                    <option value="system_memory">System memory %</option>
                    <option value="temperature">Temperature °C</option>
                    <option value="hardware_sensor">Hardware sensor (LibreHardwareMonitor)</option>
                    <option value="network_throughput">Network MB/s</option>
                    <option value="disk">Disk space</option>
                  </select>
                </div>
                <div className="field field--sm">
                  <label>Label</label>
                  <input
                    value={m.label}
                    onChange={(e) => {
                      const metrics = k.metrics.slice();
                      metrics[i] = { ...metrics[i], label: e.target.value };
                      updateKind({ metrics });
                    }}
                  />
                </div>
                <div className="field field--xs">
                  <label>Unit</label>
                  <input
                    value={m.unit}
                    onChange={(e) => {
                      const metrics = k.metrics.slice();
                      metrics[i] = { ...metrics[i], unit: e.target.value };
                      updateKind({ metrics });
                    }}
                  />
                </div>
              </div>

              {(m.source.type === "app_cpu" || m.source.type === "app_memory") && (
                <div className="field">
                  <label>Process name (empty = this widget)</label>
                  <input
                    value={m.source.process}
                    placeholder="e.g. chrome, Code, javaw"
                    onChange={(e) => {
                      const metrics = k.metrics.slice();
                      const st = metrics[i].source.type as "app_cpu" | "app_memory";
                      metrics[i] = {
                        ...metrics[i],
                        source: { type: st, process: e.target.value },
                      };
                      updateKind({ metrics });
                    }}
                  />
                </div>
              )}

              {m.source.type === "temperature" && (
                <div className="field">
                  <label>Sensor {tempComponents.length === 0 ? "(none detected — run LibreHardwareMonitor + enable its Remote Web Server)" : ""}</label>
                  <select
                    value={m.source.component}
                    onChange={(e) => {
                      const metrics = k.metrics.slice();
                      metrics[i] = {
                        ...metrics[i],
                        source: { type: "temperature", component: e.target.value },
                      };
                      updateKind({ metrics });
                    }}
                  >
                    <option value="">(first available)</option>
                    {tempComponents.map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                    {/* Keep a stale/custom value selectable even if not detected now. */}
                    {m.source.component &&
                      !tempComponents.includes(m.source.component) && (
                        <option value={m.source.component}>
                          {m.source.component} (not detected)
                        </option>
                      )}
                  </select>
                  <div className="row" style={{ gap: 8, marginTop: 6 }}>
                    <button type="button" onClick={refreshTempComponents}>
                      Refresh sensors
                    </button>
                    <button
                      type="button"
                      onClick={runDiagnose}
                    >
                      Diagnose LibreHardwareMonitor
                    </button>
                  </div>
                  {hwDiag !== null && (
                    <div style={{ marginTop: 6 }}>
                      <div className="row" style={{ gap: 8, alignItems: "center" }}>
                        <button
                          type="button"
                          onClick={() => hwDiag && copyToClipboard(hwDiag)}
                        >
                          Copy
                        </button>
                        {hwCopied && (
                          <span style={{ color: "#4ade80", fontSize: 12 }}>
                            Copied to clipboard ✓
                          </span>
                        )}
                      </div>
                      <pre
                        style={{
                          maxHeight: 220,
                          overflow: "auto",
                          fontSize: 11,
                          whiteSpace: "pre-wrap",
                          background: "rgba(0,0,0,0.25)",
                          padding: 8,
                          borderRadius: 6,
                          marginTop: 6,
                        }}
                      >
                        {hwDiag}
                      </pre>
                    </div>
                  )}
                </div>
              )}

              {m.source.type === "hardware_sensor" && (() => {
                const sensorVal = m.source.sensor;
                return (
                <div className="field">
                  <label>
                    Hardware sensor{" "}
                    {hwSensors.length === 0
                      ? "(none detected — run LibreHardwareMonitor + enable its Remote Web Server)"
                      : `(${hwSensors.length} available)`}
                  </label>
                  <select
                    value={m.source.sensor}
                    onChange={(e) => {
                      const sensor = e.target.value;
                      const chosen = hwSensors.find((h) => h.label === sensor);
                      const metrics = k.metrics.slice();
                      const cur = metrics[i];
                      metrics[i] = {
                        ...cur,
                        source: { type: "hardware_sensor", sensor },
                        // Auto-prefill unit/label from HWiNFO when empty so the
                        // readout shows the right suffix without manual typing.
                        unit: cur.unit && cur.unit.length > 0 ? cur.unit : chosen?.unit ?? "",
                        label:
                          cur.label && cur.label.length > 0
                            ? cur.label
                            : chosen?.label ?? cur.label,
                      };
                      updateKind({ metrics });
                    }}
                  >
                    <option value="">(first available)</option>
                    {Array.from(new Set(hwSensors.map((h) => h.type_name))).map(
                      (group) => (
                        <optgroup key={group} label={group}>
                          {hwSensors
                            .filter((h) => h.type_name === group)
                            .map((h) => (
                              <option key={h.label} value={h.label}>
                                {h.label}
                                {h.unit ? ` — ${h.unit}` : ""}
                              </option>
                            ))}
                        </optgroup>
                      ),
                    )}
                    {m.source.sensor &&
                      !hwSensors.some((h) => h.label === sensorVal) && (
                        <option value={m.source.sensor}>
                          {m.source.sensor} (not detected)
                        </option>
                      )}
                  </select>
                  <div className="row" style={{ gap: 8, marginTop: 6 }}>
                    <button type="button" onClick={refreshTempComponents}>
                      Refresh sensors
                    </button>
                    <button
                      type="button"
                      onClick={runDiagnose}
                    >
                      Diagnose LibreHardwareMonitor
                    </button>
                  </div>
                  {hwDiag !== null && (
                    <div style={{ marginTop: 6 }}>
                      <div className="row" style={{ gap: 8, alignItems: "center" }}>
                        <button
                          type="button"
                          onClick={() => hwDiag && copyToClipboard(hwDiag)}
                        >
                          Copy
                        </button>
                        {hwCopied && (
                          <span style={{ color: "#4ade80", fontSize: 12 }}>
                            Copied to clipboard ✓
                          </span>
                        )}
                      </div>
                      <pre
                        style={{
                          maxHeight: 220,
                          overflow: "auto",
                          fontSize: 11,
                          whiteSpace: "pre-wrap",
                          background: "rgba(0,0,0,0.25)",
                          padding: 8,
                          borderRadius: 6,
                          marginTop: 6,
                        }}
                      >
                        {hwDiag}
                      </pre>
                    </div>
                  )}
                </div>
                );
              })()}

              {m.source.type === "disk" && (
                <div className="field-row">
                  <div className="field">
                    <label>Drive / mount (e.g. C:, D:, \\\\server\\share; empty = first)</label>
                    <input
                      value={m.source.mount}
                      placeholder="C:"
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const cur = metrics[i].source as { type: "disk"; mount: string; metric: "free" | "used" | "total" | "percent" };
                        metrics[i] = {
                          ...metrics[i],
                          source: { type: "disk", mount: e.target.value, metric: cur.metric },
                        };
                        updateKind({ metrics });
                      }}
                    />
                  </div>
                  <div className="field field--sm">
                    <label>Figure</label>
                    <select
                      value={m.source.metric}
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const cur = metrics[i].source as { type: "disk"; mount: string; metric: string };
                        metrics[i] = {
                          ...metrics[i],
                          source: {
                            type: "disk",
                            mount: cur.mount,
                            metric: e.target.value as "free" | "used" | "total" | "percent",
                          },
                        };
                        updateKind({ metrics });
                      }}
                    >
                      <option value="free">Free (GB)</option>
                      <option value="used">Used (GB)</option>
                      <option value="total">Total (GB)</option>
                      <option value="percent">Used %</option>
                    </select>
                  </div>
                </div>
              )}

              {m.source.type === "network_throughput" && (
                <div className="field-row">
                  <div className="field">
                    <label>Interface (empty = all)</label>
                    <input
                      value={m.source.interface}
                      placeholder="e.g. Ethernet, Wi-Fi"
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const cur = metrics[i].source as {
                          type: "network_throughput";
                          interface: string;
                          direction: "in" | "out";
                        };
                        metrics[i] = {
                          ...metrics[i],
                          source: { type: "network_throughput", interface: e.target.value, direction: cur.direction },
                        };
                        updateKind({ metrics });
                      }}
                    />
                  </div>
                  <div className="field field--sm">
                    <label>Direction</label>
                    <select
                      value={m.source.direction}
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const cur = metrics[i].source as {
                          type: "network_throughput";
                          interface: string;
                          direction: string;
                        };
                        metrics[i] = {
                          ...metrics[i],
                          source: {
                            type: "network_throughput",
                            interface: cur.interface,
                            direction: e.target.value as "in" | "out",
                          },
                        };
                        updateKind({ metrics });
                      }}
                    >
                      <option value="in">In (down)</option>
                      <option value="out">Out (up)</option>
                    </select>
                  </div>
                </div>
              )}

              {k.type === "graph" && (
                <div className="field field--sm">
                  <label>Line colour</label>
                  <input
                    type="color"
                    value={m.line_color ?? "#60a5fa"}
                    onChange={(e) => {
                      const metrics = k.metrics.slice();
                      metrics[i] = { ...metrics[i], line_color: e.target.value };
                      updateKind({ metrics });
                    }}
                  />
                </div>
              )}

              <label style={{ fontSize: 10, color: "var(--muted)" }}>
                Thresholds (value → colour)
              </label>
              {m.thresholds.map((th, ti) => (
                <div className="field-row" key={ti} style={{ alignItems: "flex-end" }}>
                  <div className="field field--xs">
                    <label>Op</label>
                    <select
                      value={th.op ?? "gte"}
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const thresholds = metrics[i].thresholds.slice();
                        thresholds[ti] = {
                          ...thresholds[ti],
                          op: e.target.value as "gt" | "lt" | "eq" | "gte" | "lte",
                        };
                        metrics[i] = { ...metrics[i], thresholds };
                        updateKind({ metrics });
                      }}
                    >
                      <option value="gt">&gt;</option>
                      <option value="lt">&lt;</option>
                      <option value="eq">=</option>
                      <option value="gte">≥</option>
                      <option value="lte">≤</option>
                    </select>
                  </div>
                  <div className="field field--xs">
                    <label>Value</label>
                    <input
                      type="number"
                      value={th.value}
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const thresholds = metrics[i].thresholds.slice();
                        thresholds[ti] = { ...thresholds[ti], value: Number(e.target.value) || 0 };
                        metrics[i] = { ...metrics[i], thresholds };
                        updateKind({ metrics });
                      }}
                    />
                  </div>
                  <div className="field field--xs">
                    <label>Colour</label>
                    <input
                      type="color"
                      value={th.color}
                      onChange={(e) => {
                        const metrics = k.metrics.slice();
                        const thresholds = metrics[i].thresholds.slice();
                        thresholds[ti] = { ...thresholds[ti], color: e.target.value };
                        metrics[i] = { ...metrics[i], thresholds };
                        updateKind({ metrics });
                      }}
                    />
                  </div>
                  <button
                    type="button"
                    className="btn btn--xs"
                    disabled={ti === 0}
                    title="Move up"
                    onClick={() => {
                      const metrics = k.metrics.slice();
                      const thresholds = metrics[i].thresholds.slice();
                      [thresholds[ti - 1], thresholds[ti]] = [
                        thresholds[ti],
                        thresholds[ti - 1],
                      ];
                      metrics[i] = { ...metrics[i], thresholds };
                      updateKind({ metrics });
                    }}
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    className="btn btn--xs"
                    disabled={ti === m.thresholds.length - 1}
                    title="Move down"
                    onClick={() => {
                      const metrics = k.metrics.slice();
                      const thresholds = metrics[i].thresholds.slice();
                      [thresholds[ti + 1], thresholds[ti]] = [
                        thresholds[ti],
                        thresholds[ti + 1],
                      ];
                      metrics[i] = { ...metrics[i], thresholds };
                      updateKind({ metrics });
                    }}
                  >
                    ▼
                  </button>
                  <button
                    type="button"
                    className="btn btn--xs btn--danger"
                    onClick={() => {
                      const metrics = k.metrics.slice();
                      metrics[i] = {
                        ...metrics[i],
                        thresholds: metrics[i].thresholds.filter((_, j) => j !== ti),
                      };
                      updateKind({ metrics });
                    }}
                  >
                    ✕
                  </button>
                </div>
              ))}
              <div className="editor__actions" style={{ justifyContent: "space-between" }}>
                <button
                  type="button"
                  className="btn btn--xs"
                  onClick={() => {
                    const metrics = k.metrics.slice();
                    metrics[i] = {
                      ...metrics[i],
                      thresholds: [
                        ...metrics[i].thresholds,
                        { value: 0, color: "#4ade80" },
                      ],
                    };
                    updateKind({ metrics });
                  }}
                >
                  + Threshold
                </button>
                <button
                  type="button"
                  className="btn btn--xs btn--danger"
                  onClick={() =>
                    updateKind({ metrics: k.metrics.filter((_, j) => j !== i) })
                  }
                >
                  Remove metric
                </button>
              </div>
            </div>
          ))}
          {k.metrics.length < 4 && (
            <button
              type="button"
              className="btn btn--xs"
              onClick={() =>
                updateKind({
                  metrics: [
                    ...k.metrics,
                    {
                      source: { type: "system_cpu" },
                      label: "CPU",
                      unit: "%",
                      thresholds: [{ value: 80, color: "#f87171" }],
                    },
                  ],
                })
              }
            >
              + Add metric
            </button>
          )}
          <p className="hint">
            Up to 4 metrics, always shown (no hover). The border colour is a
            gradient from the highest-severity metric across its thresholds.
            When a metric meets its FINAL threshold's operator (e.g. ≥ 90), the
            border flashes that colour. Temperatures may need the app run as admin.
          </p>
        </>
      )}

      {/* Poll + order */}
      <div className="field-row">
        <div className="field field--sm">
          <label>Refresh (secs)</label>
          <input
            type="number"
            value={tile.poll_secs ?? ""}
            placeholder="default"
            onChange={(e) =>
              update({
                poll_secs: e.target.value ? Number(e.target.value) : null,
              })
            }
          />
        </div>
        {(k.type === "metrics" || k.type === "graph") && (
          <div className="field field--sm">
            <label>Fast refresh (ms)</label>
            <input
              type="number"
              min={100}
              value={tile.poll_ms ?? ""}
              placeholder="e.g. 500"
              onChange={(e) =>
                update({
                  poll_ms: e.target.value ? Math.max(100, Number(e.target.value)) : null,
                })
              }
            />
          </div>
        )}
        <div className="field field--xs">
          <label>Order</label>
          <input
            type="number"
            value={tile.order}
            onChange={(e) => update({ order: Number(e.target.value) || 0 })}
          />
        </div>
      </div>

      <div className="field">
        <label>
          <input
            type="checkbox"
            checked={tile.details_always ?? false}
            onChange={(e) => update({ details_always: e.target.checked })}
          />{" "}
          Always show details (not just on hover)
        </label>
      </div>

      {/* Actions */}
      <div className="editor__section">Click actions</div>
      <ActionEditor
        label="Single click"
        value={tile.actions.single}
        onChange={(a) => setAction("single", a)}
      />
      <ActionEditor
        label="Double click"
        value={tile.actions.double}
        onChange={(a) => setAction("double", a)}
      />
      <ActionEditor
        label="Triple click"
        value={tile.actions.triple}
        onChange={(a) => setAction("triple", a)}
      />

      {error && <div className="error">{error}</div>}

      <div className="editor__actions">
        <button className="btn btn--ghost" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn" onClick={save}>
          Save tile
        </button>
      </div>
    </div>
  );
}
