# sofleii

**Sofle Interactive Interface**: a command-line tool (a ratatui TUI comes
later) that talks to the Eyelash Sofle over the **ZMK Studio RPC** protocol,
the same protocol the Studio web app uses, without a browser. Design and
roadmap: [#44](https://github.com/rdlu/sofleii/issues/44).

Status: **spike.** Studio RPC over USB serial only (left half); batteries
over BLE GATT through BlueZ; read-only commands.

## Build and run

Needs Rust (edition 2024, 1.85+) and `protoc`, both from pacman
(`rust`, `protobuf`). From the repo root:

```bash
mise run sofleii -- status          # build (release) and run
mise run sofleii -- profiles --json
mise run sofleii-test               # cargo test + clippy -D warnings + fmt --check
```

Or directly: `cd sofleii && cargo run -- info`.

`battery` talks to BlueZ through `zbus` (pure Rust): no libdbus, no tokio,
nothing extra to install beyond a running `bluetooth.service`.

The keyboard is found by its USB product string `Eyelash Sofle` (the VID/PID
is shared by every ZMK board, and the `/dev/ttyACM*` number is not stable).
`--device /dev/ttyACMn` overrides the lookup. Your user needs access to the
port (the `uucp` group on Arch).

## Commands

| Command | What it reads |
|---|---|
| `sofleii status [--json] [--format waybar]` | One-shot summary: transport, device, Studio lock, active BLE profile (connected, bonded, address, name), output priority, both batteries. Built from `get_device_info`, `get_lock_state` and `cormoran_ble` `get_profiles` + `get_output_priority` (`get_profiles` doesn't carry the priority), plus the `battery` reader with a 1 s timeout. `split` is `null` for now, see below. |
| `sofleii battery [--json] [--format waybar] [--warn-below <pct>] [--critical-below <pct>]` | Both halves' levels over BLE GATT (BlueZ, system D-Bus), not Studio RPC. See [Battery](#battery). |
| `sofleii info [--json]` | `core.get_device_info` (name, serial number), Studio lock state, `custom.list_custom_subsystems`. Build info is `null` until the firmware exposes it (#46). |
| `sofleii profiles [--json]` | `cormoran_ble.get_profiles` and `get_output_priority` (the subsystem index is looked up by identifier, never hardcoded). |

Global options: `--device <path>` (serial port for Studio RPC),
`--bt-address <MAC>` (BLE device for the batteries, instead of the BlueZ
device named `Eyelash Sofle`), `--transport usb|ble` (BLE not implemented
yet), `--timeout <ms>` per response (default 3000), `--format
human|json|waybar` (default `human`; `--json` is the same as `--format json`;
`waybar` is `status` and `battery` only, a usage error elsewhere). With no
subcommand, sofleii prints usage.

```
$ sofleii status
Eyelash Sofle
transport       usb /dev/ttyACM1
output          usb (priority usb)
ble profile     0 connected 84:9E:56:52:6A:D2
bonded slots    1 of 5
studio          unlocked
split link      unknown (not readable yet)
battery         left 100%, right 76%
```

When BLE isn't available, the battery row reads `left -, right -
(unavailable: …)` with the reason, and `status` still exits 0.

`output` is the endpoint keystrokes most likely go to: the output priority,
with ZMK's fallback to the other endpoint when the preferred one isn't
connected (USB counts as connected while sofleii talks over USB).

## JSON

`--json` prints a versioned contract: top-level `"schema": 1`, snake_case
keys, `null` for anything unknown (never omitted). Breaking changes bump
`schema`. Diagnostics always go to stderr; stdout has no colour unless it is a
terminal, and never with `--json` or `NO_COLOR`.

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "max_profiles": 5,
  "active_profile": 0,
  "output_priority": "usb",
  "profiles": [
    { "index": 0, "active": true, "connected": true, "bonded": true,
      "address": "84:9E:56:52:6A:D2", "name": null },
    { "index": 1, "active": false, "connected": false, "bonded": false,
      "address": null, "name": null }
  ]
}
```

`status --json` (same envelope; `profile` has the `profiles` entry shape):

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "device": { "name": "Eyelash Sofle", "serial_number": "94708422E5F2FD3E" },
  "studio": { "locked": false },
  "output_priority": "usb",
  "max_profiles": 5,
  "bonded_profiles": 1,
  "active_profile": 0,
  "profile": { "index": 0, "active": true, "connected": true, "bonded": true,
               "address": "84:9E:56:52:6A:D2", "name": null },
  "split": null,
  "split_note": "unavailable: cormoran_ble get_split_info is broken upstream; …",
  "battery": { "left": { "percent": 100 }, "right": { "percent": 76 },
               "note": null }
}
```

- `split` is always `null`: `cormoran_ble`'s `get_split_info` is broken
  upstream (always empty, `peripheral_connected` hardcoded false), so sofleii
  doesn't call it. `split_note` says why; both stay in the schema.
- `battery` comes from the same reader as `sofleii battery`, with a timeout
  of at most 1 s. When BLE isn't available (keyboard not connected over BLE
  to this host, BlueZ not running, a GATT error), `left`/`right` stay `null`
  and `note` is `"unavailable: <reason>"`; the battery never changes
  `status`'s exit code. `note` may also carry a remark from the reader (see
  below) while the levels are filled.
- Without a `cormoran_ble` subsystem, the profile fields and
  `output_priority` are `null` rather than an error.

On failure with `--json`, stdout carries an error object instead:

```json
{ "schema": 1, "error": { "kind": "timeout", "message": "…", "exit_code": 4 } }
```

`kind` is one of `not_found`, `usage`, `io`, `timeout`, `protocol`, `rpc`,
`bluez`.

## Battery

Battery levels aren't part of Studio RPC. Since #45 the left (central) half
exposes **two BLE Battery Services** (`0x180F`): its own, and a proxy of the
right half's. `sofleii battery` reads both over the system D-Bus from BlueZ:

1. `GetManagedObjects` on `org.bluez`; the keyboard is the connected
   `Device1` whose `Name` or `Alias` is `Eyelash Sofle` (or the one at
   `--bt-address`). If several match, the first wins and stderr says so.
2. Under it, each `0x180F` service's Battery Level (`0x2A19`). The one with
   **no** `0x2901` user description is the left half; the one described
   `Peripheral 0` is the right half (`Peripheral 1` would be a second
   peripheral). If the descriptions don't settle it, the halves go by service
   handle order (first = left) and `note` says so.
3. `ReadValue({})` on each level, falling back to BlueZ's cached `Value` (with
   a note) if the read fails.

Nothing assumes `hci0` or fixed handles. It only reads: no writes, no
`StartNotify`, no pairing changes.

**BlueZ's `org.bluez.Battery1`, and so upower and most desktop battery
widgets, only show the first service: the left half.** That's why sofleii
reads GATT directly. (The Studio RPC service also shows up twice in BlueZ's
tree; that doesn't matter here.)

```
$ sofleii battery
left  100%
right 76%
```

```json
{
  "schema": 1,
  "source": "bluez",
  "device": { "name": "Eyelash Sofle", "address": "DD:1F:23:76:6E:71" },
  "left": { "percent": 100 },
  "right": { "percent": 76 },
  "note": null
}
```

- A half is `{"percent": null}` when it's unknown, e.g. only one Battery
  Service is exposed (firmware before #45, or the right half isn't
  forwarded); `note` then says why. `note` is `null` when there's nothing to
  add. Notes and warnings also go to stderr in human mode.
- **Exit codes:** 0 when read. **1 only when `--warn-below` is given** and a
  known half is below it (the default threshold of 20 only sets the waybar
  class, it never fails the command; an unknown half never triggers 1).
  **2** when the keyboard isn't found over BLE: the message says whether it's
  not connected to this host, not known to BlueZ, or BlueZ/D-Bus isn't
  available. **3** usage. **4** a D-Bus or GATT error (`kind: "bluez"`), e.g.
  neither level could be read.
- `--timeout` bounds each D-Bus call (default 3000 ms).

### waybar

`sofleii battery --format waybar`:

```json
{"text":"󰌌 L 100 R 76","tooltip":"Eyelash Sofle\nleft: 100%\nright: 76%\nsource: BlueZ GATT DD:1F:23:76:6E:71","class":"ok","alt":"ok","percentage":76}
```

- `text`: `󰌌` (nf-md-keyboard, U+F030C), then `L <left> R <right>`, `-` for an
  unknown half. Not found: `󰌐` (nf-md-keyboard_off, U+F0310).
- `class` / `alt`, by the **lower** known half: `critical` below
  `--critical-below` (default 10), `warning` below `--warn-below` (default
  20), else `ok`; `unknown` if neither half could be read; `disconnected`
  when the keyboard isn't found or the read failed.
- `percentage`: the lower half, for `format-icons`.
- Not found **still prints valid JSON** and exits 2, as `status` does. In
  waybar mode `--warn-below` only moves the `warning` threshold and never
  exits 1, so a low battery doesn't look like a failed script to the bar.

```jsonc
"custom/sofleii-battery": {
  "exec": "sofleii battery --format waybar",
  "return-type": "json",
  "interval": 300,
  "format": "{}",
  "tooltip": true
}
```

```css
#custom-sofleii-battery.warning { color: #ebcb8b; }
#custom-sofleii-battery.critical { color: #bf616a; }
#custom-sofleii-battery.disconnected { opacity: 0.5; }
```

For a check in a script: `sofleii battery --warn-below 20 || notify-send
"Sofle battery low"` (exit 1 low, 2 not connected).

## waybar

`sofleii status --format waybar` prints one line of waybar custom-module
JSON:

```json
{"text":"󰕓 0","tooltip":"Eyelash Sofle\ntransport: usb /dev/ttyACM1\noutput: usb (priority usb)\n…","class":"usb","alt":"usb"}
```

- `text`: an output glyph plus the active profile index (0-based, as in
  `profiles` and `&bt BT_SEL n`). The glyphs are Nerd Font Material Design
  icons: `󰕓` nf-md-usb (U+F0553), `󰂯` nf-md-bluetooth (U+F00AF), `󰂲`
  nf-md-bluetooth_off (U+F00B2). To use other icons, format with `{alt}` and
  `format-icons` instead of `{}`.
- `tooltip`: the `status` summary as `label: value` lines.
- `class` and `alt`: `usb`, `ble` or `disconnected`.
- **Errors still print valid JSON** with class `disconnected` (and the error
  in the tooltip), and the exit code still says what happened: 2 when the
  keyboard isn't found, 4 on a transport error. The bar keeps rendering.

```jsonc
"custom/sofleii": {
  "exec": "sofleii status --format waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{}",
  // later: open the TUI
  "on-click": "foot -e sofleii",
  "tooltip": true
}
```

```css
#custom-sofleii.disconnected { opacity: 0.5; }
#custom-sofleii.ble { color: #5e81ac; }
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Check failed / attention needed: `battery --warn-below` with a half below it |
| 2 | Keyboard not found / not connected (including a `--device` path that does not exist; for `battery`, not connected over BLE or BlueZ/D-Bus unavailable) |
| 3 | Usage error (bad flag, unimplemented transport) |
| 4 | Transport or protocol error: timeout, bad frame, RPC error, D-Bus/GATT error |

## Layout

```
src/
  proto.rs       prost-generated messages (build.rs, from proto/)
  framing.rs     SOF 0xAB / ESC 0xAC / EOF 0xAD, streaming decoder
  transport/     Transport trait; serial.rs (USB, discovery); ble.rs (stub); mock.rs (tests)
  client/        request ids, response matching, notifications set aside; rpc.rs = pure request builders
  commands/      status, info, profiles (status reuses profiles::fetch), battery (Gatt trait,
                 half identification; tested with a fake BlueZ tree): plain data, never print, no UI code
  bluez.rs       the Gatt trait over the system D-Bus (zbus blocking API; no tokio, no libdbus)
  output.rs      human, JSON and waybar rendering
  main.rs        clap CLI and exit codes
proto/           vendored .proto files (see proto/README.md for sources and licenses)
tests/golden.rs  request bytes captured from the v3.0.0 firmware
tests/cli.rs     exit codes and error output with no keyboard (not found, usage)
```

License: MIT (see the repository's `LICENSE`). The vendored protos keep
their own MIT licenses, listed in `proto/README.md`.
