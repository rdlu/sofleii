# sofleii

**Sofle Interactive Interface**: a command-line tool (a ratatui TUI comes
later) that talks to the Eyelash Sofle over the **ZMK Studio RPC** protocol,
the same protocol the Studio web app uses, without a browser. Design and
roadmap: [#44](https://github.com/rdlu/sofleii/issues/44).

Status: **spike.** USB serial only (left half); read-only commands.

## Build and run

Needs Rust (edition 2024, 1.85+) and `protoc`, both from pacman
(`rust`, `protobuf`). From the repo root:

```bash
mise run sofleii -- status          # build (release) and run
mise run sofleii -- profiles --json
mise run sofleii-test               # cargo test + clippy -D warnings + fmt --check
```

Or directly: `cd sofleii && cargo run -- info`.

The keyboard is found by its USB product string `Eyelash Sofle` (the VID/PID
is shared by every ZMK board, and the `/dev/ttyACM*` number is not stable).
`--device /dev/ttyACMn` overrides the lookup. Your user needs access to the
port (the `uucp` group on Arch).

## Commands

| Command | What it reads |
|---|---|
| `sofleii status [--json] [--format waybar]` | One-shot summary: transport, device, Studio lock, active BLE profile (connected, bonded, address, name), output priority. Built from `get_device_info`, `get_lock_state` and `cormoran_ble` `get_profiles` + `get_output_priority` (`get_profiles` doesn't carry the priority). `split` and `battery` are `null` for now, see below. |
| `sofleii info [--json]` | `core.get_device_info` (name, serial number), Studio lock state, `custom.list_custom_subsystems`. Build info is `null` until the firmware exposes it (#46). |
| `sofleii profiles [--json]` | `cormoran_ble.get_profiles` and `get_output_priority` (the subsystem index is looked up by identifier, never hardcoded). |

Global options: `--device <path>`, `--transport usb|ble` (BLE not implemented
yet), `--timeout <ms>` per response (default 3000), `--format
human|json|waybar` (default `human`; `--json` is the same as `--format json`;
`waybar` is `status` only and is a usage error elsewhere). With no subcommand,
sofleii prints usage.

```
$ sofleii status
Eyelash Sofle
transport       usb /dev/ttyACM1
output          usb (priority usb)
ble profile     0 connected 84:9E:56:52:6A:D2
bonded slots    1 of 5
studio          unlocked
split link      unknown (not readable yet)
battery         left -, right - (not read yet; `sofleii battery` is next)
```

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
  "battery": { "left": null, "right": null,
               "note": "unavailable: not read yet; the firmware exposes both levels since #45, read over BLE GATT by the upcoming `sofleii battery`" }
}
```

- `split` is always `null`: `cormoran_ble`'s `get_split_info` is broken
  upstream (always empty, `peripheral_connected` hardcoded false), so sofleii
  doesn't call it. `split_note` says why; both stay in the schema.
- `battery.left` / `battery.right` are `null` for now. Since #45 the firmware
  exposes both levels as two BLE Battery Services (the right half's carries
  the description "Peripheral 0"); BlueZ's `Battery1` only shows the first,
  so the upcoming `sofleii battery` reads them over GATT. They will become `{"percent": int|null}`
  objects, an additive change that needs no schema bump; `note` then turns
  `null`.
- Without a `cormoran_ble` subsystem, the profile fields and
  `output_priority` are `null` rather than an error.

On failure with `--json`, stdout carries an error object instead:

```json
{ "schema": 1, "error": { "kind": "timeout", "message": "…", "exit_code": 4 } }
```

`kind` is one of `not_found`, `usage`, `io`, `timeout`, `protocol`, `rpc`.

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
| 1 | Check failed / attention needed (reserved; no command uses it yet) |
| 2 | Keyboard not found / not connected (including a `--device` path that does not exist) |
| 3 | Usage error (bad flag, unimplemented transport) |
| 4 | Transport or protocol error: timeout, bad frame, RPC error |

## Layout

```
src/
  proto.rs       prost-generated messages (build.rs, from proto/)
  framing.rs     SOF 0xAB / ESC 0xAC / EOF 0xAD, streaming decoder
  transport/     Transport trait; serial.rs (USB, discovery); ble.rs (stub); mock.rs (tests)
  client/        request ids, response matching, notifications set aside; rpc.rs = pure request builders
  commands/      status, info, profiles (status reuses profiles::fetch): plain data, never print, no UI code
  output.rs      human, JSON and waybar rendering
  main.rs        clap CLI and exit codes
proto/           vendored .proto files (see proto/README.md for sources and licenses)
tests/golden.rs  request bytes captured from the v3.0.0 firmware
tests/cli.rs     exit codes and error output with no keyboard (not found, usage)
```

License: MIT (see the repository's `LICENSE`). The vendored protos keep
their own MIT licenses, listed in `proto/README.md`.
