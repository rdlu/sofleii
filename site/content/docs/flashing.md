---
title: Flashing
description: Flash the Eyelash Sofle with mise run flash-release, by dragging .uf2 files onto the NICENANO drive, or with settings_reset.uf2 to clear the Studio overlay and Bluetooth bonds.
keywords: [flash, uf2, NICENANO, double-tap reset, settings_reset, flash-release, bootloader]
weight: 3
---

Each half is flashed on its own, over USB, through the nice!nano's UF2
bootloader: **double-tap the reset button** and the half shows up as a USB
drive called `NICENANO`. Copy a `.uf2` file onto it and the half reboots into
the new firmware when the copy finishes. Flash the **left** half first, then
the right.

> [!IMPORTANT]
> Coming from v2.x? Read [Upgrading to v3](../upgrading-to-v3/) first: v3.0.0
> needs a settings reset on both halves before the new firmware.

## The firmware files

| File | Flash onto |
| --- | --- |
| `eyelash_sofle_studio_left.uf2` | the left half (Studio enabled) |
| `nice_view-eyelash_sofle_right-zmk.uf2` | the right half |
| `settings_reset.uf2` | either half: clears the Studio overlay and the Bluetooth bonds |

They are attached to every [GitHub release](https://github.com/rdlu/sofleii/releases/latest),
and every `main` build on GitHub Actions uploads them as a `firmware`
artifact. The local build tasks name the halves `eyelash_sofle_left.uf2` and
`eyelash_sofle_right.uf2` instead (see [Building locally](../building/)).

## With mise (recommended)

One task downloads the firmware from the latest successful `main` build on
GitHub Actions and walks you through both halves:

```sh
mise run flash-release
```

It prompts you to double-tap reset on the **left** half, waits up to 60
seconds for `NICENANO` to mount, copies and syncs, waits for the drive to go
away, then does the same for the **right** half. It needs
[mise](https://mise.jdx.dev/) and an authenticated [`gh`](https://cli.github.com/).

The mount point it waits for is `/run/media/$USER/NICENANO`, where udisks
mounts removable drives on most Linux desktops.

## By hand (drag-and-drop)

1. Download the `.uf2` files from the [latest release](https://github.com/rdlu/sofleii/releases/latest).
2. Connect the **left** half over USB with a data cable (not a charge-only one).
3. Double-tap its reset button. A `NICENANO` drive appears.
4. Copy `eyelash_sofle_studio_left.uf2` onto it. The half reboots when the copy completes.
5. Disconnect it, connect the **right** half, and double-tap its reset button.
6. Copy `nice_view-eyelash_sofle_right-zmk.uf2` onto the drive.
7. Power-cycle both halves; they pair with each other on their own.

On Linux, run `sync` after the copy so the write is flushed before the drive
disappears.

## Settings reset

`settings_reset.uf2` is a firmware of its own that wipes the half's settings
partition, then you flash the normal firmware on top. Use it when:

- **a new keymap doesn't take effect.** Keymap changes saved from
  [Studio](../studio/) live in the left half's settings and survive a normal
  flash, so they override the compiled keymap;
- **the halves won't pair** with each other (see
  [Pairing and Bluetooth](../bluetooth/#re-pairing-the-halves));
- **upgrading to v3** (both halves; see [Upgrading to v3](../upgrading-to-v3/)).

It also erases the half's Bluetooth bonds, so expect to re-pair afterwards.

With mise, flash the reset onto whichever half is in bootloader mode, then
the normal firmware:

```sh
mise run flash-release-reset   # double-tap the half to clear when asked
mise run flash-release         # left, then right
```

By hand: double-tap reset, copy `settings_reset.uf2`, wait a couple of
seconds for the half to reboot, double-tap reset **again**, and copy that
half's normal firmware.

## Locally built firmware

After a [local build](../building/), `mise run flash-left`, `flash-right` and
`flash-reset` copy the matching file the same way, and `mise run build-flash`
builds both halves, flashes them one after the other and prints the memory
budget at the end.

If the `NICENANO` drive never appears, or the half doesn't boot after the
copy, see [Troubleshooting](../reference/troubleshooting/#flashing).
