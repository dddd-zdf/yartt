//! Read-only DDC/CI probe: lists every monitor, its capabilities string and the
//! current values of the VCP codes yartt cares about. Writes nothing.
//!
//! cargo run --example probe

use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};

const CODES: [(u8, &str); 5] = [
    (0x10, "brightness"),
    (0x12, "contrast"),
    (0x60, "input source"),
    (0x62, "volume"),
    (0xD6, "power mode")
];

unsafe extern "system" fn collect(hm: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    (*(data.0 as *mut Vec<HMONITOR>)).push(hm);
    true.into()
}

fn main() {
    let mut hmonitors: Vec<HMONITOR> = Vec::new();
    unsafe { EnumDisplayMonitors(HDC::default(), None, Some(collect), LPARAM(&mut hmonitors as *mut _ as isize)) };

    for hm in hmonitors {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        unsafe { GetMonitorInfoW(hm, &mut info as *mut _ as *mut MONITORINFO) };
        let gdi = String::from_utf16_lossy(&info.szDevice).trim_end_matches('\0').to_string();

        let mut n = 0;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hm, &mut n) }.is_err() || n == 0 {
            println!("{gdi}: no physical monitors (no DDC/CI)\n");
            continue;
        }
        let mut physical = vec![PHYSICAL_MONITOR::default(); n as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hm, &mut physical) }.is_err() {
            println!("{gdi}: couldn't open physical monitors\n");
            continue;
        }

        for pm in physical {
            // PHYSICAL_MONITOR is packed, so copy the field out before borrowing it
            let desc = pm.szPhysicalMonitorDescription;
            let desc = String::from_utf16_lossy(&desc);
            println!("{gdi}: {}", desc.trim_end_matches('\0'));

            let mut len = 0;
            if unsafe { GetCapabilitiesStringLength(pm.hPhysicalMonitor, &mut len) } != 0 && len > 0 {
                let mut buf = vec![0u8; len as usize];
                if unsafe { CapabilitiesRequestAndCapabilitiesReply(pm.hPhysicalMonitor, &mut buf) } != 0 {
                    println!("  caps: {}", String::from_utf8_lossy(&buf).trim_end_matches('\0'));
                }
            } else {
                println!("  caps: (not reported)");
            }

            for (code, name) in CODES {
                let (mut cur, mut max) = (0u32, 0u32);
                let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(pm.hPhysicalMonitor, code, None, &mut cur, Some(&mut max)) };
                match ok {
                    0 => println!("  0x{code:02X} {name:<13} unsupported / no reply"),
                    _ => println!("  0x{code:02X} {name:<13} {cur} (max {max})")
                }
            }
            println!();
            let _ = unsafe { DestroyPhysicalMonitor(pm.hPhysicalMonitor) };
        }
    }
}
