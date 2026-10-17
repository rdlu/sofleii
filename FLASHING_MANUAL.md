# Manual Flashing Guide

This document covers manual flashing without the `mise` automation. Use this if you don't have `mise` / `gh` installed, or if you prefer the step-by-step drag-and-drop flow.

For the automated flow, see the [Flashing guide](https://sofleii.dlu.one/docs/flashing/) — `mise run flash-release` handles everything in one command.

---

## Prerequisites

- The `.uf2` firmware files from the [latest release](../../releases/latest):
  - `eyelash_sofle_studio_left.uf2` — left half (ZMK Studio enabled)
  - `nice_view-eyelash_sofle_right-zmk.uf2` — right half
  - `settings_reset.uf2` — settings reset (clears Studio overlay + BT bonds)
- A USB-C cable that supports data (not charge-only)

---

## Standard flash (drag-and-drop)

1. Connect the **left** half via USB.
2. Double-tap the reset button on the left half — a `NICENANO` USB drive appears in your file manager.
3. Drag `eyelash_sofle_studio_left.uf2` onto the `NICENANO` drive. The keyboard reboots automatically once the file copy completes.
4. Disconnect the left half and connect the **right** half.
5. Double-tap the reset button on the right half.
6. Drag `nice_view-eyelash_sofle_right-zmk.uf2` onto the drive.
7. Done — disconnect, power-cycle both halves, and they should pair up.

> Flash left first, then right.

---

## ⚠️ Important: ZMK Studio overlay persists across firmware updates

If you have ever used [ZMK Studio](https://studio.zmk.dev) to edit the keymap live, the changes are stored in the left half's settings partition. **These settings survive a normal firmware flash** — the stored overlay is re-applied on boot and overrides the compiled keymap.

**Symptoms** that you're hitting this:

- You flashed new firmware but the keymap behaves like the old version
- A key you've changed in the compiled firmware is still showing the old behavior
- No amount of re-flashing the normal `.uf2` fixes it

**Fix: flash `settings_reset` first, then the normal firmware on top.**

```
1. Double-tap reset on the LEFT half → drag settings_reset.uf2 onto NICENANO
2. Wait for the left half to reboot (takes ~2 seconds)
3. Double-tap reset on the LEFT half AGAIN → drag eyelash_sofle_studio_left.uf2 onto NICENANO
```

The settings reset wipes:
- ZMK Studio's stored keymap overlay
- Bluetooth pairings (so the halves will need to re-pair after)

After this two-step flash, the compiled keymap will actually run. If the halves don't auto-pair, see [Re-pairing the halves](https://sofleii.dlu.one/docs/bluetooth/#re-pairing-the-halves) on the docs site.

---

## Building the firmware locally

If you need to build from source instead of using a release, install the toolchain and run the `mise` tasks documented in [Building](https://sofleii.dlu.one/docs/building/) on the docs site. The compiled artifacts land at:

- `eyelash_sofle_studio_left.uf2` (after `mise run build-left`)
- `eyelash_sofle_right.uf2` (after `mise run build-right`)
- `settings_reset.uf2` (after `mise run build-reset`)

Once they exist, either drag-and-drop as above or use `mise run flash-left` / `flash-right` / `flash-reset` (which wait for the NICENANO mount automatically).

---

## Troubleshooting

**`NICENANO` drive doesn't appear after double-tap reset**

- Make sure you're using a **data** USB-C cable, not a charge-only cable
- Try a different USB port (some hubs don't pass through cleanly)
- Check `dmesg` / your file manager for any hint that the device enumerated as something else
- The bootloader auto-exits after ~30 seconds — if you're slow, re-double-tap

**File copy completes but the keyboard doesn't boot**

- The copy may have been interrupted before `sync` completed. Re-double-tap reset and copy again, ensuring the `NICENANO` drive unmounts cleanly before unplugging.
- On Linux, running `sync` after the `cp` is a good habit to force the buffer flush.

**After flashing, the right half shows old behavior**

- This is the Studio-overlay issue above. The right half doesn't run Studio (only the left does), so this symptom actually means the LEFT half has the stale overlay and the right half is just relaying its state.
- Follow the `settings_reset` → normal firmware two-step on the **left** half.

**Halves won't pair after flashing**

- See [Re-pairing the halves](https://sofleii.dlu.one/docs/bluetooth/#re-pairing-the-halves) on the docs site.
