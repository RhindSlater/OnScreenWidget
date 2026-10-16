// Thin typed wrappers around the Tauri backend commands.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  WaterState,
  CounterCounts,
  ActionSpec,
  AppConfig,
  BtDeviceInfo,
  HardwareSensorInfo,
  PricePoint,
  Tile,
  TileStatus,
} from "./types";

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (config: AppConfig) => invoke<void>("save_config", { config }),
  exportConfig: (dest: string) => invoke<void>("export_config", { dest }),
  importConfig: (src: string) => invoke<AppConfig>("import_config", { src }),
  backupsDir: () => invoke<string>("backups_dir"),

  setWindowProps: (alwaysOnTop: boolean, docked: boolean) =>
    invoke<void>("set_window_props", { alwaysOnTop, docked }),
  setClickThrough: (ignore: boolean) => invoke<void>("set_click_through", { ignore }),
  moveWindow: (x: number, y: number) => invoke<void>("move_window", { x, y }),
  setWindowHeight: (height: number) => invoke<void>("set_window_height", { height }),
  dragDock: (screenX: number, screenY: number) =>
    invoke<void>("drag_dock", { screenX, screenY }),
  setDockedSize: (width: number, height: number) =>
    invoke<void>("set_docked_size", { width, height }),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
  toggleSettingsWindow: () => invoke<void>("toggle_settings_window"),
  closeSettingsWindow: () => invoke<void>("close_settings_window"),
  openCalendar: () => invoke<void>("open_calendar"),
  waterGet: (tileId: string) => invoke<WaterState>("water_get", { tileId }),
  waterAdd: (tileId: string) => invoke<WaterState>("water_add", { tileId }),
  waterUndo: (tileId: string) => invoke<WaterState>("water_undo", { tileId }),
  counterGet: (tileId: string) => invoke<CounterCounts>("counter_get", { tileId }),
  counterAdd: (tileId: string, delta: number) =>
    invoke<CounterCounts>("counter_add", { tileId, delta }),
  counterReset: (tileId: string) => invoke<void>("counter_reset", { tileId }),
  resizeSettingsWindow: (height: number) =>
    invoke<void>("resize_settings_window", { height }),

  setSecret: (secretRef: string, value: string) =>
    invoke<void>("set_secret", { secretRef, value }),
  hasSecret: (secretRef: string) => invoke<boolean>("has_secret", { secretRef }),
  deleteSecret: (secretRef: string) => invoke<void>("delete_secret", { secretRef }),

  runAction: (action: ActionSpec) => invoke<void>("run_action", { action }),

  checkTile: (tile: Tile) => invoke<TileStatus>("check_tile", { tile }),
  priceHistory: (tileId: string) => invoke<PricePoint[]>("price_history", { tileId }),
  listTemperatureComponents: () =>
    invoke<string[]>("list_temperature_components"),
  listBluetoothDevices: () => invoke<BtDeviceInfo[]>("list_bluetooth_devices"),
  listHardwareSensors: () =>
    invoke<HardwareSensorInfo[]>("list_hardware_sensors"),
  hwinfoDiagnostics: () => invoke<string>("hwinfo_diagnostics"),
  mediaControl: (action: "toggle" | "next" | "previous" | "play" | "pause") =>
    invoke<boolean>("media_control", { action }),
  notifyClear: (tileId: string) => invoke<number>("notify_clear", { tileId }),
  notifyPop: (tileId: string) =>
    invoke<{ url: string | null; count: number }>("notify_pop", { tileId }),
  listSlideshowFiles: (folder: string) =>
    invoke<string[]>("list_slideshow_files", { folder }),
};

/** Subscribe to live tile status updates emitted by the backend poll loop. */
export function onTileStatus(cb: (status: TileStatus) => void): Promise<UnlistenFn> {
  return listen<TileStatus>("tile-status", (event) => cb(event.payload));
}

/** Subscribe to config changes (e.g. saved from the settings window). */
export function onConfigChanged(cb: (config: AppConfig) => void): Promise<UnlistenFn> {
  return listen<AppConfig>("config-changed", (event) => cb(event.payload));
}
