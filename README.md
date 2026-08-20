# Rift App Indicator

Standalone macOS menu-bar companion for Rift. It displays application icons
grouped by workspace and updates through Rift's Mach IPC events.

Build and run while Rift is running:

```sh
cargo run --release
```

The app uses Rift's public IPC API through the `rift-wm` git dependency. The
menu opens with workspace names and applications; the menu-bar row shows each
non-empty workspace number immediately before its application icons.
