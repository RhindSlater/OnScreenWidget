import { convertFileSrc } from "@tauri-apps/api/core";
import type { CSSProperties } from "react";
import type { BorderEffect, Tile, TileStatus } from "../types";

interface Props {
  tile: Tile;
  status?: TileStatus;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
  onClick?: (e: React.MouseEvent) => void;
}

/** Is a string an image path (vs an emoji)? */
function isPath(icon: string): boolean {
  return icon.includes("/") || icon.includes("\\");
}

/**
 * Bluetooth battery tile: a nested 2×2 grid of up to 4 device cells. Each cell
 * shows the device icon + battery %, with its own border:
 *   • flashing  when battery is low
 *   • highlight when charging
 *   • static    otherwise
 */
export function BluetoothTile({
  tile,
  status,
  borderEffect = "orbit",
  onPointerDown,
  onClick,
}: Props) {
  const devices = status?.bt_devices ?? [];

  // Show every device; the grid scrolls when there are more than fit.
  const cells = devices;
  const many = cells.length > 4;

  const classes = [
    "tile",
    "tile--bluetooth",
    many ? "tile--bluetooth-many" : "",
    borderEffect !== "none" ? `tile--border-${borderEffect}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={classes}
      data-tile-id={tile.id}
      onPointerDown={onPointerDown}
      onClick={onClick}
      title={status?.detail ?? "no data yet"}
      style={{ ["--border-accent" as string]: "var(--accent)" } as CSSProperties}
    >
      <div className="bt-grid">
        {cells.length === 0 && (
          <div className="bt-cell bt-cell--empty">
            <div className="bt-cell__battery bt-cell__hint">Add devices in settings</div>
          </div>
        )}
        {cells.map((d, i) => {
          const cellState = !d.connected
            ? "bt-cell--offline"
            : d.low
              ? "bt-cell--low"
              : d.charging
                ? "bt-cell--charging"
                : "bt-cell--static";
          const batteryText = !d.connected
            ? "offline"
            : d.battery === null || d.battery === undefined
              ? "—"
              : `${d.battery}%`;
          return (
            <div
              className={`bt-cell ${cellState}`}
              key={i}
              title={
                !d.connected
                  ? `${d.label} (disconnected)`
                  : `${d.label}${d.charging ? " (charging)" : ""}`
              }
            >
              <div className="bt-cell__icon">
                {isPath(d.icon) ? (
                  <img src={convertFileSrc(d.icon)} alt="" />
                ) : (
                  <span>{d.icon}</span>
                )}
              </div>
              <div
                className={`bt-cell__battery${
                  d.connected && d.stale ? " bt-cell__battery--stale" : ""
                }`}
              >
                {batteryText}
              </div>
              {many && <div className="bt-cell__name">{d.label}</div>}
            </div>
          );
        })}
      </div>
    </div>
  );
}
