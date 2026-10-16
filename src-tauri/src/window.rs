//! Window management: switch between Overlay and Dock modes.
//!
//! - **Overlay**: always-on-top, floating, user-positioned. Great for keeping
//!   the widget visible over other apps.
//! - **Dock**: not always-on-top, snapped to the bottom-center of the primary
//!   monitor's work area, skips the taskbar — behaves like a desktop widget.

use crate::model::{DockAlign, DockCorner, DockSide, WindowConfig};
use tauri::{
    AppHandle, LogicalPosition, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const MAIN: &str = "main";
const SETTINGS: &str = "settings";

/// True when this process runs with administrator rights.
#[cfg(windows)]
fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// Separate WebView2 profile per elevation level. An elevated and a normal copy
/// of the widget must not share one browser process: the normal copy's window
/// would then be hosted by an elevated browser and Windows blocks its keyboard
/// input (mouse still works).
fn webview_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let sub = if is_elevated() { "EBWebView-admin" } else { "EBWebView-user" };
        return dirs::data_local_dir().map(|d| d.join("com.onscreenwidget.app").join(sub));
    }
    #[allow(unreachable_code)]
    None
}

/// Create the main window from its tauri.conf.json entry (`create: false`)
/// using the elevation-specific WebView2 profile.
pub fn create_main_window(app: &AppHandle) -> Result<(), String> {
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN)
        .cloned()
        .ok_or("main window config missing")?;
    let mut b = WebviewWindowBuilder::from_config(app, &cfg).map_err(|e| e.to_string())?;
    if let Some(dir) = webview_data_dir() {
        b = b.data_directory(dir);
    }
    b.build().map_err(|e| format!("build main window: {e}"))?;
    Ok(())
}

/// Where the settings window sits relative to the widget, derived from the
/// dock side. Returns which edge of the MAIN window the settings attaches to.
enum SettingsPlacement {
    Above,
    Below,
    LeftOf,
    RightOf,
}

fn settings_placement(cfg: &WindowConfig) -> SettingsPlacement {
    if !cfg.docked {
        return SettingsPlacement::Above;
    }
    match cfg.dock_side {
        DockSide::Bottom => SettingsPlacement::Above,
        DockSide::Top => SettingsPlacement::Below,
        DockSide::Left => SettingsPlacement::RightOf,
        DockSide::Right => SettingsPlacement::LeftOf,
    }
}

/// Position the settings window adjacent to the main window on the side opposite
/// the dock edge, sizing the shared dimension to match the main window.
/// `content_logical` is the settings content extent (height for above/below,
/// width for left/right). Preserves the other dimension if `keep_other` given.
fn place_settings(
    app: &AppHandle,
    cfg: &WindowConfig,
    content_logical: Option<f64>,
) -> Result<(), String> {
    let settings = match app.get_webview_window(SETTINGS) {
        Some(w) => w,
        None => return Ok(()),
    };
    let main = main_window(app)?;
    let main_pos = main.outer_position().map_err(|e| format!("main pos: {e}"))?;
    let main_size = main.outer_size().map_err(|e| format!("main size: {e}"))?;
    let scale = settings.scale_factor().map_err(|e| format!("scale: {e}"))?;
    let s_size = settings.outer_size().map_err(|e| format!("settings size: {e}"))?;

    match settings_placement(cfg) {
        SettingsPlacement::Above | SettingsPlacement::Below => {
            // Match width to main; height = content (or keep current).
            let h = content_logical
                .map(|c| (c * scale).round() as i32)
                .unwrap_or(s_size.height as i32)
                .max(1);
            settings
                .set_size(tauri::PhysicalSize::new(main_size.width, h as u32))
                .map_err(|e| format!("size settings: {e}"))?;
            let x = main_pos.x;
            let y = match settings_placement(cfg) {
                SettingsPlacement::Below => main_pos.y + main_size.height as i32,
                _ => main_pos.y - h, // Above
            };
            settings
                .set_position(PhysicalPosition::new(x, y))
                .map_err(|e| format!("position settings: {e}"))?;
        }
        SettingsPlacement::LeftOf | SettingsPlacement::RightOf => {
            // Match height to main; width = content (or keep current).
            let w = content_logical
                .map(|c| (c * scale).round() as i32)
                .unwrap_or(s_size.width as i32)
                .max(1);
            settings
                .set_size(tauri::PhysicalSize::new(w as u32, main_size.height))
                .map_err(|e| format!("size settings: {e}"))?;
            let y = main_pos.y;
            let x = match settings_placement(cfg) {
                SettingsPlacement::RightOf => main_pos.x + main_size.width as i32,
                _ => main_pos.x - w, // LeftOf
            };
            settings
                .set_position(PhysicalPosition::new(x, y))
                .map_err(|e| format!("position settings: {e}"))?;
        }
    }
    Ok(())
}

/// Builder for the settings window (hidden), using this run's WebView2 profile.
fn settings_builder(app: &AppHandle, init_logical: f64) -> WebviewWindowBuilder<'_, tauri::Wry, AppHandle> {
    let mut b = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html#settings".into()))
        .title("Settings")
        .inner_size(init_logical, init_logical)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false);
    if let Some(dir) = webview_data_dir() {
        b = b.data_directory(dir);
    }
    b
}

/// Build the settings window once, hidden. Safe to call at startup so the first
/// user open is instant (no webview build on the click). No-op if it exists.
pub fn prebuild_settings(app: &AppHandle, cfg: &WindowConfig) -> Result<(), String> {
    if app.get_webview_window(SETTINGS).is_some() {
        return Ok(());
    }
    let init_logical = 240.0_f64;
    settings_builder(app, init_logical)
        .build()
        .map_err(|e| format!("prebuild settings window: {e}"))?;
    // Position it off-screen-ready; it stays hidden until opened.
    place_settings(app, cfg, Some(init_logical))?;
    Ok(())
}

/// Open (or focus) the settings window: a separate frameless/transparent window
/// positioned on the side opposite the dock edge. The main window is untouched.
pub fn open_settings(app: &AppHandle, cfg: &WindowConfig) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        place_settings(app, cfg, None)?;
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }

    let init_logical = 240.0_f64;
    let win = settings_builder(app, init_logical)
        .build()
        .map_err(|e| format!("build settings window: {e}"))?;

    // Initial placement (frontend will call resize_settings with real content).
    place_settings(app, cfg, Some(init_logical))?;
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

/// Reposition the settings window relative to the main window (used when the
/// widget moves/resizes). No-op if settings isn't open.
pub fn reposition_settings_to_main(app: &AppHandle, cfg: &WindowConfig) -> Result<(), String> {
    place_settings(app, cfg, None)
}

/// Toggle the settings window: hide if visible, show (or build once) if not.
pub fn toggle_settings(app: &AppHandle, cfg: &WindowConfig) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        // Already built — just toggle visibility (instant, no webview rebuild).
        let visible = w.is_visible().unwrap_or(false);
        if visible {
            let _ = w.hide();
        } else {
            place_settings(app, cfg, None)?;
            let _ = w.show();
            let _ = w.set_focus();
        }
        Ok(())
    } else {
        open_settings(app, cfg)
    }
}

/// Hide the settings window if present. We hide (not close/destroy) so the next
/// open is instant — rebuilding the webview is what caused the open lag.
pub fn close_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        w.hide().map_err(|e| format!("hide settings: {e}"))?;
    }
    Ok(())
}

/// Resize + reposition the settings window to fit `content_logical` (the
/// settings content extent along the growth axis) on the side opposite the dock
/// edge. For above/below docks this is a height; for left/right docks a width.
pub fn resize_settings(
    app: &AppHandle,
    cfg: &WindowConfig,
    content_logical: f64,
) -> Result<(), String> {
    place_settings(app, cfg, Some(content_logical))
}

/// Apply the two independent window toggles. `always_on_top` keeps the widget
/// above other windows; `docked` hides it from the taskbar. Actual docked
/// SIZE/PLACEMENT is applied separately via `apply_dock_placement` (driven by
/// the frontend once it knows the tile layout). Any combination is valid.
pub fn apply_props(
    window: &WebviewWindow,
    always_on_top: bool,
    docked: bool,
) -> Result<(), String> {
    window
        .set_always_on_top(always_on_top)
        .map_err(|e| format!("set always on top: {e}"))?;
    // Docked windows skip the taskbar; undocked ones show in it.
    window
        .set_skip_taskbar(docked)
        .map_err(|e| format!("set skip taskbar: {e}"))?;
    Ok(())
}

/// Set the window to an explicit logical size and re-pin it to the bottom of
/// the monitor. Used in dock mode to make the window exactly one tile tall and
/// wide enough for the current tile count. Width is capped to the monitor.
/// Resolve the target monitor: the preferred index if valid, else the window's
/// current monitor, else the first available. Returns (position, size, scale).
fn resolve_monitor(
    window: &WebviewWindow,
    preferred: Option<usize>,
) -> Option<(PhysicalPosition<i32>, tauri::PhysicalSize<u32>, f64)> {
    let monitors = window.available_monitors().ok()?;
    let chosen = preferred
        .and_then(|i| monitors.get(i))
        .cloned()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| monitors.first().cloned())?;
    Some((*chosen.position(), *chosen.size(), chosen.scale_factor()))
}

/// Size and place the docked window on the chosen monitor edge, honoring
/// orientation, side, alignment/corner. `logical_width`/`logical_height` are the
/// content size the frontend computed.
pub fn apply_dock_placement(
    window: &WebviewWindow,
    cfg: &WindowConfig,
    logical_width: f64,
    logical_height: f64,
) -> Result<(), String> {
    let (mon_pos, mon_size, scale) = match resolve_monitor(window, cfg.dock_monitor) {
        Some(m) => m,
        None => return Ok(()),
    };
    let mon_w = mon_size.width as i32;
    let mon_h = mon_size.height as i32;

    // Window size in physical px, clamped to the monitor.
    let w_phys = ((logical_width * scale).round() as i32).clamp(1, mon_w);
    let h_phys = ((logical_height * scale).round() as i32).clamp(1, mon_h);

    window
        .set_size(tauri::PhysicalSize::new(w_phys as u32, h_phys as u32))
        .map_err(|e| format!("set size: {e}"))?;

    // Small gap from the screen edge (e.g. above the taskbar).
    let margin = (12.0 * scale) as i32;
    // Extra clearance for the bottom edge so a docked widget sits above the
    // Windows taskbar (Tauri's Monitor doesn't expose the work area, so we use
    // a sensible default taskbar height). ~48px logical ≈ Win11 taskbar.
    let taskbar_clearance = (48.0 * scale) as i32;

    // Compute top-left based on side + alignment.
    let (x, y) = match cfg.dock_side {
        DockSide::Bottom | DockSide::Top => {
            // Horizontal edges: X varies by alignment, Y pinned to edge.
            let x = match cfg.dock_align {
                DockAlign::Centered => mon_pos.x + (mon_w - w_phys) / 2,
                DockAlign::Cornered => match cfg.dock_corner {
                    DockCorner::Start => mon_pos.x + margin, // left
                    DockCorner::End => mon_pos.x + mon_w - w_phys - margin, // right
                },
            };
            let y = match cfg.dock_side {
                DockSide::Top => mon_pos.y + margin,
                _ => mon_pos.y + mon_h - h_phys - taskbar_clearance, // bottom: clear taskbar
            };
            (x, y)
        }
        DockSide::Left | DockSide::Right => {
            // Vertical edges: Y varies by alignment, X pinned to edge.
            let y = match cfg.dock_align {
                DockAlign::Centered => mon_pos.y + (mon_h - h_phys) / 2,
                DockAlign::Cornered => match cfg.dock_corner {
                    DockCorner::Start => mon_pos.y + margin, // top
                    DockCorner::End => mon_pos.y + mon_h - h_phys - taskbar_clearance, // bottom: clear taskbar
                },
            };
            let x = match cfg.dock_side {
                DockSide::Left => mon_pos.x + margin,
                _ => mon_pos.x + mon_w - w_phys - margin, // right
            };
            (x, y)
        }
    };

    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("set position: {e}"))?;
    Ok(())
}

/// Toggle click-through (mouse events pass to windows beneath). Useful for a
/// non-interactive overlay. The frontend can re-enable interactivity on hover.
pub fn set_click_through(window: &WebviewWindow, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| format!("set ignore cursor events: {e}"))
}

/// Reposition the overlay to an explicit logical position.
pub fn move_to(window: &WebviewWindow, x: f64, y: f64) -> Result<(), String> {
    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| format!("move: {e}"))
}

/// Convenience: fetch the main window from any Manager handle.
pub fn main_window<M: Manager<tauri::Wry>>(app: &M) -> Result<WebviewWindow, String> {
    app.get_webview_window(MAIN)
        .ok_or_else(|| "main window not found".to_string())
}

/// Drag the docked window along its edge only: for horizontal docks (top/
/// bottom) the free axis is X (pinned Y to the edge); for vertical docks
/// (left/right) the free axis is Y (pinned X to the edge). `screen_x`/`screen_y`
/// are the desired window top-left in physical screen pixels; the pinned axis
/// value is recomputed and the free axis is clamped to the monitor.
pub fn drag_along_edge(
    window: &WebviewWindow,
    cfg: &WindowConfig,
    screen_x: i32,
    screen_y: i32,
) -> Result<(), String> {
    let (mon_pos, mon_size, scale) = match resolve_monitor(window, cfg.dock_monitor) {
        Some(m) => m,
        None => return Ok(()),
    };
    let win_size = window.outer_size().map_err(|e| format!("outer size: {e}"))?;
    let margin = (12.0 * scale) as i32;
    let taskbar_clearance = (48.0 * scale) as i32;
    let mon_w = mon_size.width as i32;
    let mon_h = mon_size.height as i32;
    let w = win_size.width as i32;
    let h = win_size.height as i32;

    let (x, y) = match cfg.dock_side {
        DockSide::Bottom => {
            let min_x = mon_pos.x;
            let max_x = (mon_pos.x + mon_w - w).max(min_x);
            (screen_x.clamp(min_x, max_x), mon_pos.y + mon_h - h - taskbar_clearance)
        }
        DockSide::Top => {
            let min_x = mon_pos.x;
            let max_x = (mon_pos.x + mon_w - w).max(min_x);
            (screen_x.clamp(min_x, max_x), mon_pos.y + margin)
        }
        DockSide::Left => {
            let min_y = mon_pos.y;
            let max_y = (mon_pos.y + mon_h - h).max(min_y);
            (mon_pos.x + margin, screen_y.clamp(min_y, max_y))
        }
        DockSide::Right => {
            let min_y = mon_pos.y;
            let max_y = (mon_pos.y + mon_h - h).max(min_y);
            (mon_pos.x + mon_w - w - margin, screen_y.clamp(min_y, max_y))
        }
    };

    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("set position: {e}"))
}

/// Resize the window to `new_logical_height` while keeping its BOTTOM edge in
/// the same screen position (i.e. the window grows/shrinks upward). Used so the
/// settings panel can expand above the toolbar without moving the toolbar.
pub fn set_height_anchored_bottom(
    window: &WebviewWindow,
    new_logical_height: f64,
) -> Result<(), String> {
    let scale = window
        .scale_factor()
        .map_err(|e| format!("scale factor: {e}"))?;

    // Current physical geometry.
    let pos = window
        .outer_position()
        .map_err(|e| format!("outer position: {e}"))?;
    let size = window.outer_size().map_err(|e| format!("outer size: {e}"))?;

    // The bottom edge we want to keep fixed (physical pixels).
    let bottom = pos.y + size.height as i32;

    // New height in physical pixels.
    let new_h_phys = (new_logical_height * scale).round() as i32;

    // New top so the bottom stays put.
    let new_y = bottom - new_h_phys;

    // Apply size first, then position (order avoids a transient visual jump on
    // some platforms).
    window
        .set_size(tauri::PhysicalSize::new(size.width, new_h_phys as u32))
        .map_err(|e| format!("set size: {e}"))?;
    window
        .set_position(PhysicalPosition::new(pos.x, new_y))
        .map_err(|e| format!("set position: {e}"))?;
    Ok(())
}
