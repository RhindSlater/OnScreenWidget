import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, onTileStatus, onConfigChanged } from "./api";
import { TileCard } from "./components/TileCard";
import { GraphTile } from "./components/GraphTile";
import { BluetoothTile } from "./components/BluetoothTile";
import { MediaTile } from "./components/MediaTile";
import { SlideshowTile } from "./components/SlideshowTile";
import { CounterTile } from "./components/CounterTile";
import { ClockTile } from "./components/ClockTile";
import { WaterTile } from "./components/WaterTile";
import { PriceHistoryModal } from "./components/PriceHistoryModal";
import type { ActionSpec, AppConfig, Tile, TileStatus } from "./types";

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [statuses, setStatuses] = useState<Record<string, TileStatus>>({});
  const [historyTile, setHistoryTile] = useState<Tile | null>(null);
  // Drag-to-reorder state: the ids of the dragged tile and the tile hovered over.
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);
  const titlebarRef = useRef<HTMLElement | null>(null);
  // Tracks the previous docked state so we only force the docked bar size on
  // the transition into dock mode (not when dock is turned off).
  const prevDockedRef = useRef<boolean>(false);

  // Load config and subscribe to live status updates.
  useEffect(() => {
    // Attach the live-status listener first so we don't miss any backend emits.
    const unlisten = onTileStatus((s) => {
      setStatuses((prev) => ({ ...prev, [s.tile_id]: s }));
    });

    // Refresh when the config changes (e.g. edited in the settings window),
    // and re-check all tiles so new/edited ones update immediately.
    const unlistenCfg = onConfigChanged((cfg) => {
      setConfig(cfg);
      for (const tile of cfg.tiles) {
        api
          .checkTile(tile)
          .then((status) =>
            setStatuses((prev) => ({ ...prev, [status.tile_id]: status }))
          )
          .catch(() => {});
      }
    });

    // Load config, then immediately check every tile so results appear right
    // away on open instead of waiting for the first poll-loop cycle. (The
    // background poll loop then keeps them updated on each tile's interval.)
    api
      .getConfig()
      .then((cfg) => {
        setConfig(cfg);
        for (const tile of cfg.tiles) {
          api
            .checkTile(tile)
            .then((status) =>
              setStatuses((prev) => ({ ...prev, [status.tile_id]: status }))
            )
            .catch((e) => console.error("initial check failed", tile.id, e));
        }
      })
      .catch(console.error);

    return () => {
      unlisten.then((f) => f());
      unlistenCfg.then((f) => f());
    };
  }, []);

  // When docked, the title bar drags the window ALONG its edge. We track both
  // pointer axes and send both to the backend, which moves only the free axis
  // for the current dock side (X for top/bottom, Y for left/right).
  const onTitlebarPointerDown = useCallback(async (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const dpr = window.devicePixelRatio || 1;
    const startScreenXPhys = e.screenX * dpr;
    const startScreenYPhys = e.screenY * dpr;
    let startWinX = 0;
    let startWinY = 0;
    try {
      const pos = await getCurrentWindow().outerPosition();
      startWinX = pos.x;
      startWinY = pos.y;
    } catch (err) {
      console.error("read window pos failed", err);
      return;
    }
    const onMove = (ev: PointerEvent) => {
      const targetX = Math.round(startWinX + (ev.screenX * dpr - startScreenXPhys));
      const targetY = Math.round(startWinY + (ev.screenY * dpr - startScreenYPhys));
      api.dragDock(targetX, targetY).catch(() => {});
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }, []);

  // Apply the two independent window toggles (always-on-top / docked).
  const setWindowToggles = useCallback(
    async (alwaysOnTop: boolean, docked: boolean) => {
      await api.setWindowProps(alwaysOnTop, docked);
      setConfig((c) =>
        c
          ? {
              ...c,
              window: { ...c.window, always_on_top: alwaysOnTop, docked },
            }
          : c
      );
    },
    []
  );

  // Move the dragged tile to the dropped-on tile's position and persist the
  // new order. `order` values are reassigned sequentially after the move.
  const reorderTiles = useCallback((fromId: string, toId: string) => {
    if (fromId === toId) return;
    setConfig((c) => {
      if (!c) return c;
      const ordered = [...c.tiles].sort((a, b) => a.order - b.order);
      const fromIdx = ordered.findIndex((t) => t.id === fromId);
      const toIdx = ordered.findIndex((t) => t.id === toId);
      if (fromIdx === -1 || toIdx === -1) return c;
      const [moved] = ordered.splice(fromIdx, 1);
      ordered.splice(toIdx, 0, moved);
      const renumbered = ordered.map((t, i) => ({ ...t, order: i }));
      const next = { ...c, tiles: renumbered };
      api.saveConfig(next).catch((e) => console.error("save order failed", e));
      return next;
    });
  }, []);

  // Pointer-based drag reordering. Native HTML5 DnD is unreliable in WebView2,
  // so we track pointermove/up ourselves and hit-test tiles by their
  // `data-tile-id` attribute via elementFromPoint.
  const onReorderStart = useCallback(
    (tileId: string, e: React.PointerEvent) => {
      const startX = e.clientX;
      const startY = e.clientY;
      const THRESHOLD = 5; // px before a press becomes a drag
      let dragging = false;
      let lastOver: string | null = null;
      // Clear any prior drag marker.
      (window as unknown as { __oswDragged?: boolean }).__oswDragged = false;

      const tileUnder = (x: number, y: number): string | null => {
        const el = document.elementFromPoint(x, y);
        const tileEl = el?.closest("[data-tile-id]") as HTMLElement | null;
        return tileEl?.getAttribute("data-tile-id") ?? null;
      };

      const onMove = (ev: PointerEvent) => {
        if (!dragging) {
          if (
            Math.abs(ev.clientX - startX) > THRESHOLD ||
            Math.abs(ev.clientY - startY) > THRESHOLD
          ) {
            dragging = true;
            setDragId(tileId);
            document.body.style.cursor = "grabbing";
          } else {
            return;
          }
        }
        const over = tileUnder(ev.clientX, ev.clientY);
        if (over !== lastOver) {
          lastOver = over;
          setOverId(over);
        }
      };

      const onUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        document.body.style.cursor = "";
        if (dragging) {
          const target = tileUnder(ev.clientX, ev.clientY);
          if (target) reorderTiles(tileId, target);
          // Mark that a drag just happened so the trailing click is ignored.
          (window as unknown as { __oswDragged?: boolean }).__oswDragged = true;
          setTimeout(() => {
            (window as unknown as { __oswDragged?: boolean }).__oswDragged = false;
          }, 50);
        }
        setDragId(null);
        setOverId(null);
      };

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [reorderTiles]
  );

  const onBackendAction = useCallback(async (action: ActionSpec) => {
    try {
      await api.runAction(action);
    } catch (e) {
      console.error("action failed", e);
    }
  }, []);

  const onFrontendAction = useCallback(
    async (tile: Tile, count: 1 | 2 | 3, action: ActionSpec) => {
      if (action.type !== "frontend") return;
      // Interpret frontend action names per feature.
      switch (action.name) {
        case "price_history":
          setHistoryTile(tile);
          break;
        case "open_latest_video": {
          // If there are notifications, pop the next new video (open it,
          // decrement count, remove from stack). Otherwise open YouTube home.
          try {
            const res = await api.notifyPop(tile.id);
            if (res.url) {
              await api.runAction({ type: "open_url", url: res.url });
              setBadge(tile.id, res.count);
            } else {
              await api.runAction({
                type: "open_url",
                url: "https://www.youtube.com",
              });
              setBadge(tile.id, 0);
            }
            // Refresh so the description reflects the NEXT video (new top of
            // stack) or the no-notifications message.
            api
              .checkTile(tile)
              .then((s) => setStatuses((prev) => ({ ...prev, [s.tile_id]: s })))
              .catch(() => {});
          } catch (e) {
            console.error("youtube pop failed", e);
          }
          break;
        }
        case "clear_badge": {
          try {
            const n = await api.notifyClear(tile.id);
            setBadge(tile.id, n);
          } catch (e) {
            console.error("clear failed", e);
          }
          break;
        }
        default:
          console.warn("unknown frontend action", action.name, "count", count);
      }
    },
    [statuses]
  );

  // Set a tile's badge to an absolute value (from the persisted backend count).
  const setBadge = useCallback((tileId: string, value: number) => {
    setStatuses((prev) => {
      const s = prev[tileId];
      if (!s) return prev;
      return { ...prev, [tileId]: { ...s, badge: value } };
    });
  }, []);

  const sortedTiles = useMemo(
    () =>
      config
        ? [...config.tiles]
            .filter((t) => t.enabled !== false)
            .sort((a, b) => a.order - b.order)
        : [],
    [config]
  );

  const dockedOn = config?.window.docked ?? false;
  const tileCount = sortedTiles.length;
  const dockOrientation = config?.window.dock_orientation ?? "horizontal";
  // Any change to these dock settings should re-place/re-size the window.
  const dockPlacementKey = config
    ? `${config.window.dock_side}|${config.window.dock_align}|${config.window.dock_corner}|${config.window.dock_monitor ?? ""}`
    : "";

  // Dock sizing: single row (horizontal) or single column (vertical) that fits
  // the tile count. Dock OFF keeps the current size.
  useEffect(() => {
    const TILE = 110;
    const GAP = 8;
    const GRID_PAD = 10;
    const BAR = (titlebarRef.current?.offsetHeight ?? 34) + 4;
    const FUDGE = 20;
    const n = Math.max(tileCount, 1);
    const tilesExtent = GRID_PAD * 2 + n * TILE + (n - 1) * GAP + FUDGE;
    const oneTileExtent = GRID_PAD * 2 + TILE;

    const wasDocked = prevDockedRef.current;
    prevDockedRef.current = dockedOn;

    if (!dockedOn && wasDocked) return; // dock-disable → keep size

    if (dockedOn) {
      let width: number;
      let height: number;
      if (dockOrientation === "vertical") {
        // Tall & narrow: side toolbar adds width; tiles stack downward.
        width = BAR + oneTileExtent;
        height = tilesExtent;
      } else {
        // Short & wide: top toolbar; tiles in a row.
        width = tilesExtent;
        height = BAR + oneTileExtent;
      }
      api
        .setDockedSize(width, height)
        .catch((e) => console.error("dock size failed", e));
    }
  }, [dockedOn, tileCount, dockOrientation, dockPlacementKey]);

  if (!config) {
    return <div className="loading">Loading…</div>;
  }

  const alwaysOnTop = config.window.always_on_top;
  const docked = config.window.docked;
  const vertical = docked && config.window.dock_orientation === "vertical";
  const dockSide = docked ? config.window.dock_side : "none";
  const dockAlign = docked ? config.window.dock_align : "centered";
  const dockCorner = config.window.dock_corner;

  return (
    <div
      className="app"
      data-docked={docked ? "true" : "false"}
      data-orientation={vertical ? "vertical" : "horizontal"}
      data-dock-side={dockSide}
      data-dock-align={dockAlign}
      data-dock-corner={dockCorner}
      style={{ opacity: config.window.opacity }}
    >
      <header
        className="titlebar"
        ref={titlebarRef}
        {...(docked ? {} : { "data-tauri-drag-region": true })}
        onPointerDown={docked ? onTitlebarPointerDown : undefined}
      >
        <span
          className="titlebar__name"
          {...(docked ? {} : { "data-tauri-drag-region": true })}
        >
          OnScreen Widget
        </span>
        <div className="titlebar__controls">
          <button
            className={`chip ${alwaysOnTop ? "chip--on" : ""}`}
            onClick={() => setWindowToggles(!alwaysOnTop, docked)}
            title="Keep the widget above other windows (overlay)"
          >
            Overlay
          </button>
          <button
            className={`chip ${docked ? "chip--on" : ""}`}
            onClick={() => setWindowToggles(alwaysOnTop, !docked)}
            title="Dock to the bottom of the screen (independent of overlay)"
          >
            Dock
          </button>
          <button
            className="chip"
            onClick={() => api.toggleSettingsWindow().catch(console.error)}
            title="Settings"
          >
            ⚙
          </button>
        </div>
      </header>

      {sortedTiles.length === 0 ? (
        <div className="empty">
          <p>No tiles yet.</p>
          <p className="empty__hint">
            Click the ⚙ button above to open Settings and add a tile. Each tile
            monitors a service, price, channel, email, or AI credits.
          </p>
        </div>
      ) : (
        <div className="grid">
          {sortedTiles.map((tile) =>
            tile.kind.type === "graph" ? (
              <GraphTile
                key={tile.id}
                tile={tile}
                status={statuses[tile.id]}
                pollMs={tile.poll_ms ?? (tile.poll_secs ?? 2) * 1000}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "bluetooth" ? (
              <BluetoothTile
                key={tile.id}
                tile={tile}
                status={statuses[tile.id]}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "media" ? (
              <MediaTile
                key={tile.id}
                tile={tile}
                status={statuses[tile.id]}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "water" ? (
              <WaterTile
                key={tile.id}
                tile={tile}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "clock" ? (
              <ClockTile
                key={tile.id}
                tile={tile}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "counter" ? (
              <CounterTile
                key={tile.id}
                tile={tile}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : tile.kind.type === "slideshow" ? (
              <SlideshowTile
                key={tile.id}
                tile={tile}
                status={statuses[tile.id]}
                borderEffect={config.window.border_effect}
                onPointerDown={(e) => {
                  if (e.button === 0) onReorderStart(tile.id, e);
                }}
              />
            ) : (
              <TileCard
                key={tile.id}
                tile={tile}
                status={statuses[tile.id]}
                borderEffect={config.window.border_effect}
                showDetails={tile.details_always}
                onFrontendAction={onFrontendAction}
                onBackendAction={onBackendAction}
                draggable
                isDragging={dragId === tile.id}
                isDragOver={overId === tile.id && dragId !== tile.id}
                onReorderStart={onReorderStart}
              />
            )
          )}
        </div>
      )}

      {historyTile && (
        <PriceHistoryModal
          tileId={historyTile.id}
          label={historyTile.label}
          onClose={() => setHistoryTile(null)}
        />
      )}
    </div>
  );
}
