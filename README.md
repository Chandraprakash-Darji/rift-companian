# Rift App Indicator

Standalone macOS menu-bar companion for Rift. It displays application icons
grouped by workspace and updates through Rift's Mach IPC events.

The app uses Rift's public IPC API through the `rift-client` git dependency.
The menu opens with workspace names and applications; the menu-bar row shows
each non-empty workspace number immediately before its application icons.

## Run

Run from a terminal while Rift is running:

```sh
cargo run --release
```

## Run as a background service

To keep it running in the background without a terminal, install it as a
user LaunchAgent:

```sh
rift-app-indicator service install
rift-app-indicator service start
```

The agent starts at login, is kept alive by launchd, and logs to
`/tmp/rift_app_indicator_<user>.{out,err}.log`.

Management commands:

```sh
rift-app-indicator service restart
rift-app-indicator service stop
rift-app-indicator service uninstall
```