# Changelog

YARTT (Yet Another Rusty Twinkle Tray), forked from
[sidit77/rusty-twinkle-tray](https://github.com/sidit77/rusty-twinkle-tray) on 2026-10-24.

## Unreleased

- Renamed to YARTT: the exe is `yartt.exe` and settings live in `%APPDATA%\yartt.ini`. The old
  `rusty-twinkle-tray.ini` is moved there on first start. The autostart entry is now `_YARTT`.
  Difei: if you had autostart on for an older build, turn it off and on again in Settings.
- The tray icon's tooltip shows the current brightness, one line per monitor or one line while
  linked, so scrolling over the icon shows the new level.
- The settings window scrolls when it's short and wraps its text when it's narrow.
- New README.
- Flyout rows for volume and input under each monitor, shown only when the monitor supports
  them. The input list comes from the monitor's capabilities string, read once per monitor.
  While linked, each monitor still shows its own volume and input below the shared slider.
- Both rows are off by default: turn them on in Settings > Controls ("Show a volume slider…",
  "Show input selection…", saved as `ShowVolume` / `ShowInput`). While off, the app doesn't read
  them from the monitors at all. Difei: turn them on if you want them.
- Screen off button in the bottom bar: turns every display off the way Windows does when idle
  (any mouse or key input wakes them).
- Volume and input writes are skipped when the monitor already has that value.
- Linked monitors: a Link button in the flyout's bottom bar. While linked, the flyout shows one
  "All monitors" slider in place of the per-monitor ones, and tray scroll and hotkeys move that
  slider. Turning it on brings every monitor to the first one's level. Saved as
  `LinkedBrightness` in the ini. Nobody needs to act.
- Restore timing from Twinkle Tray: after waking from sleep, brightness is reapplied after 8 s
  (it was right away, often before the monitors answered). After a display change it waits
  5 s, and a burst of changes becomes one refresh.

- Backend can read and write monitor volume, input source and power mode (raw DDC/CI VCP
  codes 0x62, 0x60, 0xD6). Each monitor's values are read once when it's first found. Not shown
  in the flyout yet. Nobody needs to act.
- A setting the monitor doesn't support is no longer retried, which saved about 3 s per
  missing setting.
- `examples/probe.rs`: read-only dump of each monitor's DDC/CI capabilities and values
  (`cargo run --example probe`).
- The upstream `caps` test, which drops every monitor to minimum brightness, now only runs when
  asked for by name.
