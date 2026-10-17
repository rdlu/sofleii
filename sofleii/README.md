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
mise run sofleii -- info            # build (release) and run
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
| `sofleii info [--json]` | `core.get_device_info` (name, serial number), Studio lock state, `custom.list_custom_subsystems`. Build info is `null` until the firmware exposes it (#46). |
| `sofleii profiles [--json]` | `cormoran_ble.get_profiles` and `get_output_priority` (the subsystem index is looked up by identifier, never hardcoded). |

Global options: `--device <path>`, `--transport usb|ble` (BLE not implemented
yet), `--timeout <ms>` per response (default 3000). With no subcommand,
sofleii prints usage.

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

On failure with `--json`, stdout carries an error object instead:

```json
{ "schema": 1, "error": { "kind": "timeout", "message": "…", "exit_code": 4 } }
```

`kind` is one of `not_found`, `usage`, `io`, `timeout`, `protocol`, `rpc`.

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
  transport/     Transport trait; serial.rs (USB, discovery); ble.rs (stub)
  client/        request ids, response matching, notifications set aside; rpc.rs = pure request builders
  commands/      info, profiles: return plain data, never print, no UI code
  output.rs      human and JSON rendering
  main.rs        clap CLI and exit codes
proto/           vendored .proto files (see proto/README.md for sources and licenses)
tests/golden.rs  request bytes captured from the v3.0.0 firmware
```

License: MIT (see the repository's `LICENSE`). The vendored protos keep
their own MIT licenses, listed in `proto/README.md`.
