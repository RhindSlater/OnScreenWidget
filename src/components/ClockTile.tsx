import { useEffect, useState } from "react";
import { api } from "../api";
import type { BorderEffect, Tile } from "../types";

interface Props {
  tile: Tile;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
}

/** Date & time tile. Clicking it opens the Windows calendar flyout. */
export function ClockTile({ tile, borderEffect = "orbit", onPointerDown }: Props) {
  const [now, setNow] = useState(() => new Date());

  // Tick on the minute boundary: no per-second redraws, so it stays light.
  useEffect(() => {
    let t: number;
    const schedule = () => {
      const d = new Date();
      t = window.setTimeout(() => {
        setNow(new Date());
        schedule();
      }, 60_000 - (d.getSeconds() * 1000 + d.getMilliseconds()) + 50);
    };
    schedule();
    return () => window.clearTimeout(t);
  }, []);

  const cls = ["tile", "tile--clock", borderEffect !== "none" ? `tile--border-${borderEffect}` : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      data-tile-id={tile.id}
      onPointerDown={onPointerDown}
      onClick={() => {
        if ((window as unknown as { __oswDragged?: boolean }).__oswDragged) return;
        api.openCalendar().catch(console.error);
      }}
      title="Open calendar"
    >
      <div className="clock__time">
        {now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}
      </div>
      <div className="clock__date">
        {now.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}
      </div>
    </div>
  );
}
