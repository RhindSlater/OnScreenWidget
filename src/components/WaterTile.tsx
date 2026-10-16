import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { api } from "../api";
import type { BorderEffect, Tile, WaterState } from "../types";

interface Props {
  tile: Tile;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
}

/** Tile colour per quarter of the goal: 0–25%, 25–50%, 50–75%, 75%+. */
const QUARTER_COLORS = ["#f87171", "#fb923c", "#fbbf24", "#4ade80"];

function quarterIndex(cups: number, goal: number): number {
  const pct = goal > 0 ? cups / goal : 0;
  return Math.min(3, Math.floor(pct * 4));
}

/**
 * Water intake tile. Click adds one cup, right-click removes the last one.
 * Each cup counts for 24 hours from when it was added (rolling window), so the
 * progress never resets at midnight; the tile colour steps up every quarter of
 * the goal.
 */
export function WaterTile({ tile, borderEffect = "orbit", onPointerDown }: Props) {
  const goal = Math.max(1, tile.kind.type === "water" ? tile.kind.goal_cups || 12 : 12);
  const [state, setState] = useState<WaterState | null>(null);

  // Load now, re-check every 30s, and refresh right when the oldest cup expires.
  useEffect(() => {
    let timer: number;
    const load = () =>
      api
        .waterGet(tile.id)
        .then((s) => {
          setState(s);
          window.clearTimeout(timer);
          const wait = s.next_expiry_ms
            ? Math.min(30_000, Math.max(1_000, s.next_expiry_ms - Date.now() + 500))
            : 30_000;
          timer = window.setTimeout(load, wait);
        })
        .catch(() => {
          timer = window.setTimeout(load, 30_000);
        });
    load();
    return () => window.clearTimeout(timer);
  }, [tile.id]);

  const cups = state?.cups ?? 0;
  const pct = Math.min(100, (cups / goal) * 100);
  const color = QUARTER_COLORS[quarterIndex(cups, goal)];
  const showCups = goal <= 16;

  const style: CSSProperties = {
    borderColor: color,
    borderLeftColor: color,
    background: `color-mix(in srgb, ${color} 14%, var(--bg-tile))`,
    ["--border-accent" as string]: color,
    ["--flash-color" as string]: color,
  } as CSSProperties;

  const cls = ["tile", "tile--water", borderEffect !== "none" ? `tile--border-${borderEffect}` : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      data-tile-id={tile.id}
      style={style}
      onPointerDown={onPointerDown}
      onClick={() => {
        if ((window as unknown as { __oswDragged?: boolean }).__oswDragged) return;
        api.waterAdd(tile.id).then(setState).catch(console.error);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        api.waterUndo(tile.id).then(setState).catch(console.error);
      }}
      title={`${tile.label}: ${cups}/${goal} cups in the last 24h. Click +1, right-click −1.`}
    >
      <div className="tile__label water__label">{tile.label}</div>
      <div className="water__count">
        <strong>{cups}</strong>
        <span>/ {goal} cups</span>
      </div>
      {showCups && (
        <div className="water__cups">
          {Array.from({ length: goal }, (_, i) => (
            <span key={i} className={i < cups ? "water__cup water__cup--full" : "water__cup"}>
              🥛
            </span>
          ))}
        </div>
      )}
      <div className="water__bar">
        <div className="water__fill" style={{ width: `${pct}%`, background: color }} />
      </div>
    </div>
  );
}
