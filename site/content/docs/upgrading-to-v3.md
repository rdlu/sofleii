---
title: Upgrading to v3
description: Moving an Eyelash Sofle from a v2.x release to v3.0.0 (ZMK main, Zephyr 4.1) - settings reset on both halves, flash, power the right half first, then re-pair every host.
keywords: [upgrade, v3, v3.0.0, settings reset, re-pair, bluetoothctl remove, Zephyr 4.1]
weight: 4
---

[v3.0.0](https://github.com/rdlu/sofleii/releases/tag/v3.0.0)
(2026-10-17) moves the firmware from ZMK v0.3 / Zephyr 3.5 onto **ZMK main
on Zephyr 4.1**, through cormoran's `main+dya` fork. **The keymap is
unchanged**: same layers, home-row mods, combos, gaming mode and underglow.
It is a major version because flashing it needs a settings reset and fresh
pairings.

## Steps

1. Flash `settings_reset.uf2` on **both** halves.
2. Flash the new left and right firmware.
3. Power both halves off. Turn on the **right** half first, then the left.
4. On every computer the keyboard was paired with, remove the old "Eyelash
   Sofle" pairing (for example `bluetoothctl remove <MAC>`), then pair again
   from a Bluetooth profile key on the `SYS|NUM` layer (see
   [Pairing and Bluetooth](../bluetooth/)).

With mise:

```sh
mise run flash-release-reset   # once per half
mise run flash-release         # left, then right
```

The settings reset also clears any keymap edits you saved from Studio. If
you want them back, redo them in [Studio](../studio/) after the upgrade.

## What else changed

- The board is now **`nice_nano//zmk`** (it was `nice_nano_v2`). This only
  matters if you build locally; see [Building locally](../building/).
- The reset artifact is now **`settings_reset.uf2`** (it was
  `settings_reset-nice_nano_v2-zmk.uf2`).
- The shield is vendored in-repo at `config/boards/shields/eyelash_sofle/`.
- Three DYA Studio modules are gone: `battery-history` and `settings-rpc`
  (still v0.3-only upstream) and `runtime-input-processor` (only the vendor
  overlay used it). Their Studio panels went with them. `ble-management` and
  `runtime-sensor-rotate` stay.
- Flash use went up by about 47 to 53 KB per half, which is the newer Zephyr
  and BLE stack, not the keymap. There is plenty of room left; see
  [Firmware](../reference/firmware/#memory-budget).

The [release notes](https://github.com/rdlu/sofleii/releases/tag/v3.0.0)
have the full list, what was tested on hardware, and one known upstream issue
that only affects RPC clients.
