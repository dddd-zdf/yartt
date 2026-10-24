# Changelog

YARTT (Yet Another Rusty Twinkle Tray), forked from
[sidit77/rusty-twinkle-tray](https://github.com/sidit77/rusty-twinkle-tray) on 2026-10-24.

## Unreleased

- Linked monitors: a Link button in the flyout's bottom bar. While linked, moving one slider
  moves the others to the same level, and tray scroll and hotkeys step all monitors from one
  shared level. Turning it on brings every monitor to the first one's level. Saved as
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
