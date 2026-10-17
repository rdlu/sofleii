---
title: Getting started
description: What the Eyelash Sofle is, how the two halves split the work, and what this firmware does, at a glance.
weight: 2
---

## The board

The **Eyelash Sofle** is a wireless split keyboard. Each half runs on a
**nice!nano v2** (nRF52840) and has a **nice!view** display. The hardware and
its original vendor firmware come from the Eyelash Sofle vendor
([original README](https://github.com/rdlu/sofleii/blob/main/README_JP.md));
this repo vendors the shield definition and builds its own keymap on top.

Beyond the alpha keys, the board has:

- a **five-way joystick** (up, down, left, right and a centre press), which
  each layer gives its own job;
- a rotary **encoder** on the left half (volume by default, rebindable live
  in [Studio](../studio/));
- RGB **underglow** on both halves: seven LEDs a half, one per column plus
  the thumb, so it lights zones rather than single keys.

## The two halves

| Half | Role | Runs |
| --- | --- | --- |
| Left | BLE **central**: talks to your computer and to the right half | Studio (USB and BLE), the custom RPC modules, the underglow module |
| Right | BLE **peripheral**: reports its keys to the left half | its keys, display and underglow strip |

That split matters in a few places:

- Studio and its stored keymap overlay live on the **left** half.
- After clearing bonds, power the **right** half on first, so it is
  advertising when the left half starts scanning. See
  [Pairing and Bluetooth](../bluetooth/).
- Each half gets its own firmware file. See [Flashing](../flashing/).

## The firmware at a glance

- **ZMK main on Zephyr 4.1**, through cormoran's `main+dya` fork, every
  dependency pinned to a commit. See [Firmware](../reference/firmware/).
- **Home-row mods** on S D F and J K L (Alt, Ctrl, Shift on each hand), held
  only when the other hand joins in. See
  [Hold-tap timing](../reference/hold-tap/).
- **Nine layers**: BASE, NAV, CODE, MEDIA, SYS|NUM, the GAME? picker, and
  three game layers. See [Keymap](../reference/keymap/).
- **Gaming mode**: locked layers without any hold-tap. See
  [Gaming mode](../gaming-mode/).
- **Per-layer underglow** from the in-repo `zmk-rgb-layer` module. See
  [Underglow](../underglow/).
- **Studio** keymap editing over USB and Bluetooth, no unlock key. See
  [Studio](../studio/).
- **Soft off** with the combo <kbd>Q</kbd> + <kbd>S</kbd> + <kbd>Z</kbd>,
  held for two seconds; one press of the reset button wakes the board. Left
  alone, it also goes to sleep after an hour idle. See
  [Combos](../reference/combos/).

## Where to go next

New firmware onto the board: [Flashing](../flashing/). Coming from a v2.x
release: [Upgrading to v3](../upgrading-to-v3/) first, since v3 needs a
settings reset. Building your own: [Building locally](../building/).
