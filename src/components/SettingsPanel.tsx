import { useState } from "react";
import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { AppConfig, Tile } from "../types";
import { TileEditor } from "./TileEditor";
import { SecretsManager } from "./SecretsManager";

interface Props {
  config: AppConfig;
  onClose: () => void;
  /** Persist a full config (tiles and/or window). */
  onSave: (config: AppConfig) => void;
  /** Apply the two independent window toggles. */
  onSetToggles: (alwaysOnTop: boolean, docked: boolean) => void;
}

type Tab = "tiles" | "window" | "secrets";

function newTile(order: number): Tile {
  return {
    id: "",
    label: "",
    icon: null,
    kind: { type: "http_status", url: "", expected_status: 200 },
    actions: {},
    poll_secs: null,
    order,
    enabled: true,
    details_always: false,
    size: "1x1",
  };
}

export function SettingsPanel({ config, onClose, onSave, onSetToggles }: Props) {
  const [tab, setTab] = useState<Tab>("tiles");
  const [editing, setEditing] = useState<{ tile: Tile; isNew: boolean } | null>(
    null
  );

  const tiles = [...config.tiles].sort((a, b) => a.order - b.order);

  const saveTile = (tile: Tile) => {
    const isNew = !config.tiles.some((t) => t.id === tile.id);
    const nextTiles = isNew
      ? [...config.tiles, tile]
      : config.tiles.map((t) => (t.id === tile.id ? tile : t));
    onSave({ ...config, tiles: nextTiles });
    setEditing(null);
  };

  const deleteTile = (id: string) => {
    onSave({ ...config, tiles: config.tiles.filter((t) => t.id !== id) });
  };

  const toggleEnabled = (id: string) => {
    onSave({
      ...config,
      tiles: config.tiles.map((t) =>
        t.id === id ? { ...t, enabled: !(t.enabled ?? true) } : t
      ),
    });
  };

  const updateWindow = (patch: Partial<AppConfig["window"]>) => {
    onSave({ ...config, window: { ...config.window, ...patch } });
  };

  return (
    <div className="settings">
      <header className="settings__head">
        <div className="settings__tabs">
          <button
            className={`tab ${tab === "tiles" ? "tab--on" : ""}`}
            onClick={() => setTab("tiles")}
          >
            Tiles
          </button>
          <button
            className={`tab ${tab === "window" ? "tab--on" : ""}`}
            onClick={() => setTab("window")}
          >
            Window
          </button>
          <button
            className={`tab ${tab === "secrets" ? "tab--on" : ""}`}
            onClick={() => setTab("secrets")}
          >
            Secrets
          </button>
        </div>
        <button className="btn btn--ghost" onClick={onClose}>
          Done
        </button>
      </header>

      <div className="settings__body">
        {tab === "tiles" && !editing && (
          <>
            <button
              className="btn btn--full"
              onClick={() =>
                setEditing({ tile: newTile(tiles.length), isNew: true })
              }
            >
              + Add tile
            </button>
            {tiles.length === 0 && (
              <p className="hint">No tiles yet. Add one above.</p>
            )}
            <ul className="tile-list">
              {tiles.map((t) => (
                <li
                  key={t.id}
                  className={`tile-list__item ${
                    t.enabled === false ? "tile-list__item--disabled" : ""
                  }`}
                >
                  <span className="tile-list__icon">
                    {t.icon && !t.icon.includes("/") && !t.icon.includes("\\")
                      ? t.icon
                      : "•"}
                  </span>
                  <span className="tile-list__label">
                    {t.label || <em>(unnamed)</em>}
                    <small>
                      {t.kind.type}
                      {t.enabled === false ? " • disabled" : ""}
                    </small>
                  </span>
                  <button
                    className={`btn btn--xs ${
                      t.enabled === false ? "" : "btn--on"
                    }`}
                    title={
                      t.enabled === false
                        ? "Enable (show in widget)"
                        : "Disable (hide from widget, keep config)"
                    }
                    onClick={() => toggleEnabled(t.id)}
                  >
                    {t.enabled === false ? "Enable" : "Disable"}
                  </button>
                  <button
                    className="btn btn--xs"
                    onClick={() => setEditing({ tile: t, isNew: false })}
                  >
                    Edit
                  </button>
                  <button
                    className="btn btn--xs btn--danger"
                    onClick={() => deleteTile(t.id)}
                  >
                    Delete
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}

        {tab === "tiles" && editing && (
          <TileEditor
            initial={editing.tile}
            isNew={editing.isNew}
            existingIds={config.tiles.map((t) => t.id)}
            onSave={saveTile}
            onCancel={() => setEditing(null)}
          />
        )}

        {tab === "window" && (
          <div className="window-settings">
            <div className="field">
              <label>Window behavior</label>
              <div className="seg">
                <button
                  className={`chip ${config.window.always_on_top ? "chip--on" : ""}`}
                  onClick={() =>
                    onSetToggles(!config.window.always_on_top, config.window.docked)
                  }
                  title="Keep the widget above other windows"
                >
                  Overlay (always on top)
                </button>
                <button
                  className={`chip ${config.window.docked ? "chip--on" : ""}`}
                  onClick={() =>
                    onSetToggles(config.window.always_on_top, !config.window.docked)
                  }
                  title="Dock to the bottom of the screen"
                >
                  Dock to bottom
                </button>
              </div>
              <p className="hint">
                These are independent — enable either, both, or neither.
              </p>
            </div>

            {config.window.docked && (
              <div className="field">
                <label>Dock orientation</label>
                <div className="seg">
                  <button
                    className={`chip ${
                      config.window.dock_orientation === "horizontal" ? "chip--on" : ""
                    }`}
                    onClick={() =>
                      updateWindow({
                        dock_orientation: "horizontal",
                        dock_side: "bottom",
                      })
                    }
                  >
                    Horizontal
                  </button>
                  <button
                    className={`chip ${
                      config.window.dock_orientation === "vertical" ? "chip--on" : ""
                    }`}
                    onClick={() =>
                      updateWindow({
                        dock_orientation: "vertical",
                        dock_side: "left",
                      })
                    }
                  >
                    Vertical
                  </button>
                </div>

                <label style={{ marginTop: 8 }}>Side</label>
                <div className="seg">
                  {config.window.dock_orientation === "horizontal" ? (
                    <>
                      <button
                        className={`chip ${config.window.dock_side === "top" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_side: "top" })}
                      >
                        Top
                      </button>
                      <button
                        className={`chip ${config.window.dock_side === "bottom" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_side: "bottom" })}
                      >
                        Bottom
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        className={`chip ${config.window.dock_side === "left" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_side: "left" })}
                      >
                        Left
                      </button>
                      <button
                        className={`chip ${config.window.dock_side === "right" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_side: "right" })}
                      >
                        Right
                      </button>
                    </>
                  )}
                </div>

                <label style={{ marginTop: 8 }}>Alignment</label>
                <div className="seg">
                  <button
                    className={`chip ${config.window.dock_align === "centered" ? "chip--on" : ""}`}
                    onClick={() => updateWindow({ dock_align: "centered" })}
                  >
                    Centered
                  </button>
                  <button
                    className={`chip ${config.window.dock_align === "cornered" ? "chip--on" : ""}`}
                    onClick={() => updateWindow({ dock_align: "cornered" })}
                  >
                    Cornered
                  </button>
                </div>

                {config.window.dock_align === "cornered" && (
                  <>
                    <label style={{ marginTop: 8 }}>Corner</label>
                    <div className="seg">
                      <button
                        className={`chip ${config.window.dock_corner === "start" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_corner: "start" })}
                      >
                        {config.window.dock_orientation === "horizontal" ? "Left" : "Top"}
                      </button>
                      <button
                        className={`chip ${config.window.dock_corner === "end" ? "chip--on" : ""}`}
                        onClick={() => updateWindow({ dock_corner: "end" })}
                      >
                        {config.window.dock_orientation === "horizontal" ? "Right" : "Bottom"}
                      </button>
                    </div>
                  </>
                )}

                <div className="field" style={{ marginTop: 8 }}>
                  <label>Preferred monitor (0 = primary)</label>
                  <input
                    type="number"
                    min={0}
                    value={config.window.dock_monitor ?? ""}
                    placeholder="current"
                    onChange={(e) =>
                      updateWindow({
                        dock_monitor:
                          e.target.value === "" ? null : Math.max(0, Number(e.target.value) || 0),
                      })
                    }
                  />
                </div>
              </div>
            )}

            <div className="field">
              <label>Opacity ({Math.round((config.window.opacity ?? 1) * 100)}%)</label>
              <input
                type="range"
                min={30}
                max={100}
                step={1}
                value={Math.round((config.window.opacity ?? 1) * 100)}
                onChange={(e) =>
                  updateWindow({ opacity: Number(e.target.value) / 100 })
                }
              />
            </div>

            <div className="field field--sm">
              <label>Border effect</label>
              <select
                value={config.window.border_effect}
                onChange={(e) =>
                  updateWindow({
                    border_effect: e.target
                      .value as AppConfig["window"]["border_effect"],
                  })
                }
              >
                <option value="orbit">Orbit (colour circles)</option>
                <option value="breathing">Breathing (fade in/out)</option>
                <option value="pulse">Pulse (glow)</option>
                <option value="rainbow">Rainbow (hue cycle)</option>
                <option value="static">Static (solid)</option>
                <option value="none">None</option>
              </select>
            </div>

            <div className="field field--sm">
              <label>Default refresh (secs)</label>
              <input
                type="number"
                min={5}
                value={config.window.default_poll_secs}
                onChange={(e) =>
                  updateWindow({
                    default_poll_secs: Math.max(5, Number(e.target.value) || 60),
                  })
                }
              />
            </div>
            <p className="hint">
              Opacity and refresh changes save instantly. Tiles pick up the new
              default on their next check.
            </p>

            <div className="editor__section" style={{ marginTop: 14 }}>
              Backup &amp; restore
            </div>
            <BackupRestore onImported={onSave} />
          </div>
        )}

        {tab === "secrets" && <SecretsManager />}
      </div>
    </div>
  );
}

/** Export/import the whole config to/from a file, plus a note about auto-backups. */
function BackupRestore({ onImported }: { onImported: (cfg: AppConfig) => void }) {
  const [status, setStatus] = useState<string | null>(null);

  const doExport = async () => {
    try {
      const dest = await saveDialog({
        title: "Export config",
        defaultPath: "onscreen-widget-config.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!dest) return;
      await api.exportConfig(dest);
      setStatus(`Exported to ${dest}`);
    } catch (e) {
      setStatus(`Export failed: ${String(e)}`);
    }
  };

  const doImport = async () => {
    try {
      const src = await openDialog({
        title: "Restore config from file",
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof src !== "string") return;
      const cfg = await api.importConfig(src);
      onImported(cfg);
      setStatus(`Restored from ${src}`);
    } catch (e) {
      setStatus(`Restore failed: ${String(e)}`);
    }
  };

  const showBackups = async () => {
    try {
      const dir = await api.backupsDir();
      setStatus(`Automatic backups are kept in: ${dir}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  return (
    <div className="backup">
      <div className="editor__actions" style={{ justifyContent: "flex-start" }}>
        <button className="btn" onClick={doExport}>
          Export…
        </button>
        <button className="btn btn--ghost" onClick={doImport}>
          Restore…
        </button>
        <button className="btn btn--ghost" onClick={showBackups}>
          Backups folder
        </button>
      </div>
      <p className="hint">
        The app also keeps automatic timestamped backups every time the config is
        saved. If tiles ever go missing, Restore from a backup file.
      </p>
      {status && <div className="note">{status}</div>}
    </div>
  );
}
