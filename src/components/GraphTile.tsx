import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { Tile, TileStatus } from "../types";

interface Props {
  tile: Tile;
  status?: TileStatus;
  /** Poll interval (ms) used to size the 5-minute rolling buffer. */
  pollMs: number;
  onPointerDown?: (e: React.PointerEvent) => void;
  onClick?: (e: React.MouseEvent) => void;
}

const WINDOW_MS = 5 * 60 * 1000; // 5 minutes
const MAX_POINTS = 600; // hard cap regardless of poll rate (RAM-light)
const TEMP_MIN = 20;
const TEMP_MAX = 120;
const TEMP_STEP = 20;

/**
 * A dependency-free rolling multi-line graph tile. Keeps a capped ring buffer
 * of recent values per metric line (in a ref, so storage is cheap) and draws
 * them as inline SVG polylines, auto-scaled to the window's max value.
 */
export function GraphTile({ tile, status, pollMs, onPointerDown, onClick }: Props) {
  const buf = useRef<Map<string, { color: string; values: number[] }>>(new Map());
  const [, setTick] = useState(0);

  const cap = useMemo(
    () => Math.min(Math.ceil(WINDOW_MS / Math.max(pollMs, 100)), MAX_POINTS),
    [pollMs]
  );

  // Append the latest sample to the buffer whenever a new status arrives.
  useEffect(() => {
    const pts = status?.graph_points;
    if (!pts || pts.length === 0) return;
    for (const p of pts) {
      let entry = buf.current.get(p.label);
      if (!entry) {
        entry = { color: p.color, values: [] };
        buf.current.set(p.label, entry);
      }
      entry.color = p.color;
      entry.values.push(p.value);
      if (entry.values.length > cap) {
        entry.values.splice(0, entry.values.length - cap);
      }
    }
    setTick((t) => t + 1); // trigger a redraw
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status?.checked_at]);

  const W = 100;
  const H = 70;

  // Temperature graphs use a fixed 20–120 °C scale and a fixed 5-minute window
  // (newest sample at the right edge) so the axis is readable at a glance.
  const metrics = tile.kind.type === "graph" ? tile.kind.metrics : [];
  const isTemp =
    metrics.length > 0 &&
    metrics.every((m) => m.source.type === "temperature" || /°\s*C$/i.test(m.unit.trim()));

  // Shared Y scale across all lines (so in/out are comparable).
  const allValues: number[] = [];
  for (const e of buf.current.values()) allValues.push(...e.values);
  const maxY = isTemp ? TEMP_MAX : Math.max(1e-9, ...allValues);
  const minY = isTemp ? TEMP_MIN : 0;
  const span = maxY - minY;
  const step = cap > 1 ? W / (cap - 1) : W;

  const lines = [...buf.current.entries()].map(([label, e]) => {
    const n = e.values.length;
    const path = e.values
      .map((v, i) => {
        const x = isTemp ? W - (n - 1 - i) * step : n > 1 ? (i / (n - 1)) * W : 0;
        const clamped = Math.min(maxY, Math.max(minY, v));
        const y = H - ((clamped - minY) / span) * H;
        return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
    return { label, color: e.color, path };
  });

  const ticks = isTemp
    ? Array.from({ length: (TEMP_MAX - TEMP_MIN) / TEMP_STEP + 1 }, (_, i) => TEMP_MIN + i * TEMP_STEP)
    : [];

  const borderColor = status?.border_color ?? null;
  const style: CSSProperties = borderColor
    ? ({
        borderColor,
        borderLeftColor: borderColor,
        ["--flash-color" as string]: borderColor,
      } as CSSProperties)
    : {};

  const cls = ["tile", "tile--graph", status?.flash ? "tile--flash" : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      data-tile-id={tile.id}
      style={style}
      onPointerDown={onPointerDown}
      onClick={onClick}
      title={status?.detail ?? tile.label}
    >
      <div className="tile__label tile__graph-label">{tile.label}</div>
      {isTemp && (
        <>
          <div className="tile__graph-yaxis">
            {ticks.map((t) => (
              <span key={t} style={{ top: `${((TEMP_MAX - t) / (TEMP_MAX - TEMP_MIN)) * 100}%` }}>
                {t}°
              </span>
            ))}
          </div>
          <div className="tile__graph-xaxis">
            <span>-5m</span>
            <span>now</span>
          </div>
        </>
      )}
      <svg
        className={isTemp ? "tile__graph-svg tile__graph-svg--axis" : "tile__graph-svg"}
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
      >
        {ticks.map((t) => {
          const y = H - ((t - TEMP_MIN) / (TEMP_MAX - TEMP_MIN)) * H;
          return (
            <line
              key={t}
              x1={0}
              x2={W}
              y1={y}
              y2={y}
              stroke="rgba(255,255,255,0.14)"
              strokeWidth={1}
              strokeDasharray={t === TEMP_MIN ? undefined : "2 3"}
              vectorEffect="non-scaling-stroke"
            />
          );
        })}        {lines.map((l) => (
          <path
            key={l.label}
            d={l.path}
            fill="none"
            stroke={l.color}
            strokeWidth={1.5}
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </svg>
    </div>
  );
}
