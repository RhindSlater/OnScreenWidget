import { useEffect, useState } from "react";
import { api } from "../api";
import type { PricePoint } from "../types";

interface Props {
  tileId: string;
  label: string;
  onClose: () => void;
}

/** Simple inline SVG sparkline of the price history. No chart dependency. */
export function PriceHistoryModal({ tileId, label, onClose }: Props) {
  const [points, setPoints] = useState<PricePoint[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .priceHistory(tileId)
      .then(setPoints)
      .finally(() => setLoading(false));
  }, [tileId]);

  const values = points.map((p) => p.value);
  const min = values.length ? Math.min(...values) : 0;
  const max = values.length ? Math.max(...values) : 1;
  const range = max - min || 1;
  const w = 300;
  const h = 120;

  const path = points
    .map((p, i) => {
      const x = points.length > 1 ? (i / (points.length - 1)) * w : 0;
      const y = h - ((p.value - min) / range) * h;
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");

  const latest = values.at(-1);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__title">{label} — price history</div>
        {loading ? (
          <div className="modal__empty">loading…</div>
        ) : points.length === 0 ? (
          <div className="modal__empty">No history recorded yet.</div>
        ) : (
          <>
            <svg width={w} height={h} className="sparkline">
              <path d={path} fill="none" stroke="#4ade80" strokeWidth={2} />
            </svg>
            <div className="modal__stats">
              <span>low {min.toFixed(2)}</span>
              <span>high {max.toFixed(2)}</span>
              <span>now {latest?.toFixed(2)}</span>
              <span>{points.length} pts</span>
            </div>
          </>
        )}
        <button className="btn" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
