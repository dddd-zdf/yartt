#[allow(dead_code)]
const APP_ICON: u16 = 1;

/// Sizes the tray icon is drawn at, see assets/icons/render.py.
#[allow(dead_code)]
const TRAY_SIZES: [u32; 4] = [16, 20, 24, 32];

/// Resource id of the tray icon at `TRAY_SIZES[index]`; light is the dark glyph for a light taskbar.
#[allow(dead_code)]
const fn tray_icon_id(dark: bool, index: usize) -> u16 {
    (if dark { 20 } else { 10 }) + index as u16
}
