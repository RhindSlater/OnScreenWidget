//! OnScreen Widget — Tauri backend library.
//!
//! Exposes Tauri commands to the frontend, runs a background poll loop that
//! checks every tile on its interval and emits `tile-status` events, and wires
//! up window mode switching, actions, secrets, and config persistence.

mod actions;
mod bluetooth;
mod counter;
mod water;
mod config;
mod history;
mod lhm;
mod media;
mod model;
mod monitors;
mod notify;
mod secrets;
mod window;
use window as win; // alias to avoid clashing with tauri's window types

use model::{ActionSpec, AppConfig, MonitorKind, Tile};
use monitors::TileStatus;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

/// Shared app state: the current config, guarded for concurrent access.
struct AppState {
    config: Arc<Mutex<AppConfig>>,
}

// --- Config commands --------------------------------------------------------

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().await.clone())
}

#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    // Safety net for the dev-reload / startup-race failure mode: refuse to
    // overwrite a populated on-disk config with an EMPTY tile list. A genuine
    // "user deleted their last tile" is rare and can be done via explicit edits;
    // this guard stops an accidental empty/default config from wiping tiles.
    if config.tiles.is_empty() {
        if let Ok(existing) = config::load() {
            if !existing.tiles.is_empty() {
                return Err(
                    "refused to overwrite existing tiles with an empty config (safety guard)"
                        .into(),
                );
            }
        }
    }
    config::save(&config)?;
    *state.config.lock().await = config.clone();
    // Notify other windows (e.g. the main widget) that config changed so they
    // can refresh. The settings window edits config in a separate window.
    let _ = app.emit("config-changed", &config);
    // Immediately re-check tiles so edits (thresholds, bluetooth devices, etc.)
    // reflect right away instead of waiting for the next poll interval.
    let app2 = app.clone();
    let tiles = config.tiles.clone();
    tauri::async_runtime::spawn(async move {
        for tile in tiles {
            if tile.enabled {
                check_and_emit(&app2, &tile).await;
            }
        }
    });
    Ok(())
}

/// Export the current config file to a user-chosen path (manual backup).
#[tauri::command]
async fn export_config(dest: String) -> Result<(), String> {
    config::export_to(&dest)
}

/// Import a config from a file: validates, backs up the current one, replaces,
/// and returns the new config so the UI can refresh.
#[tauri::command]
async fn import_config(state: State<'_, AppState>, src: String) -> Result<AppConfig, String> {
    let cfg = config::import_from(&src)?;
    *state.config.lock().await = cfg.clone();
    Ok(cfg)
}

/// Absolute path to the backups folder (so the UI can reveal/open it).
#[tauri::command]
fn backups_dir() -> Result<String, String> {
    config::backup_dir().map(|p| p.to_string_lossy().to_string())
}

// --- Window commands --------------------------------------------------------

#[tauri::command]
async fn set_window_props(
    app: AppHandle,
    state: State<'_, AppState>,
    always_on_top: bool,
    docked: bool,
) -> Result<(), String> {
    let window = win::main_window(&app)?;
    win::apply_props(&window, always_on_top, docked)?;
    let mut cfg = state.config.lock().await;
    cfg.window.always_on_top = always_on_top;
    cfg.window.docked = docked;
    config::save(&cfg)?;
    Ok(())
}

#[tauri::command]
async fn set_click_through(app: AppHandle, ignore: bool) -> Result<(), String> {
    let window = win::main_window(&app)?;
    win::set_click_through(&window, ignore)
}

#[tauri::command]
async fn move_window(app: AppHandle, x: f64, y: f64) -> Result<(), String> {
    let window = win::main_window(&app)?;
    win::move_to(&window, x, y)
}

/// Resize the window to `height` (logical px) keeping its BOTTOM edge fixed, so
/// the settings panel expands upward without moving the toolbar.
#[tauri::command]
async fn set_window_height(app: AppHandle, height: f64) -> Result<(), String> {
    let window = win::main_window(&app)?;
    win::set_height_anchored_bottom(&window, height)
}

/// Drag the docked window along its edge. For horizontal docks the X moves (Y
/// pinned); for vertical docks the Y moves (X pinned). Coords are physical px.
#[tauri::command]
async fn drag_dock(
    app: AppHandle,
    state: State<'_, AppState>,
    screen_x: i32,
    screen_y: i32,
) -> Result<(), String> {
    let window = win::main_window(&app)?;
    let cfg = state.config.lock().await.window.clone();
    win::drag_along_edge(&window, &cfg, screen_x, screen_y)
}

/// Size the docked window to one tile row (height) and the current tile count
/// Size + place the docked window using the saved dock settings (orientation,
/// side, alignment, corner, monitor). `width`/`height` are the content size the
/// frontend computed in logical pixels.
#[tauri::command]
async fn set_docked_size(
    app: AppHandle,
    state: State<'_, AppState>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let window = win::main_window(&app)?;
    let dock = state.config.lock().await.window.clone();
    win::apply_dock_placement(&window, &dock, width, height)
}

/// Open (or focus) a separate settings window on the side opposite the dock.
#[tauri::command]
async fn open_settings_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let cfg = state.config.lock().await.window.clone();
    win::open_settings(&app, &cfg)
}

/// Toggle the settings window: open it if closed, close it if open. Used by the
/// widget's gear button so a second click dismisses it.
#[tauri::command]
async fn toggle_settings_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let cfg = state.config.lock().await.window.clone();
    win::toggle_settings(&app, &cfg)
}

/// Close the settings window if open.
#[tauri::command]
async fn close_settings_window(app: AppHandle) -> Result<(), String> {
    win::close_settings(&app)
}

/// Resize the settings window to a content extent (logical px) and reposition
/// it on the side opposite the dock edge.
#[tauri::command]
async fn resize_settings_window(
    app: AppHandle,
    state: State<'_, AppState>,
    height: f64,
) -> Result<(), String> {
    let cfg = state.config.lock().await.window.clone();
    win::resize_settings(&app, &cfg, height)
}

// --- Secret commands --------------------------------------------------------

#[tauri::command]
fn set_secret(secret_ref: String, value: String) -> Result<(), String> {
    secrets::set_secret(&secret_ref, &value)
}

#[tauri::command]
fn has_secret(secret_ref: String) -> Result<bool, String> {
    Ok(secrets::get_secret(&secret_ref)?.is_some())
}

#[tauri::command]
fn delete_secret(secret_ref: String) -> Result<(), String> {
    secrets::delete_secret(&secret_ref)
}

// --- Action command ---------------------------------------------------------

#[tauri::command]
async fn run_action(action: ActionSpec) -> Result<(), String> {
    // HTTP requests are async (and read the token from the keychain); everything
    // else is a synchronous process/URL launch.
    if let ActionSpec::HttpRequest {
        method,
        url,
        auth_header,
        auth_token_secret_ref,
        body,
    } = &action
    {
        return actions::run_http_request(
            method,
            url,
            auth_header.as_deref(),
            auth_token_secret_ref.as_deref(),
            body.as_deref(),
        )
        .await;
    }
    actions::run_action(&action)
}

// --- Monitor commands -------------------------------------------------------

/// Force an immediate check of a single tile (e.g. on user request).
#[tauri::command]
async fn check_tile(tile: Tile) -> Result<TileStatus, String> {
    let mut status = monitors::check(&tile.id, &tile.kind).await;
    // Price tiles also record history and compute drop info.
    if let MonitorKind::Price { .. } = tile.kind {
        if let Some(v) = status.value {
            if let Ok(change) = history::record(&tile.id, v) {
                if change.below_base {
                    let delta = change
                        .base
                        .map(|b| format!(" ↓ {:.2} below base", b - v))
                        .unwrap_or_else(|| " ↓".into());
                    status.detail = format!("{}{}", status.detail, delta);
                }
            }
        }
    }
    Ok(status)
}

/// Water tile state: cups in the last 24 hours.
#[tauri::command]
fn water_get(tile_id: String) -> water::WaterState {
    water::get(&tile_id)
}

/// Log one cup and return the new state.
#[tauri::command]
fn water_add(tile_id: String) -> water::WaterState {
    water::add(&tile_id)
}

/// Remove the most recent cup (undo).
#[tauri::command]
fn water_undo(tile_id: String) -> water::WaterState {
    water::undo(&tile_id)
}

/// Open the Windows calendar flyout (Win+N: notification center + calendar on
/// Windows 11). Sends the shortcut because the flyout has no public API.
#[tauri::command]
fn open_calendar() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
            VK_LWIN,
        };
        const VK_N: u16 = 0x4E;
        let key = |vk: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    ..Default::default()
                },
            },
        };
        let inputs = [
            key(VK_LWIN.0, false),
            key(VK_N, false),
            key(VK_N, true),
            key(VK_LWIN.0, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err("could not send Win+N".into());
        }
    }
    Ok(())
}

/// Current value of a Counter tile: today (resets daily) and lifetime.
#[tauri::command]
fn counter_get(tile_id: String) -> counter::Counts {
    counter::get(&tile_id)
}

/// Add `delta` to a Counter tile (negative to undo) and return the new counts.
#[tauri::command]
fn counter_add(tile_id: String, delta: i64) -> counter::Counts {
    counter::add(&tile_id, delta)
}

/// Reset a Counter tile to 0.
#[tauri::command]
fn counter_reset(tile_id: String) {
    counter::reset(&tile_id)
}

/// Return the recorded price history for a tile.
#[tauri::command]
fn price_history(tile_id: String) -> Result<Vec<history::PricePoint>, String> {
    history::series_for(&tile_id)
}

/// List available temperature sensor labels (for the metrics/graph editor
/// dropdown). May be empty on Windows without admin / supported sensors.
#[tauri::command]
async fn list_temperature_components() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(monitors::temperature_component_labels)
        .await
        .map_err(|e| format!("temp scan join: {e}"))
}

/// List currently-detected Bluetooth/wireless devices (name + battery +
/// connection) for the editor dropdown. Runs the blocking reader on the
/// blocking pool so it never stalls the async runtime.
#[tauri::command]
async fn list_bluetooth_devices() -> Result<Vec<bluetooth::BtDeviceInfo>, String> {
    let devices = tokio::task::spawn_blocking(bluetooth::read_devices_all)
        .await
        .map_err(|e| format!("bt scan join: {e}"))?;
    let mut infos: Vec<bluetooth::BtDeviceInfo> = devices
        .into_iter()
        .map(|d| bluetooth::BtDeviceInfo {
            name: d.name,
            battery: d.battery,
            connected: d.connected,
        })
        .collect();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    infos.dedup_by(|a, b| a.name == b.name);
    Ok(infos)
}

/// Troubleshooting: check the LibreHardwareMonitor web server + sample readings.
#[tauri::command]
async fn hwinfo_diagnostics() -> Result<String, String> {
    tokio::task::spawn_blocking(lhm::diagnostics)
        .await
        .map_err(|e| format!("diagnostics join: {e}"))
}

/// List all available LibreHardwareMonitor sensors for the editor.
#[tauri::command]
async fn list_hardware_sensors() -> Result<Vec<lhm::SensorInfo>, String> {
    tokio::task::spawn_blocking(monitors::hardware_sensor_list)
        .await
        .map_err(|e| format!("sensor scan join: {e}"))
}

/// Send a transport command to the current system media session.
/// `action` is one of: "toggle", "next", "previous", "play", "pause".
#[tauri::command]
fn media_control(action: String) -> Result<bool, String> {
    let a = match action.as_str() {
        "toggle" => media::MediaAction::Toggle,
        "next" => media::MediaAction::Next,
        "previous" => media::MediaAction::Previous,
        "play" => media::MediaAction::Play,
        "pause" => media::MediaAction::Pause,
        other => return Err(format!("unknown media action: {other}")),
    };
    Ok(media::control(a))
}

/// Clear a tile's notification stack. Persisted. Returns 0.
#[tauri::command]
fn notify_clear(tile_id: String) -> Result<i64, String> {
    Ok(notify::clear(&tile_id))
}

/// Result of popping the next notification: the video URL to open and the
/// remaining count. `url` is None when the stack was empty.
#[derive(serde::Serialize)]
struct PopResult {
    url: Option<String>,
    count: i64,
}

/// Pop the next unseen upload off a tile's stack: returns its URL and the new
/// remaining count (both persisted). When empty, url=None and count=0.
#[tauri::command]
fn notify_pop(tile_id: String) -> Result<PopResult, String> {
    match notify::pop_next(&tile_id) {
        Some((url, count)) => Ok(PopResult { url: Some(url), count }),
        None => Ok(PopResult { url: None, count: 0 }),
    }
}

#[tauri::command]
async fn list_slideshow_files(folder: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&folder);
        if !path.is_dir() {
            return Err(format!("not a folder: {folder}"));
        }
        let mut files: Vec<String> = std::fs::read_dir(path)
            .map_err(|e| format!("{e}"))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path().extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico"
                        | "mp4" | "webm" | "ogg" | "mov" | "mkv" | "avi"
                )
            })
            .filter_map(|e| e.path().to_str().map(|s| s.to_string()))
            .collect();
        files.sort();
        Ok(files)
    })
    .await
    .map_err(|e| format!("join: {e}"))?
}

// --- Background poll loop ---------------------------------------------------

/// Check a single tile and emit its `tile-status`. Shared by the poll loop and
/// the immediate post-save refresh so edits (thresholds, devices, etc.) show up
/// right away instead of waiting for the next poll interval.
async fn check_and_emit(app: &AppHandle, tile: &Tile) {
    let mut status = monitors::check(&tile.id, &tile.kind).await;
    if let MonitorKind::Price { .. } = tile.kind {
        if let Some(v) = status.value {
            if let Ok(change) = history::record(&tile.id, v) {
                if change.below_base {
                    let delta = change
                        .base
                        .map(|b| format!(" ↓ {:.2} below base", b - v))
                        .unwrap_or_else(|| " ↓".into());
                    status.detail = format!("{}{}", status.detail, delta);
                }
            }
        }
    }
    let _ = app.emit("tile-status", &status);
}

/// Continuously poll tiles. Each tile is checked when its interval elapses.
/// Results are emitted to the frontend as `tile-status` events.
async fn poll_loop(app: AppHandle, config: Arc<Mutex<AppConfig>>) {
    use std::collections::HashMap;
    use std::time::Instant;

    let mut last_checked: HashMap<String, Instant> = HashMap::new();

    loop {
        // Snapshot current tiles + default interval.
        let (tiles, default_poll) = {
            let cfg = config.lock().await;
            (cfg.tiles.clone(), cfg.window.default_poll_secs)
        };

        let now = Instant::now();
        for tile in tiles {
            // Disabled tiles keep their config but are never polled or shown.
            if !tile.enabled {
                continue;
            }
            // Interval in milliseconds. Metrics tiles may use `poll_ms` for
            // sub-second refresh (min 100ms). All other tiles keep the 5s floor
            // and are unaffected by fast polling.
            let is_metrics = matches!(
                tile.kind,
                MonitorKind::Metrics { .. } | MonitorKind::Graph { .. } | MonitorKind::Media
            );
            let is_bluetooth = matches!(tile.kind, MonitorKind::Bluetooth { .. });
            let is_kiro = matches!(tile.kind, MonitorKind::Kiro { .. });
            let is_slideshow = matches!(tile.kind, MonitorKind::Slideshow { .. });
            let interval_ms: u128 = if is_metrics {
                if let Some(ms) = tile.poll_ms {
                    ms.max(100) as u128
                } else if let Some(secs) = tile.poll_secs {
                    // Explicit seconds override (user wants it slower).
                    (secs.max(1) as u128) * 1000
                } else {
                    // Live metrics/graph default to ~1s so readings feel live
                    // without needing the "Fast refresh (ms)" field set. The
                    // CPU (250ms) and LHM (750ms) caches keep this cheap.
                    1000
                }
            } else if is_bluetooth {
                // Connect/disconnect is now event-driven (DeviceWatcher), so this
                // poll is just a slow battery-refresh safety net.
                (tile.poll_secs.map(|s| s.max(1)).unwrap_or(30) as u128) * 1000
            } else if is_kiro {
                // Usage changes slowly and each check spawns the Kiro CLI, so
                // default to 30 min. Honor an override but floor it at 60s so we
                // never hammer the CLI.
                (tile.poll_secs.map(|s| s.max(60)).unwrap_or(1800) as u128) * 1000
            } else if is_slideshow {
                // The frontend handles rotation; backend just checks folder health.
                (tile.poll_secs.map(|s| s.max(30)).unwrap_or(300) as u128) * 1000
            } else {
                (tile.poll_secs.unwrap_or(default_poll).max(5) as u128) * 1000
            };

            let due = last_checked
                .get(&tile.id)
                .map(|t| now.duration_since(*t).as_millis() >= interval_ms)
                .unwrap_or(true);
            if !due {
                continue;
            }
            last_checked.insert(tile.id.clone(), now);

            let app = app.clone();
            // Spawn each check so a slow host doesn't delay others.
            tokio::spawn(async move {
                check_and_emit(&app, &tile).await;
            });
        }

        // Fine tick so metrics tiles can refresh multiple times per second
        // without affecting the slower cadence of other tiles.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Seed example tiles on first run (no config file yet) so the grid isn't
    // empty. Saved immediately so subsequent runs load the user's edits.
    // Safe startup load:
    //  * Fresh install (no file)  -> seed example tiles and save.
    //  * Loaded OK                -> use it as-is (never auto-overwrite).
    //  * Unreadable existing file -> DO NOT overwrite; start empty in memory and
    //    log loudly. The original file is preserved (and copied to a
    //    config.corrupt-*.json), so the user can recover it.
    let loaded = match config::load_safe() {
        config::LoadOutcome::Fresh => {
            let mut cfg = AppConfig::default();
            cfg.tiles = model::example_tiles();
            let _ = config::save(&cfg);
            cfg
        }
        config::LoadOutcome::Loaded(cfg) => cfg,
        config::LoadOutcome::Unreadable { error, corrupt_backup } => {
            log::error!(
                "config exists but could not be read ({error}). NOT overwriting it. \
                 A copy was preserved at {:?}. Starting with an empty in-memory config; \
                 use Settings → Restore to recover.",
                corrupt_backup
            );
            // Important: start empty but DO NOT save over the existing file.
            AppConfig::default()
        }
    };
    let config = Arc::new(Mutex::new(loaded));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            config: config.clone(),
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            // Main window is created here (not from tauri.conf.json alone) so it
            // can use an elevation-specific WebView2 profile.
            win::create_main_window(&handle)?;

            // System tray: gives a way to toggle visibility and quit, since the
            // dock window skips the taskbar (otherwise there's no close affordance).
            {
                use tauri::menu::{MenuBuilder, MenuItemBuilder};
                use tauri::tray::TrayIconBuilder;

                let show = MenuItemBuilder::with_id("show", "Show / Hide").build(app)?;
                let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
                let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
                let menu = MenuBuilder::new(app)
                    .item(&show)
                    .item(&settings)
                    .separator()
                    .item(&quit)
                    .build()?;

                let tray_cfg = config.clone();
                let _tray = TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("OnScreen Widget")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| match event.id().as_ref() {
                        "quit" => app.exit(0),
                        "show" => {
                            if let Ok(w) = win::main_window(app) {
                                let visible = w.is_visible().unwrap_or(true);
                                if visible {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                        "settings" => {
                            let app2 = app.clone();
                            let c = tray_cfg.clone();
                            tauri::async_runtime::spawn(async move {
                                let cfg = c.lock().await.window.clone();
                                let _ = win::open_settings(&app2, &cfg);
                            });
                        }
                        _ => {}
                    })
                    .build(app)?;
            }

            // Apply the saved window mode on startup.
            // Apply the saved window toggles on startup.
            let startup_cfg = config.clone();
            let startup_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let (aot, docked) = {
                    let c = startup_cfg.lock().await;
                    (c.window.always_on_top, c.window.docked)
                };
                if let Ok(window) = win::main_window(&startup_handle) {
                    let _ = win::apply_props(&window, aot, docked);
                }
                // Pre-build the settings window (hidden) so the first open is
                // instant instead of waiting for a fresh webview to spin up.
                let scfg = startup_cfg.lock().await.window.clone();
                let _ = win::prebuild_settings(&startup_handle, &scfg);
            });

            // Start the background poll loop.
            let loop_handle = handle.clone();
            let loop_cfg = config.clone();
            tauri::async_runtime::spawn(async move {
                poll_loop(loop_handle, loop_cfg).await;
            });

            // Event-driven Bluetooth: a DeviceWatcher fires on connect/disconnect
            // so the tile updates instantly instead of waiting for the slow poll.
            let bt_handle = handle.clone();
            let bt_cfg = config.clone();
            bluetooth::start_watcher(move || {
                let app = bt_handle.clone();
                let cfg = bt_cfg.clone();
                tauri::async_runtime::spawn(async move {
                    let tiles = { cfg.lock().await.tiles.clone() };
                    for tile in tiles {
                        if tile.enabled
                            && matches!(tile.kind, MonitorKind::Bluetooth { .. })
                        {
                            check_and_emit(&app, &tile).await;
                        }
                    }
                });
            });

            // Keep the settings window attached to the widget: when the main
            // window moves or resizes, reposition settings onto its top edge.
            // Registered once; it no-ops when settings isn't open.
            if let Ok(main) = win::main_window(&handle) {
                let follow_handle = handle.clone();
                let follow_cfg = config.clone();
                main.on_window_event(move |event| {
                    use tauri::WindowEvent;
                    if matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
                        let h = follow_handle.clone();
                        let c = follow_cfg.clone();
                        tauri::async_runtime::spawn(async move {
                            let cfg = c.lock().await.window.clone();
                            let _ = win::reposition_settings_to_main(&h, &cfg);
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            export_config,
            import_config,
            backups_dir,
            set_window_props,
            set_click_through,
            move_window,
            set_window_height,
            drag_dock,
            set_docked_size,
            open_settings_window,
            toggle_settings_window,
            close_settings_window,
            open_calendar,
            water_get,
            water_add,
            water_undo,
            counter_get,
            counter_add,
            counter_reset,
            resize_settings_window,
            set_secret,
            has_secret,
            delete_secret,
            run_action,
            check_tile,
            price_history,
            list_temperature_components,
            list_bluetooth_devices,
            hwinfo_diagnostics,
            list_hardware_sensors,
            media_control,
            notify_clear,
            notify_pop,
            list_slideshow_files,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                lhm::shutdown_helper();
            }
        });
}
