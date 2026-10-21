---
title: Firmware
description: What the Eyelash Sofle firmware is built from - ZMK main via cormoran's main+dya fork, Zephyr 4.1, the pinned west dependencies, the modules, the build matrix, notable Kconfig, and the flash and RAM budget.
keywords: [ZMK, Zephyr 4.1, west.yml, pins, cormoran, main+dya, ble-management, runtime-sensor-rotate, device-info, zmk-rgb-layer, Kconfig, memory budget, nice_nano//zmk]
weight: 14
---

## Base

- **ZMK main**, through **cormoran's fork** at `main+dya`: ZMK main plus a
  custom Studio protocol and cormoran's BLE and activity extensions. Upstream
  ZMK has no v0.4 release; this is the line cormoran's "v4" modules target.
- **Zephyr 4.1** (`v4.1.0+zmk-fixes`).
- Board **`nice_nano//zmk`** (Zephyr's hardware model v2 name; the default
  revision is the nice!nano v2), with the **`eyelash_sofle_left`** /
  **`eyelash_sofle_right`** shield and **`nice_view`**.
- The shield is vendored in-repo at
  [`config/boards/shields/eyelash_sofle/`](https://github.com/rdlu/sofleii/tree/main/config/boards/shields/eyelash_sofle),
  adapted from the vendor's v0.3-only module.

## Dependencies

Every west project is pinned to a commit, so the same repo commit always
builds the same firmware. Read from `config/west.yml` on every site build:

{{< west-pins >}}

`zephyr` is declared in `config/west.yml` on purpose: ZMK's own manifest
names it by branch, so pinning `zmk` alone would leave Zephyr floating.

Pinning means updates don't arrive by themselves. `mise run deps-check`
reports which pins have moved upstream. To take one: change the revision in
`config/west.yml` (keep its `# track:` comment), `west update`, `mise run
doctor`, build both halves, and commit with the reason. If a `zmk` bump
changes the Zephyr entry in ZMK's manifest, mirror it in `config/west.yml`.

## Modules

| Module | Where | Does |
| --- | --- | --- |
| `zmk-module-ble-management` | west, left half; from our fork [`rdlu/zmk-module-ble-management`](https://github.com/rdlu/zmk-module-ble-management) | BLE profile list, switch, rename and unpair, output priority and split info, over Studio RPC. The fork is cormoran's `57738cc` plus a `get_split_info` fix (the reply now carries `info`, and the central reports whether the right half is connected), pending upstream |
| `zmk-behavior-runtime-sensor-rotate` | west, left half | the encoder behavior (`rsr_vol`), editable live in Studio |
| `zmk-feature-device-info` | west, left half | build and version info over Studio RPC (subsystem `zmk__device_info`), read by `sofleii info` |
| `zmk-rgb-layer` | in-repo (`zmk-rgb-layer/`) | per-layer underglow, moods, brightness; see [Underglow](../../underglow/) |

The Studio and RPC settings are central-only and live in the left shield's
`eyelash_sofle_left.conf`, not in the shared `config/eyelash_sofle.conf`.

`zmk-feature-device-info` reports which build the keyboard runs. Its
`zmk_config_version` is `git describe --dirty --always --tags` of this repo,
taken when the build is configured: a local build gives something like
`v3.0.0-65-g1e468b7` (with `-dirty` if the tree had uncommitted changes), and a
CI build gives the bare short hash of the pushed commit, because the CI
checkout is shallow and has no tags. For CI to read the repo at all, the left
build loads `config/ci-git-safe-directory.cmake`, which marks the workspace as a
git safe directory (the build runs as root on a checkout the runner owns).
Reads don't need a Studio unlock.

## Build matrix

GitHub Actions builds three firmwares on every push to `main` that touches
`config/`, `build.yaml` or `zmk-rgb-layer/`, using ZMK's reusable
`build-user-config` workflow at a pinned ZMK-main commit:

| Artifact | Shield | Extras |
| --- | --- | --- |
| `eyelash_sofle_studio_left` | `eyelash_sofle_left nice_view` | Studio on, locking off, `studio-rpc-usb-uart` snippet, `zmk-rgb-layer` |
| `nice_view-eyelash_sofle_right-zmk` | `eyelash_sofle_right nice_view` | `zmk-rgb-layer` |
| `settings_reset` | `settings_reset` | |

`zmk-rgb-layer` goes in through `-DZMK_EXTRA_MODULES`, exactly as the local
build tasks do.

## Notable settings

From `config/eyelash_sofle.conf`:

| Setting | Value |
| --- | --- |
| Idle sleep | after 1 hour (`CONFIG_ZMK_IDLE_SLEEP_TIMEOUT=3600000`) |
| Soft off | on (`CONFIG_ZMK_PM_SOFT_OFF=y`) |
| Debounce | 8 ms press, 8 ms release |
| HID report | NKRO off (6KRO) |
| BLE transmit power | +8 dBm |
| Pointing (mouse keys) | on |
| Underglow | on; off at boot, solid effect, max brightness 60, off when idle, same on USB and battery |

## Memory budget

Left half: a **local build** (2026-10-21) with the MYGAME custom game slot
([#79](https://github.com/rdlu/sofleii/issues/79)), 472572 B of flash and
128354 B of RAM. The tenth layer cost +1204 B of flash and +1232 B of RAM,
measured as two builds of the same tree with and without it: about 1.2 KB of
RAM per 64-key layer, since ZMK keeps the keymap in RAM so Studio can edit it.
Before it, the 2026-10-19 build with the peripheral battery proxy
([#45](https://github.com/rdlu/sofleii/issues/45)), device info
([#46](https://github.com/rdlu/sofleii/issues/46)) and the `get_split_info`
fix from our `ble-management` fork was 471368 B / 127122 B.
The fix cost +80 B of flash and no RAM over the same build with cormoran's
`57738cc` (471288 B / 127122 B). Device info cost +2396 B and +600 B over the #45 build
(468892 B / 126522 B), which was +928 B and +168 B over v3.0.0. Right half:
the keymap lives on the central only, so MYGAME adds nothing there; a local
right build is 330840 B / 78952 B (re-measured 2026-10-21, the same as
2026-10-19), the same RAM as CI run
[39026110927](https://github.com/rdlu/sofleii/actions/runs/39026110927)
(2026-10-17) and 96 B less flash (CI's longer source paths). Device info is
off on the right (`CONFIG_ZMK_DEVICE_INFO` unset), so it adds nothing there,
and neither does `ble-management` (left only). Settings reset: unchanged
from v3.0.0, measured from that CI run.

| Build | Flash used | Flash free | RAM used | RAM free |
| --- | ---: | ---: | ---: | ---: |
| Left (Studio, nice!view; local build) | 461 KB / 792 KB (58.27%) | **331 KB** | 125 KB / 256 KB (48.96%) | **131 KB** |
| Right (nice!view) | 323 KB / 792 KB (40.8%) | **469 KB** | 77 KB / 256 KB (30.1%) | **179 KB** |
| Settings reset | 51 KB / 792 KB (6.5%) | | 13 KB / 256 KB (4.9%) | |

The left half is the tightest: Studio, both Studio transports (USB CDC-ACM
and BLE GATT), the custom RPC modules, `zmk-rgb-layer`, the peripheral
battery proxy (#45) and device info (#46) are all on it. The
move from Zephyr 3.5 to 4.1 cost about 47 KB of flash and 14 KB of RAM on the
left half, and 53 KB and 15 KB on the right, even after dropping three
modules: that is the newer Zephyr and BLE stack. `zmk-rgb-layer` takes well
under 1 KB.

Refresh this table after significant keymap or config changes, from the
`Memory region` block of the latest `Build ZMK firmware` run log
(`mise run build-flash` prints the same numbers for a local build).
