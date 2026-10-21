---
title: Studio
description: Edit the Eyelash Sofle keymap live from the browser with DYA Studio, over USB (WebSerial) or Bluetooth (WebBluetooth), the Chrome flag Linux needs for BLE, and why saved edits survive a flash.
keywords: [Studio, ZMK Studio, DYA Studio, live keymap editing, WebSerial, WebBluetooth, Chrome flag, uucp, encoder, runtime-sensor-rotate]
weight: 6
---

The left half runs cormoran's Studio-enabled ZMK, so you can change the
keymap from a browser while the keyboard is in use, over **USB or
Bluetooth**, without rebuilding anything.

## Connect

1. Open a Studio web app in a Chromium browser (Chrome, Edge):
   - the hosted DYA Studio app at [studio.dya.cormoran.works](https://studio.dya.cormoran.works);
   - or **Sofle Studio**, a fork of [cormoran/dya-studio](https://github.com/cormoran/dya-studio)
     tailored for this keyboard, maintained as a separate project (work in
     progress).
2. Connect the **left** half, over **USB** (WebSerial) or **Bluetooth**
   (WebBluetooth).
3. Edit. Changes apply at once; **Save** writes them to the left half's
   flash.

Locking is off (`CONFIG_ZMK_STUDIO_LOCKING=n`), so there is no unlock key to
press first.

Besides keys and layers, Studio can change:

- **the encoder**: its binding is a `runtime-sensor-rotate` behavior, volume
  by default, editable on every layer;
- **Bluetooth profiles**, through the `ble-management` module.

## Linux notes

- **USB**: the browser needs access to the serial device the left half
  exposes. On Arch Linux that means being in the `uucp` group.
- **Bluetooth**: if the app says "BLE Not Supported on your Browser", enable
  `chrome://flags/#enable-experimental-web-platform-features` and relaunch
  Chrome. Web Bluetooth is off by default on Chrome for Linux. This is a
  browser setting, not a firmware problem, and typing over Bluetooth is
  unaffected.

Studio over BLE in the browser was not re-tested on v3.0.0; USB was.

## Saved edits outlive a flash

Saved Studio edits live in the left half's settings partition, not in the
firmware, so **flashing a new release doesn't remove them**: the stored
overlay is applied on boot over the compiled keymap. If a new firmware still
behaves like the old one, that is why: [`sofleii keymap diff`](../sofleii/#keymap)
lists the keys that differ from `config/eyelash_sofle.keymap`.

To go back to the compiled keymap, flash `settings_reset.uf2` on the left
half and then the left firmware (see [Flashing](../flashing/#settings-reset)).
That also clears the left half's Bluetooth bonds. To keep edits for good,
make them in `config/eyelash_sofle.keymap` and rebuild; export them from
Studio first if you need a copy.

## Editing in the repo instead

[keymap-editor](https://nickcoutsos.github.io/keymap-editor/) edits
`config/eyelash_sofle.keymap` in the browser and commits the change to this
repository, so the change lives in Git rather than in one keyboard's flash.
Install its [GitHub App](https://github.com/apps/keymap-editor) on the repo
once, sign in, and pick the repo; it finds the `.keymap` and `.json` in
`config/`. A save that lands on `main` starts the firmware build and the
keymap render in GitHub Actions; flash the new build with
`mise run flash-release` (see [Flashing](../flashing/)).
