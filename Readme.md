# YARTT

**Yet Another Rusty Twinkle Tray.** A small Windows tray app for controlling external monitors
over DDC/CI: brightness, linked brightness across monitors, and optionally volume and input.

It's a fork of [Rusty Twinkle Tray](https://github.com/sidit77/rusty-twinkle-tray), a Rust
rewrite of [Twinkle Tray](https://github.com/xanderfrangos/twinkle-tray). It uses the native
Windows controls instead of Electron, so it's a small single exe that stays near idle when
you're not using it.

## Features

- **Brightness** per monitor from the tray flyout, the mouse wheel on the tray icon, or hotkeys.
  The tray tooltip shows the current level.
- **Linked monitors**: one "All monitors" slider that moves every screen together.
- **Restore**: brings back your saved brightness at startup, after waking from sleep (after 8 s)
  and after display changes (after 5 s). Many monitors forget DDC/CI settings when they lose power.
  The delays come from Twinkle Tray.
- **Screen off**: turns every display off the way Windows does when idle. Any input wakes them.
- **Optional, off by default** (Settings > Controls):
  - a volume slider for monitors with speakers
  - an input dropdown, built from the inputs the monitor lists itself

While an option is off, the app doesn't talk to the monitors about it at all.

## Install

Build it (below) and run `target\release\yartt.exe`. Turn on "Automatically run on startup" in
Settings to start it with Windows.

Settings live in `%APPDATA%\yartt.ini`. An existing `rusty-twinkle-tray.ini` is moved there on
first start.

## Building

Needs [Rust](https://www.rust-lang.org/).

```shell
cargo build --release
```

`cargo run --example probe` prints each monitor's DDC/CI capabilities and current values without
changing anything. It's handy for checking what a new monitor supports.

### UI bindings

The flyout and settings window use the UWP XAML controls in a XAML island. `windows-rs` has no
bindings for `Windows.UI.Xaml`, so `lib/windows-ext` holds bindings generated with
`windows-bindgen`. `lib/codegen` trims them to the types and methods listed in
`lib/windows-ext/Codegen.toml`. To use a new control, add it there and regenerate:

```shell
cargo run --release --package codegen lib/windows-ext/
```

## Credits and license

MIT, like the projects it builds on. Rusty Twinkle Tray is by sidit77, and Twinkle Tray, whose
restore timings this follows, is by Xander Frangos. See [License.md](License.md) and
[CHANGELOG.md](CHANGELOG.md).
