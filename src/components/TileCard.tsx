import type { CSSProperties } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ActionSpec, BorderEffect, Tile, TileStatus } from "../types";
import { useClickCount } from "../useClickCount";

interface Props {
  tile: Tile;
  status?: TileStatus;
  /** Global border-effect style from settings. */
  borderEffect?: BorderEffect;
  /** When true, show the detail text always (not just on hover). */
  showDetails?: boolean;
  /** Fires when a frontend-handled action should run (count encodes 1/2/3). */
  onFrontendAction: (tile: Tile, count: 1 | 2 | 3, action: ActionSpec) => void;
  /** Fires for backend actions (SSH, terminal, url). */
  onBackendAction: (action: ActionSpec) => void;
  /** Pointer-based reordering (optional; enabled in normal grid view). */
  draggable?: boolean;
  isDragging?: boolean;
  isDragOver?: boolean;
  /** Called on pointer-down to begin a potential drag of this tile. */
  onReorderStart?: (tileId: string, e: React.PointerEvent) => void;
}

/** Default emoji icon per monitor kind when no custom icon is set. */
function defaultIcon(tile: Tile): string {
  switch (tile.kind.type) {
    case "minecraft":
      return "🟩";
    case "jellyfin":
      return "🎬";
    case "tcp_ping":
      return "🖥️";
    case "http_status":
      return "🌐";
    case "price":
      return "🏷️";
    case "youtube":
      return "▶️";
    case "twitch":
      return "🎮";
    case "email":
      return "✉️";
    case "ai_credits":
      return "🤖";
    case "discord":
      return "🎧";
    case "api":
      return "🔌";
    case "kiro":
      return "✨";
    case "slideshow":
      return "🖼️";
    default:
      return "📦";
  }
}

export function TileCard({
  tile,
  status,
  borderEffect = "orbit",
  showDetails = false,
  onFrontendAction,
  onBackendAction,
  draggable = false,
  isDragging = false,
  isDragOver = false,
  onReorderStart,
}: Props) {
  const handleClicks = useClickCount((count) => {
    const action =
      count === 1 ? tile.actions.single : count === 2 ? tile.actions.double : tile.actions.triple;
    if (!action) return;
    if (action.type === "frontend") {
      onFrontendAction(tile, count, action);
    } else {
      onBackendAction(action);
    }
  });

  const health = status?.health ?? "unknown";
  const badge = status?.badge ?? null;

  // Highlight (purple): price drop (↓), twitch live, youtube uploads, discord
  // in-call, or a Minecraft server with players online — all when health is up.
  const highlight =
    (status?.detail?.includes("↓") ?? false) ||
    (health === "up" &&
      (tile.kind.type === "twitch" ||
        (tile.kind.type === "youtube" && (badge ?? 0) > 0) ||
        (tile.kind.type === "discord" && (badge ?? 0) > 0) ||
        (tile.kind.type === "minecraft" && (badge ?? 0) > 0)));

  const iconIsPath = !!tile.icon && (tile.icon.includes("/") || tile.icon.includes("\\"));

  const classes = [
    "tile",
    `tile--${health}`,
    borderEffect !== "none" ? `tile--border-${borderEffect}` : "",
    showDetails ? "tile--details-always" : "",
    highlight ? "tile--highlight" : "",
    isDragging ? "tile--dragging" : "",
    isDragOver ? "tile--dragover" : "",
    draggable ? "tile--draggable" : "",
    status?.flash ? "tile--flash" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const isMetrics = tile.kind.type === "metrics";
  const borderColor = status?.border_color ?? null;

  // Colour that orbits the border. Priority: explicit metric border colour →
  // the purple highlight → the health status colour (var set in CSS).
  const healthVar =
    health === "up"
      ? "var(--up)"
      : health === "down"
        ? "var(--down)"
        : health === "warn"
          ? "var(--warn)"
          : "var(--unknown)";
  const borderAccent = borderColor ?? (highlight ? "var(--highlight)" : healthVar);

  // Grid footprint: width×height in cells. Default 1×1.
  const [spanCols, spanRows] =
    tile.size === "2x2"
      ? [2, 2]
      : tile.size === "2x1"
        ? [2, 1]
        : tile.size === "1x2"
          ? [1, 2]
          : [1, 1];

  // For a metrics tile, color the whole border with the gradient color.
  // Expose the color as a CSS var so the flash keyframes can pulse it.
  const inlineStyle: CSSProperties = {
    ["--border-accent" as string]: borderAccent,
    gridColumn: `span ${spanCols}`,
    gridRow: `span ${spanRows}`,
    ...(isMetrics && borderColor
      ? ({
          borderColor,
          borderLeftColor: borderColor,
          ["--flash-color" as string]: borderColor,
        } as CSSProperties)
      : {}),
  } as CSSProperties;

  return (
    <div
      className={classes}
      data-tile-id={tile.id}
      style={inlineStyle}
      onPointerDown={(e) => {
        if (!draggable || e.button !== 0) return;
        onReorderStart?.(tile.id, e);
      }}
      // The parent sets window.__oswDragged after a real drag so the trailing
      // click doesn't trigger a tile action.
      onClick={(e) => {
        if ((window as unknown as { __oswDragged?: boolean }).__oswDragged) {
          return; // this click concluded a drag; ignore it
        }
        handleClicks(e);
      }}
      title={status?.detail ?? "no data yet"}
    >
      {isMetrics ? (
        /* Metrics tile: no icon, metric readouts always visible. */
        <div className="tile__metrics">
          <div className="tile__label">{tile.label}</div>
          {(status?.metrics ?? []).map((m, i) => (
            <div className="tile__metric" key={i}>
              {m.value_text}
            </div>
          ))}
          {(!status || (status.metrics?.length ?? 0) === 0) && (
            <div className="tile__metric tile__metric--dim">…</div>
          )}
        </div>
      ) : (
        <>
          {/* Large background icon layer, behind the text. */}
          <div className="tile__bgicon" aria-hidden="true">
            {tile.icon ? (
              iconIsPath ? (
                <img className="tile__bgicon-img" src={convertFileSrc(tile.icon)} alt="" />
              ) : (
                <span className="tile__bgicon-emoji">{tile.icon}</span>
              )
            ) : (
              <span className="tile__bgicon-emoji">{defaultIcon(tile)}</span>
            )}
          </div>

          {/* Foreground: label pinned to the TOP. Detail shown on hover. */}
          <div className="tile__top">
            <span className="tile__label">{tile.label}</span>
            <span className="tile__meta">
              {badge !== null &&
                (tile.kind.type === "minecraft" ? badge >= 0 : badge > 0) && (
                  <span className="tile__badge">{badge}</span>
                )}
            </span>
          </div>

          {/* Hover-only detail: appears under the title, wraps, fills the body. */}
          <div className="tile__hover">{status?.detail ?? "…"}</div>
        </>
      )}
    </div>
  );
}
