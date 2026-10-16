import { useEffect, useState } from "react";
import { api } from "../api";
import type { BorderEffect, CounterCounts, Tile } from "../types";

interface Props {
  tile: Tile;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
}

/**
 * Daily fitness counter tile: a large number for today; each click adds `step`
 * (default 5), right-click undoes one step. Today resets at local midnight; on
 * hover the tile shows the lifetime total. Counts live in the backend.
 */
export function CounterTile({ tile, borderEffect = "orbit", onPointerDown }: Props) {
  const step = tile.kind.type === "counter" ? tile.kind.step || 5 : 5;
  const [counts, setCounts] = useState<CounterCounts | null>(null);

  // Load now and re-check every 30s so the number drops to 0 at midnight even
  // if the widget stays open.
  useEffect(() => {
    const load = () =>
      api.counterGet(tile.id).then(setCounts).catch(() => {});
    load();
    const t = window.setInterval(load, 30_000);
    return () => window.clearInterval(t);
  }, [tile.id]);

  const add = (delta: number) =>
    api.counterAdd(tile.id, delta).then(setCounts).catch(console.error);

  const cls = ["tile", "tile--counter", borderEffect !== "none" ? `tile--border-${borderEffect}` : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      data-tile-id={tile.id}
      onPointerDown={onPointerDown}
      onClick={() => {
        if ((window as unknown as { __oswDragged?: boolean }).__oswDragged) return;
        add(step);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        add(-step);
      }}
      title={`${tile.label}: click +${step}, right-click −${step}. Resets daily.`}
    >
      <div className="tile__label counter__label">{tile.label}</div>
      <div className="counter__value">{counts?.today ?? "…"}</div>
      <div className="counter__lifetime">
        <span>Lifetime</span>
        <strong>{counts?.lifetime ?? "…"}</strong>
      </div>
    </div>
  );
}
