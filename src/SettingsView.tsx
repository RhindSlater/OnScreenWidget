import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "./api";
import { SettingsPanel } from "./components/SettingsPanel";
import type { AppConfig } from "./types";

/**
 * Standalone settings window content. Loads the config, renders the settings
 * panel, persists changes, applies window toggles to the MAIN window, and sizes
 * its OWN window to fit its content (growing upward above the main window).
 */
export function SettingsView() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    api.getConfig().then(setConfig).catch(console.error);
  }, []);

  // Resize this window to fit the rendered content. For settings placed to the
  // left/right of the widget (vertical dock), the growth axis is WIDTH; for
  // above/below it's HEIGHT.
  const fitToContent = useCallback(() => {
    const el = rootRef.current;
    if (!el) return;
    const side = config?.window;
    const isSideways =
      !!side &&
      side.docked &&
      (side.dock_side === "left" || side.dock_side === "right");
    const extent = isSideways ? el.offsetWidth : el.offsetHeight;
    if (extent > 0) {
      api
        .resizeSettingsWindow(extent)
        .catch((e) => console.error("resize settings failed", e));
    }
  }, [config]);

  // Re-fit whenever the content size changes (tabs, tile list edits, etc.).
  useLayoutEffect(() => {
    if (!config) return;
    fitToContent();
    const ro = new ResizeObserver(() => fitToContent());
    if (rootRef.current) ro.observe(rootRef.current);
    return () => ro.disconnect();
  }, [config, fitToContent]);

  const persistConfig = useCallback((next: AppConfig) => {
    setConfig(next);
    api.saveConfig(next).catch((e) => console.error("save config failed", e));
  }, []);

  const setToggles = useCallback(
    (alwaysOnTop: boolean, docked: boolean) => {
      api.setWindowProps(alwaysOnTop, docked).catch(console.error);
      setConfig((c) =>
        c ? { ...c, window: { ...c.window, always_on_top: alwaysOnTop, docked } } : c
      );
    },
    []
  );

  const close = useCallback(() => {
    api.closeSettingsWindow().catch(console.error);
  }, []);

  if (!config) {
    return (
      <div className="settings-window" ref={rootRef}>
        <div className="loading">Loading…</div>
      </div>
    );
  }

  return (
    <div className="settings-window" ref={rootRef}>
      <SettingsPanel
        config={config}
        onClose={close}
        onSave={persistConfig}
        onSetToggles={setToggles}
      />
    </div>
  );
}
