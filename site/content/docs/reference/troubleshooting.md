---
title: Troubleshooting
description: Fixes for the Eyelash Sofle firmware - a new keymap that doesn't take effect, NICENANO not appearing, halves that won't pair, "BLE Not Supported" in Studio, and local builds failing with Invalid BOARD or toolchain errors.
keywords: [troubleshooting, Studio overlay, settings reset, NICENANO, pairing, BLE Not Supported, Invalid BOARD, nice_nano//zmk, doctor, doctor-fix, protobuf, ZEPHYR_TOOLCHAIN_VARIANT, snippet]
weight: 15
---

## Keyboard

### A new firmware still behaves like the old keymap

Keymap edits saved from [Studio](../../studio/) are stored on the left
half's settings partition and **survive a normal flash**: they are applied
on boot over the compiled keymap. Flashing the normal firmware again won't
help. This also explains "the right half shows old behavior": the right half
only relays key presses, and the stale keymap is on the left.

Fix: on the **left** half, flash `settings_reset.uf2`, wait for it to
reboot, double-tap reset again, then flash the left firmware.

```sh
mise run flash-release-reset   # double-tap the LEFT half when asked
mise run flash-release         # left, then right
```

This also clears the left half's Bluetooth bonds; see
[Pairing and Bluetooth](../../bluetooth/).

### The halves won't connect to each other

Settings reset on both halves, flash the firmware back, power both off, then
power on the **right** half first and the **left** second. See
[Re-pairing the halves](../../bluetooth/#re-pairing-the-halves).

### A computer won't reconnect after an upgrade or a reset

Remove the old "Eyelash Sofle" entry on the computer
(`bluetoothctl remove <MAC>` on Linux), then pair again from a profile key on
SYS|NUM.

### "BLE Not Supported on your Browser" in Studio

Web Bluetooth is off by default in Chrome on Linux. Enable
`chrome://flags/#enable-experimental-web-platform-features` and relaunch.
Not a firmware problem: typing over Bluetooth works either way.

## Flashing

### `NICENANO` doesn't appear after a double-tap

- Use a USB-C cable that carries data, not a charge-only one.
- Try another USB port; some hubs don't pass the bootloader through cleanly.
- Check `dmesg` or your file manager in case it enumerated as something else.
- The bootloader leaves by itself after about 30 seconds. Double-tap again
  if you were slow.

### The copy finished but the half doesn't boot

The copy may have been cut off before it was flushed. Double-tap reset and
copy again, run `sync`, and let the drive unmount before unplugging.

## Builds

Run `mise run doctor` first whenever a local build fails in a way that looks
environmental. It is fast and offline. If it finds drift, `mise run
doctor-fix` repairs it.

### `Invalid BOARD: nice_nano//zmk`

A desynced west workspace, not a keymap or manifest problem. CMake stops
before it reads `config/`, and the list of "valid boards" it prints is
Zephyr's own. A tree left on the old v0.3 / Zephyr 3.5 base has
`nice_nano_v2`, not `nice_nano//zmk`.

The usual cause is a project clone whose git remote points somewhere other
than the manifest says. west names remotes from the manifest but never
repoints an existing clone, so `west update` can't fetch the pinned commit
and leaves the project on whatever it had. `doctor` reports these as
`REMOTE` and `DRIFT`; `doctor-fix` repoints the remotes, runs `west update`,
and deletes the stale `build/`.

A good workspace for the current manifest has `zephyr/VERSION` at 4.1 and
`zmk/app/boards/nicekeyboards/nice_nano/nice_nano_nrf52840_zmk.dts` present.

If CI is green and your local build isn't, your workspace drifted, not the
manifest. `mise run doctor-ci` compares your checkouts with the commits the
last successful `main` build used.

### Protobuf "gencode/runtime" VersionError on the left half

The Studio build generates nanopb code with the system `protobuf`, which has
to be at least as new as the generator. Update it through pacman:

```sh
sudo pacman -Syu python-protobuf
```

The right half doesn't build Studio and isn't affected.

### `dtc failed with return code: no such file or directory`

This happens after you move or rename the checkout. The Zephyr SDK's host
tools, `dtc` among them, embed their absolute install path, so they break
when `tools/zephyr-sdk-0.17.0` moves. Re-run:

```sh
mise run setup
```

Its last step runs the SDK's `setup.sh -h -c`, which relocates the host tools
and registers the SDK with CMake again.

### "Unknown arguments specified" at configure time

`ZEPHYR_TOOLCHAIN_VARIANT` is unset. `.mise.toml` sets it, together with
`ZEPHYR_SDK_INSTALL_DIR`, so run builds through `mise run build-*`.

### The left half has no USB serial port for Studio

The snippet flag is case-sensitive: `-DSNIPPET=studio-rpc-usb-uart`. A
lowercase `-Dsnippet` is ignored without a word, and the left half ships
Bluetooth-only. `mise run build-left` gets it right; to check a build:

```sh
grep -E 'TRANSPORT_UART=|USB_CDC_ACM=' build/zephyr/.config
```
