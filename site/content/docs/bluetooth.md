---
title: Pairing and Bluetooth
description: Pair the Eyelash Sofle with up to five hosts from the SYS|NUM layer, clear a profile, and re-pair the two halves with each other.
keywords: [bluetooth, BLE, pairing, profiles, BT_SEL, BT_CLR, re-pair, split, bonds]
weight: 5
---

There are two separate Bluetooth links:

- **host profiles**: the left half (the central) pairs with your computers,
  phones and tablets, up to five of them;
- **the split link**: the right half pairs with the left half. That bond is
  made automatically the first time both halves run the firmware.

## Host profiles

The profile keys are on the **SYS|NUM** layer, which you reach by holding
the left-thumb `mo(SYS|NUM)` key.

| SYS\|NUM key | BASE key in that spot | Does |
| --- | --- | --- |
| `BT0` to `BT4` | <kbd>1</kbd> to <kbd>5</kbd> | select profile 0 to 4 |
| `BT_CLR` | <kbd>`</kbd> (top-left corner) | clear the bond of the **selected** profile |

To pair a new host: select a free profile, then pair from the host's
Bluetooth settings, where the keyboard shows up as **Eyelash Sofle**. To
replace a host on a profile: select it, `BT_CLR`, and pair again. Remove the
old entry on the host too (`bluetoothctl remove <MAC>` on Linux) before
pairing again.

Passkey entry is off, so pairing doesn't ask you to type a code.

The `ble-management` module also exposes the profile list over Studio RPC:
list, switch, rename and unpair profiles, and set the output priority. That is
the interface the planned [sofleii CLI/TUI](../sofleii/) will use.

## Re-pairing the halves

If the halves stop connecting to each other, clear their bond and let them
find each other again:

1. Flash `settings_reset.uf2` onto **both** halves (see
   [Flashing](../flashing/#settings-reset)). `BT_CLR` only clears a host
   profile, and the right half can't reach the SYS|NUM layer on its own, so a
   settings reset is what clears the split bond.
2. Flash the normal firmware back onto each half.
3. Power both halves off.
4. Power the **right** half on first. It is the peripheral and starts
   advertising.
5. Power the **left** half on. It is the central, scans, and connects within
   a few seconds.

A settings reset on the left half also clears its host profiles and any
Studio edits, so re-pair your computers afterwards.

## Battery levels

The host sees two Battery Services: the first is the left half, and the
second, with the user description "Peripheral 0", is the right half,
forwarded by the left ([#45](https://github.com/rdlu/sofleii/issues/45)).
BlueZ's `Battery1` and upower show only the first; the right half's level
can be read over GATT, which is what [sofleii](../sofleii/) will do.

## Power

- **Sleep**: after an hour without a key press, the board goes to deep sleep.
- **Soft off**: hold <kbd>Q</kbd> + <kbd>S</kbd> + <kbd>Z</kbd> for two
  seconds. One press of the reset button wakes it. See
  [Combos](../../keymap/combos/).
