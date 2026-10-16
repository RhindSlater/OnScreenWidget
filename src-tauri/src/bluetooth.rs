//! Bluetooth / wireless device battery reader.
//!
//! Reads the battery level Windows exposes for connected PnP/Bluetooth devices
//! via `DEVPKEY_Bluetooth_Battery` (`{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2`)
//! plus the connection state (`{83DA6326-97A6-4088-9453-A1923F573B29} 15`) and
//! the device name (`DEVPKEY_NAME`). This is the same data the Windows Settings
//! battery indicator uses.
//!
//! Implementation: direct **SetupAPI** calls (`SetupDiGetClassDevs` +
//! `SetupDiGetDeviceProperty`) via the `windows` crate — no PowerShell process
//! spawn. This is dramatically lighter (microseconds + ~0 RAM vs a ~40MB
//! powershell.exe per poll) and fast.
//!
//! A physical device often spans multiple PnP nodes (battery on one, connection
//! on another), so we group nodes by a normalized name and combine them.
//!
//! Not yet implemented: Logitech HID++ over a Bolt/Unifying/Lightspeed dongle
//! (not a Bluetooth PnP device), DualSense/DualShock HID, and charging state.
//! On non-Windows targets this returns an empty list.

/// A device battery reading.
#[derive(Debug, Clone)]
pub struct BtDevice {
    /// True when `charging` comes from a real signal, so the battery-trend
    /// heuristic must not override it (e.g. keyboard wired vs. on its dongle).
    pub charging_exact: bool,
    pub name: String,
    /// Battery percentage 0–100, if known.
    pub battery: Option<u8>,
    /// Charging state (not exposed by the PnP property; always false here).
    pub charging: bool,
    /// Whether Windows currently considers the device connected.
    pub connected: bool,
    /// True when `battery` is a last-known value, not a fresh read this cycle.
    pub stale: bool,
}

/// Serializable device info for the editor dropdown.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BtDeviceInfo {
    pub name: String,
    pub battery: Option<u8>,
    pub connected: bool,
}

/// Read all currently-connected devices Windows reports a battery level for.
/// Also infers `charging` by comparing each device's battery against its last
/// reading: a rising level (while connected) means it's charging. This is a
/// heuristic — Windows doesn't expose charging state for these devices — so it
/// turns on the poll after the % ticks up and clears when the % drops.
pub fn read_devices() -> Vec<BtDevice> {
    #[cfg(target_os = "windows")]
    let mut devices = win::read_devices(true);
    #[cfg(not(target_os = "windows"))]
    let mut devices: Vec<BtDevice> = Vec::new();
    devices.extend(usb_receiver::read());
    // Real charging state from Logitech HID++ overrides the trend heuristic.
    for (name, soc, charging) in usb_receiver::logitech() {
        if let Some(d) = devices.iter_mut().find(|d| d.name.to_lowercase() == name) {
            d.charging = charging;
            d.charging_exact = true;
            if d.battery.is_none() {
                d.battery = soc;
            }
        }
    }

    infer_charging(&mut devices);
    devices
}

/// 2.4 GHz USB-receiver keyboards (VTER/Galaxy series). Not Bluetooth, so
/// Windows exposes no battery; the vendor app asks the receiver over a vendor
/// HID interface (usage page 0xFF60, MI_03) and we replicate that request.
///
/// Reverse-engineered from VTER DeviceDriver.exe: it writes a 33-byte report
/// `00 20 01 00… <sum>` (cmd 0x20 0x01 + additive checksum) and calls
/// `BatteryCtrl::SetBatteryInfo(reply[3], …)`. Reply: `20 01 00 <battery%> …
/// <checksum at byte 32>`. The second SetBatteryInfo argument (charging) was 0
/// in every sample, so charging stays heuristic.
mod usb_receiver {
    use super::BtDevice;
    use std::sync::{Mutex, OnceLock};

    /// (vid, pid, interface, display name) from the vendor app's config.xml.
    const RECEIVERS: &[(u16, u16, i32, &str)] = &[(0x05AC, 0x024F, 3, "Galaxy80 2.4G")];
    const USAGE_PAGE: u16 = 0xFF60;

    fn api() -> &'static Mutex<Option<hidapi::HidApi>> {
        static A: OnceLock<Mutex<Option<hidapi::HidApi>>> = OnceLock::new();
        A.get_or_init(|| Mutex::new(None))
    }

    pub fn read() -> Vec<BtDevice> {
        let mut guard = api().lock().unwrap();
        if guard.is_none() {
            *guard = hidapi::HidApi::new().ok();
        }
        let Some(hid) = guard.as_mut() else { return Vec::new() };
        if hid.refresh_devices().is_err() {
            return Vec::new();
        }

        let mut out = Vec::new();
        for &(vid, pid, iface, name) in RECEIVERS {
            let Some(info) = hid.device_list().find(|d| {
                d.vendor_id() == vid
                    && d.product_id() == pid
                    && d.interface_number() == iface
                    && d.usage_page() == USAGE_PAGE
            }) else {
                continue;
            };
            let battery = hid.open_path(info.path()).ok().and_then(|dev| query(&dev));
            // On the 2.4G dongle the product is "Galaxy80-2.4g"; a USB cable
            // makes it enumerate as plain "Galaxy80" (what the vendor app calls
            // USB mode) → plugged in → charging.
            let wired = hid.device_list().any(|d| {
                d.vendor_id() == vid
                    && d.product_id() == pid
                    && d.product_string()
                        .map_or(false, |p| !p.to_ascii_lowercase().contains("2.4g"))
            });
            out.push(BtDevice {
                charging_exact: true,
                name: name.to_string(),
                battery,
                charging: wired,
                connected: true,
                stale: false,
            });
        }
        out
    }

    /// Logitech HID++ 2.0 over a Bluetooth LE mouse/keyboard (vendor collection,
    /// usage page 0xFF43). Windows exposes no charging flag for these, so ask
    /// the UNIFIED_BATTERY feature (0x1004) directly: read-only requests.
    /// Returns (lowercase name, battery %, charging).
    pub fn logitech() -> Vec<(String, Option<u8>, bool)> {
        let mut guard = api().lock().unwrap();
        if guard.is_none() {
            *guard = hidapi::HidApi::new().ok();
        }
        let Some(hid) = guard.as_mut() else { return Vec::new() };
        if hid.refresh_devices().is_err() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for info in hid
            .device_list()
            .filter(|d| d.vendor_id() == 0x046D && d.usage_page() == 0xFF43)
        {
            let Some(name) = info.product_string() else { continue };
            let Ok(dev) = hid.open_path(info.path()) else { continue };
            if let Some((soc, charging)) = hidpp_battery(&dev) {
                out.push((name.replace('_', " ").to_lowercase(), soc, charging));
            }
        }
        out
    }

    /// One HID++ 2.0 request/response on the direct (BLE) device index 0xFF.
    fn hidpp(dev: &hidapi::HidDevice, feat_idx: u8, func: u8, params: &[u8]) -> Option<[u8; 20]> {
        let mut m = [0u8; 20];
        m[0] = 0x11; // long report
        m[1] = 0xFF;
        m[2] = feat_idx;
        m[3] = (func << 4) | 0x05; // software id
        m[4..4 + params.len()].copy_from_slice(params);
        dev.write(&m).ok()?;

        let mut buf = [0u8; 20];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
        while std::time::Instant::now() < deadline {
            let n = dev.read_timeout(&mut buf, 100).ok()?;
            if n < 7 || buf[0] != 0x11 {
                continue;
            }
            if buf[2] == 0xFF {
                return None; // HID++ error response
            }
            if buf[2] == feat_idx && buf[3] >> 4 == func {
                return Some(buf);
            }
        }
        None
    }

    fn hidpp_battery(dev: &hidapi::HidDevice) -> Option<(Option<u8>, bool)> {
        // Root feature (index 0), function 0: look up UNIFIED_BATTERY (0x1004).
        let idx = hidpp(dev, 0x00, 0, &[0x10, 0x04])?[4];
        if idx == 0 {
            return None; // not supported by this device
        }
        // getStatus: [soc %][level][status][external power]
        let r = hidpp(dev, idx, 1, &[])?;
        let soc = (r[4] <= 100).then_some(r[4]);
        // status: 1 charging, 2 charging slow, 3 full; plugged in when ext power set.
        let charging = matches!(r[6], 1 | 2) || (r[6] == 3 && r[7] != 0);
        Some((soc, charging))
    }

    fn query(dev: &hidapi::HidDevice) -> Option<u8> {
        let mut req = [0u8; 33]; // [report id 0][31 payload bytes][checksum]
        req[1] = 0x20;
        req[2] = 0x01;
        req[32] = req[1..32].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        dev.write(&req).ok()?;

        let mut buf = [0u8; 64];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1200);
        while std::time::Instant::now() < deadline {
            let n = dev.read_timeout(&mut buf, 100).ok()?;
            if n < 4 || buf[0] != 0x20 || buf[1] != 0x01 {
                continue; // unrelated report; keep waiting for our reply
            }
            if n >= 33 {
                let sum = buf[..32].iter().fold(0u8, |a, b| a.wrapping_add(*b));
                if sum != buf[32] {
                    continue;
                }
            }
            return (buf[3] <= 100).then_some(buf[3]);
        }
        None
    }
}

/// Per-device (last battery, charging) state, to detect a rising trend, hold
/// the charging indicator across steady readings, and provide a last-known
/// battery value when a fresh read isn't available yet (shown faded/stale).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn charge_history() -> &'static std::sync::Mutex<std::collections::HashMap<String, (u8, bool)>> {
    use std::sync::OnceLock;
    static H: OnceLock<std::sync::Mutex<std::collections::HashMap<String, (u8, bool)>>> =
        OnceLock::new();
    H.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Post-process readings: infer charging from the battery trend, and fill a
/// last-known (stale) battery for connected devices whose fresh read is empty.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn infer_charging(devices: &mut [BtDevice]) {
    let mut hist = charge_history().lock().unwrap();
    for d in devices.iter_mut() {
        // Stale fill: connected but no fresh battery → reuse last-known, faded.
        if d.battery.is_none() {
            if let Some(&(prev, was)) = hist.get(&d.name) {
                d.battery = Some(prev);
                d.stale = true;
                if !d.charging && !d.charging_exact {
                    d.charging = was; // keep last charging state while stale
                }
            }
            continue; // nothing new to record this cycle
        }
        // Exact charging signal (e.g. wired keyboard): record it, skip the trend.
        if d.charging_exact {
            if let Some(cur) = d.battery {
                hist.insert(d.name.clone(), (cur, d.charging));
            }
            continue;
        }
        // USB presence already proves charging — don't let the %-trend override it.
        if d.charging {
            if let Some(cur) = d.battery {
                hist.insert(d.name.clone(), (cur, true));
            }
            continue;
        }
        if let Some(cur) = d.battery {
            let charging = match hist.get(&d.name) {
                Some(&(prev, was)) => {
                    if cur > prev {
                        true
                    } else if cur < prev {
                        false
                    } else {
                        was
                    }
                }
                None => false,
            };
            d.charging = charging;
            hist.insert(d.name.clone(), (cur, charging));
        }
    }
}

/// Read ALL battery-capable devices (connected or not) — for the editor
/// dropdown, so a device can be picked even while currently disconnected.
pub fn read_devices_all() -> Vec<BtDevice> {
    #[cfg(target_os = "windows")]
    {
        let mut all = win::read_devices(false);
        all.extend(usb_receiver::read());
        all
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Start an event-driven watcher for Bluetooth connect/disconnect changes.
/// `on_change` is invoked (from a background thread) whenever a device is
/// added/updated/removed, so the caller can immediately re-read + emit instead
/// of polling. No-op on non-Windows. The watcher runs for the process lifetime.
pub fn start_watcher<F: Fn() + Send + Sync + 'static>(on_change: F) {
    #[cfg(target_os = "windows")]
    {
        win::start_watcher(on_change);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = on_change;
    }
}

/// Normalize a PnP node name to a device-family key by stripping common
/// Bluetooth profile/service descriptors, so the battery node and the
/// connection node of one physical device group together. Shared (also used by
/// tests) and OS-independent.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn normalize_device_name(name: &str) -> String {
    let mut n = name.to_lowercase();
    // Unwrap "headset (wh-1000xm5 hands-free)" → "wh-1000xm5 hands-free".
    if let Some(rest) = n.strip_prefix("headset (") {
        n = rest.trim_end_matches(')').to_string();
    }
    const SUFFIXES: &[&str] = &[
        " hands-free ag",
        " hands-free hf",
        " hands-free",
        " stereo",
        " avrcp transport",
        " avrcp",
        " le",
        " headset",
    ];
    let mut changed = true;
    while changed {
        changed = false;
        let trimmed = n.trim();
        for s in SUFFIXES {
            if let Some(stripped) = trimmed.strip_suffix(s) {
                n = stripped.trim().to_string();
                changed = true;
                break;
            }
        }
    }
    n.trim().to_string()
}

/// Combine raw (name, battery, connected, usb) node rows into per-device
/// readings. A device is reported when any of its nodes is connected; the
/// battery is taken from whichever node has one; the shortest node name is the
/// label; `charging` is set when any node is enumerated over USB (plugged in).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn combine_nodes(rows: Vec<(String, Option<u8>, bool, bool)>, only_connected: bool) -> Vec<BtDevice> {
    use std::collections::HashMap;
    struct Agg {
        display: String,
        battery: Option<u8>,
        connected: bool,
        usb: bool,
    }
    let mut groups: HashMap<String, Agg> = HashMap::new();
    for (name, battery, connected, usb) in rows {
        if name.is_empty() {
            continue;
        }
        let key = normalize_device_name(&name);
        if key.is_empty() {
            continue;
        }
        let entry = groups.entry(key).or_insert_with(|| Agg {
            display: name.clone(),
            battery: None,
            connected: false,
            usb: false,
        });
        if name.len() < entry.display.len() {
            entry.display = name.clone();
        }
        if battery.is_some() {
            entry.battery = battery;
        }
        if connected {
            entry.connected = true;
        }
        if usb {
            entry.usb = true;
        }
    }
    groups
        .into_values()
        .filter(|g| !only_connected || g.connected)
        .map(|g| BtDevice {
            charging_exact: false,
            name: g.display,
            battery: g.battery,
            charging: g.usb,
            connected: g.connected,
            stale: false,
        })
        .collect()
}

#[cfg(target_os = "windows")]
mod win {
    use super::{combine_nodes, BtDevice};
    use windows::core::GUID;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceInstanceIdW,
        SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA,
    };
    use windows::Win32::Foundation::DEVPROPKEY;

    // DEVPKEY_Bluetooth_Battery {104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2
    const KEY_BATTERY: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5),
        pid: 2,
    };
    // DEVPKEY_Device_IsConnected {83DA6326-97A6-4088-9453-A1923F573B29} 15
    const KEY_CONNECTED: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x83DA6326_97A6_4088_9453_A1923F573B29),
        pid: 15,
    };
    // PKEY_Devices_Connected (container-level) {78c34fc8-104a-4aca-9ea4-524d52996e57} 55
    // Reliable per-physical-device connection flag (what Windows Settings uses);
    // present on some nodes where the device-instance flag above is not.
    const KEY_CONNECTED_CONTAINER: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57),
        pid: 55,
    };
    // DEVPKEY_NAME {b725f130-47ef-101a-a5f1-02608c9eebac} 10
    const KEY_NAME: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac),
        pid: 10,
    };
    // DEVPKEY_Device_EnumeratorName {a45c254e-df1c-4efd-8020-67d146a850e0} 24
    // "USB" when the device is attached over a USB cable/receiver → charging.
    const KEY_ENUMERATOR: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 24,
    };

    pub fn read_devices(only_connected: bool) -> Vec<BtDevice> {
        unsafe { enumerate(only_connected) }.unwrap_or_default()
    }

    unsafe fn enumerate(only_connected: bool) -> windows::core::Result<Vec<BtDevice>> {
        // All present devices (DIGCF_PRESENT | DIGCF_ALLCLASSES).
        let hdev = SetupDiGetClassDevsW(
            None,
            windows::core::PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        )?;

        let mut rows: Vec<(String, Option<u8>, bool, bool)> = Vec::new();
        let mut index = 0u32;
        loop {
            let mut info = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(hdev, index, &mut info).is_err() {
                break; // no more devices
            }
            index += 1;

            // Keep battery-capable nodes plus the root node of every Bluetooth
            // device (BTHENUM\DEV_* classic / BTHLE\DEV_* LE), so connected
            // devices that expose no battery to Windows still get a cell ("—").
            let battery = read_u8_prop(hdev, &info, &KEY_BATTERY);
            let has_battery = battery.is_some() || has_prop(hdev, &info, &KEY_BATTERY);
            if !has_battery && !is_bt_root(hdev, &info) {
                continue;
            }
            // Connection: a Bluetooth device's instance-level IsConnected flag
            // can be false on the node that carries the battery (the flag lives
            // on a sibling node). The CONTAINER-level Connected property (pid 55)
            // is shared across all nodes of the physical device, so OR the two.
            let conn_instance =
                read_bool_prop(hdev, &info, &KEY_CONNECTED).unwrap_or(false);
            let conn_container =
                read_bool_prop(hdev, &info, &KEY_CONNECTED_CONTAINER).unwrap_or(false);
            let connected = conn_instance || conn_container;
            let name = read_string_prop(hdev, &info, &KEY_NAME).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // A battery-capable node enumerated via USB means the device is on a
            // cable/receiver → plugged in → charging.
            let usb = read_string_prop(hdev, &info, &KEY_ENUMERATOR)
                .map(|e| e.eq_ignore_ascii_case("usb"))
                .unwrap_or(false);
            rows.push((name, battery, connected, usb));
        }

        let _ = SetupDiDestroyDeviceInfoList(hdev);
        Ok(combine_nodes(rows, only_connected))
    }

    /// True for the root PnP node of a paired Bluetooth device.
    unsafe fn is_bt_root(hdev: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO, info: &SP_DEVINFO_DATA) -> bool {
        let mut buf = [0u16; 200];
        if SetupDiGetDeviceInstanceIdW(hdev, info, Some(&mut buf), None).is_err() {
            return false;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let id = String::from_utf16_lossy(&buf[..end]).to_ascii_uppercase();
        id.starts_with("BTHENUM\\DEV_") || id.starts_with("BTHLE\\DEV_")
    }

    /// Query a property's size to learn whether it exists at all.
    unsafe fn has_prop(hdev: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> bool {
        let mut prop_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
        let mut required: u32 = 0;
        // Calling with no buffer returns the required size (and an error we
        // ignore); a non-zero required size means the property is present.
        let _ = SetupDiGetDevicePropertyW(
            hdev,
            info,
            key,
            &mut prop_type,
            None,
            Some(&mut required),
            0,
        );
        required > 0
    }

    /// Read a BYTE property (battery %).
    unsafe fn read_u8_prop(hdev: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<u8> {
        let mut prop_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
        let mut buf = [0u8; 1];
        let mut required: u32 = 0;
        SetupDiGetDevicePropertyW(
            hdev,
            info,
            key,
            &mut prop_type,
            Some(&mut buf),
            Some(&mut required),
            0,
        )
        .ok()?;
        let v = buf[0];
        if v <= 100 {
            Some(v)
        } else {
            None
        }
    }

    /// Read a BOOLEAN property (IsConnected). DEVPROP_TYPE_BOOLEAN is a single
    /// byte where 0xFF/non-zero = true.
    unsafe fn read_bool_prop(hdev: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<bool> {
        let mut prop_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
        let mut buf = [0u8; 1];
        let mut required: u32 = 0;
        SetupDiGetDevicePropertyW(
            hdev,
            info,
            key,
            &mut prop_type,
            Some(&mut buf),
            Some(&mut required),
            0,
        )
        .ok()?;
        Some(buf[0] != 0)
    }

    /// Read a UTF-16 STRING property (device name).
    unsafe fn read_string_prop(hdev: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
        let mut prop_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
        let mut required: u32 = 0;
        // First call: get required byte count.
        let _ = SetupDiGetDevicePropertyW(hdev, info, key, &mut prop_type, None, Some(&mut required), 0);
        if required == 0 {
            return None;
        }
        let mut buf = vec![0u8; required as usize];
        SetupDiGetDevicePropertyW(
            hdev,
            info,
            key,
            &mut prop_type,
            Some(&mut buf),
            Some(&mut required),
            0,
        )
        .ok()?;
        // Interpret the byte buffer as UTF-16.
        let u16s: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        if u16s.is_empty() {
            None
        } else {
            Some(String::from_utf16_lossy(&u16s))
        }
    }

    /// Start a WinRT DeviceWatcher on the Bluetooth AEP selector, requesting the
    /// `System.Devices.Aep.IsConnected` property, and call `on_change` on every
    /// Added/Updated/Removed event. Runs on a dedicated thread with a message
    /// pump so the (non-Send) COM callbacks keep firing for the app's lifetime.
    pub fn start_watcher<F: Fn() + Send + Sync + 'static>(on_change: F) {
        std::thread::spawn(move || {
            if let Err(e) = run_watcher(on_change) {
                log::warn!("bluetooth DeviceWatcher failed: {e:?}");
            }
        });
    }

    fn run_watcher<F: Fn() + Send + Sync + 'static>(on_change: F) -> windows::core::Result<()> {
        use std::sync::Arc;
        use windows::core::HSTRING;
        use windows::Devices::Enumeration::{
            DeviceInformation, DeviceInformationUpdate, DeviceWatcher,
        };
        use windows::Foundation::TypedEventHandler;
        use windows_collections::IIterable;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG};

        unsafe {
            // Join an MTA so the watcher's async callbacks are serviced.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        // AQS selector for connected Bluetooth + BLE association endpoints.
        let aqs = HSTRING::from(
            "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" \
              OR System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\")",
        );
        // Extra property we care about: connection state.
        let props: Vec<HSTRING> = vec![HSTRING::from("System.Devices.Aep.IsConnected")];
        let props_iter: IIterable<HSTRING> = props.into();

        let watcher: DeviceWatcher =
            DeviceInformation::CreateWatcherAqsFilterAndAdditionalProperties(&aqs, &props_iter)?;

        let cb = Arc::new(on_change);

        let cb_added = cb.clone();
        watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(
            move |_w, _info| {
                cb_added();
                Ok(())
            },
        ))?;
        let cb_upd = cb.clone();
        watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
            move |_w, _upd| {
                cb_upd();
                Ok(())
            },
        ))?;
        let cb_rem = cb.clone();
        watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
            move |_w, _upd| {
                cb_rem();
                Ok(())
            },
        ))?;

        watcher.Start()?;
        // Keep the watcher alive for the process lifetime.
        std::mem::forget(watcher.clone());

        // Pump messages so the STA/MTA callbacks are delivered.
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        let _ = watcher; // keep in scope
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // reads live hardware: cargo test live_devices -- --ignored --nocapture
    fn live_devices() {
        for d in read_devices() {
            println!("{:<30} battery={:?} charging={} connected={}", d.name, d.battery, d.charging, d.connected);
        }
    }

    #[test]
    fn groups_split_battery_and_connection_nodes() {
        // Mirrors real PnP output: battery on the AG node (disconnected flag),
        // connection on the base node, plus a parenthesized headset node.
        let rows = vec![
            ("WH-1000XM5 Hands-Free AG".to_string(), Some(50), false, false),
            ("WH-1000XM5".to_string(), None, true, false),
            ("WH-1000XM5".to_string(), None, false, false),
            ("Headset (WH-1000XM5 Hands-Free)".to_string(), None, true, false),
            ("MX Master 3S".to_string(), None, false, false),
        ];
        let mut out = combine_nodes(rows, true);
        out.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(out.len(), 1, "only the connected headphone should report");
        assert_eq!(out[0].name, "WH-1000XM5");
        assert_eq!(out[0].battery, Some(50));
        assert!(!out[0].charging);
    }

    #[test]
    fn disconnected_only_device_is_excluded() {
        let rows = vec![("MX Master 3S".to_string(), Some(80), false, false)];
        assert!(combine_nodes(rows, true).is_empty());
        // ...but shows up when not filtering by connection (editor dropdown).
        assert_eq!(
            combine_nodes(vec![("MX Master 3S".to_string(), Some(80), false, false)], false).len(),
            1
        );
    }

    #[test]
    fn usb_node_marks_charging() {
        // A device present over USB (cable/receiver) is charging.
        let rows = vec![
            ("MX Master 4".to_string(), Some(90), true, false),
            ("MX Master 4".to_string(), None, true, true), // USB-enumerated node
        ];
        let out = combine_nodes(rows, true);
        assert_eq!(out.len(), 1);
        assert!(out[0].charging, "USB presence should mean charging");
    }
}
