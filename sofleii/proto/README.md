# Vendored protobuf definitions

The `.proto` files here are copied **unmodified** from upstream so they can be
diffed against it on every bump. The upstream files carry no per-file license
header; each directory holds its upstream `LICENSE` instead.

| Directory | Upstream | Commit | License |
|---|---|---|---|
| `studio/` (`studio`, `core`, `meta`, `custom`, `keymap`, `behaviors`) | [cormoran/zmk-studio-messages](https://github.com/cormoran/zmk-studio-messages) `proto/zmk/` (fork of zmkfirmware/zmk-studio-messages, pulled in by cormoran's ZMK `main+dya`) | `89b81d2e587fce807b668dff2a6967a40beef421` | MIT, (c) 2024 The ZMK Contributors ([`studio/LICENSE`](studio/LICENSE)) |
| `ble_management/` | [cormoran/zmk-module-ble-management](https://github.com/cormoran/zmk-module-ble-management) `proto/zmk/ble_management/` | `57738cc4fc6ba80e82a7ac57741a0339cb186cd4` | MIT, (c) 2026 cormoran ([`ble_management/LICENSE`](ble_management/LICENSE)) |

`keymap.proto` and `behaviors.proto` are only here because `studio.proto`
imports them; sofleii does not use them yet.

The directories are not named `zmk/` like upstream because the repo's
`.gitignore` ignores `zmk/` (the west-cloned ZMK tree) at any depth.

## Bumping

The commits must match what the firmware is built from: `zmk-studio-messages`
is whatever cormoran's ZMK revision in `config/west.yml` imports (check
`modules/msgs/zmk-studio-messages` after `west update`), and
`zmk-module-ble-management` is pinned directly in `config/west.yml`. Copy the
files over, update the table, and run `cargo test`.
