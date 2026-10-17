---
title: Firmware
description: What the Eyelash Sofle firmware is built from - ZMK main via cormoran's main+dya fork, Zephyr 4.1, the pinned west dependencies, the modules, the build matrix, notable Kconfig, and the flash and RAM budget.
keywords: [ZMK, Zephyr 4.1, west.yml, pins, cormoran, main+dya, ble-management, runtime-sensor-rotate, zmk-rgb-layer, Kconfig, memory budget, nice_nano//zmk]
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
| `zmk-module-ble-management` | west, left half | BLE profile list, switch, rename and unpair, and output priority, over Studio RPC |
| `zmk-behavior-runtime-sensor-rotate` | west, left half | the encoder behavior (`rsr_vol`), editable live in Studio |
| `zmk-rgb-layer` | in-repo (`zmk-rgb-layer/`) | per-layer underglow, moods, brightness; see [Underglow](../../underglow/) |

The Studio and RPC settings are central-only and live in the left shield's
`eyelash_sofle_left.conf`, not in the shared `config/eyelash_sofle.conf`.

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

Left half: a **local build** (2026-10-17) with the peripheral battery proxy
([#45](https://github.com/rdlu/sofleii/issues/45)), 468892 B of flash and
126522 B of RAM, +928 B and +168 B over v3.0.0. Right half and settings
reset: unchanged from v3.0.0, measured from CI run
[39026110927](https://github.com/rdlu/sofleii/actions/runs/39026110927)
(2026-10-17).

| Build | Flash used | Flash free | RAM used | RAM free |
| --- | ---: | ---: | ---: | ---: |
| Left (Studio, nice!view; local build) | 458 KB / 792 KB (57.82%) | **334 KB** | 124 KB / 256 KB (48.26%) | **132 KB** |
| Right (nice!view) | 323 KB / 792 KB (40.8%) | **469 KB** | 77 KB / 256 KB (30.1%) | **179 KB** |
| Settings reset | 51 KB / 792 KB (6.5%) | | 13 KB / 256 KB (4.9%) | |

The left half is the tightest: Studio, both Studio transports (USB CDC-ACM
and BLE GATT), the custom RPC modules, `zmk-rgb-layer` and the peripheral
battery proxy (#45) are all on it. The
move from Zephyr 3.5 to 4.1 cost about 47 KB of flash and 14 KB of RAM on the
left half, and 53 KB and 15 KB on the right, even after dropping three
modules: that is the newer Zephyr and BLE stack. `zmk-rgb-layer` takes well
under 1 KB.

Refresh this table after significant keymap or config changes, from the
`Memory region` block of the latest `Build ZMK firmware` run log
(`mise run build-flash` prints the same numbers for a local build).
