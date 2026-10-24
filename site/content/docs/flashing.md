---
title: Flashing
description: Flash the Eyelash Sofle with sofleii flash (or mise run flash-release), by dragging .uf2 files onto the NICENANO drive, or with settings_reset.uf2 to clear the Studio overlay and Bluetooth bonds.
keywords: [flash, uf2, NICENANO, double-tap reset, settings_reset, flash-release, sofleii flash, bootloader, CURRENT.UF2, flash identify, wrong half, adopt, chip serial]
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

## With sofleii (recommended)

[`sofleii flash`](../sofleii/#flash) downloads the newest release and walks
you through both halves:

```sh
sofleii flash both          # or, from a checkout: mise run flash-release
```

It asks you to double-tap reset on the **left** half, waits up to 60 seconds
for its drive to mount, [checks that it is the left half](#which-half-is-this),
copies and syncs, waits for the half to reboot, then does the same for the
**right** half. At the end it reads the version the left half reports.

```text
firmware   release sofleii-fw-v3.2.0
           eyelash_sofle_studio_left.uf2 922 KB (legacy, eyelash_sofle, left)
           nice_view-eyelash_sofle_right-zmk.uf2 646 KB (legacy, eyelash_sofle, right)
           settings_reset.uf2 103 KB (settings_reset)

[1/2] left half: eyelash_sofle_studio_left.uf2
>> Double-tap RESET on the LEFT half (waiting 60 s)
drive      /run/media/rdlu/NICENANO (chip 94708422E5F2FD3E)
on chip    sofleii, eyelash_sofle, left
decision   flash (its firmware says so)
copying    ████████████████████████ 922 / 922 KB
remember   chip 94708422E5F2FD3E = your left half (verified)
rebooted   ✓
```

Other firmware: `--release <tag>` for an older release, `--ci [branch]` for
the newest successful CI build of `main` or a branch (`mise run flash-ci`
for `main`; needs a GitHub token, e.g. `GH_TOKEN=$(gh auth token)`), and
`--file` for files you built or downloaded. `sofleii flash left` or `right`
does one half.

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

With sofleii, `--reset` flashes the reset and then the normal firmware on
each half (two double-taps per half); `flash reset` flashes only the reset:

```sh
sofleii flash both --reset     # or: mise run flash-release-reset
sofleii flash left --reset     # only the left half
```

By hand: double-tap reset, copy `settings_reset.uf2`, wait a couple of
seconds for the half to reboot, double-tap reset **again**, and copy that
half's normal firmware.

## Locally built firmware

After a [local build](../building/), `mise run flash-left`, `flash-right` and
`flash-reset` (settings_reset on both halves) flash the matching file the
same way (`sofleii flash left --file eyelash_sofle_left.uf2`, …), and
`mise run build-flash` builds both halves, flashes them one after the other
and prints the memory budget at the end.

## Which half is this?

Every nice!nano mounts as the same `NICENANO` drive, both halves included,
and its `INFO_UF2.TXT` only describes the bootloader. So before it copies
anything, `sofleii flash` (and every mise flash task, which runs it) checks
two things:

- **the chip's serial number**, which is the nRF52840's own ID and doesn't
  change with the firmware. sofleii remembers each chip it has flashed
  (`sofleii flash chips`), and a known chip decides on its own: it's flashed
  as its half even when it carries the wrong firmware, and refused as the
  other one;
- **the firmware on the chip**, read from the drive's `CURRENT.UF2`, for a
  chip it hasn't seen yet. That is what
  [`sofleii flash identify`](../sofleii/#flash-identify) shows:

```text
drive       /run/media/rdlu/NICENANO
bootloader  nice!nano, UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) …
firmware    sofleii (identity record)
keyboard    eyelash_sofle
half        right
expected    left half: MISMATCH: do not flash
```

- Firmware built since [#98](https://github.com/rdlu/sofleii/issues/98)
  carries an **identity record** naming the keyboard and the half.
- Older builds (v3.2.0 and before) are recognised by their strings, as
  `legacy`.
- After a **settings reset** the half is still recognised: the reset firmware
  is small, and the end of the firmware it replaced is still in flash.
- An erased chip, vendor or other firmware, or contradictory leftovers come
  out **unknown**: nothing is flashed until you **adopt** the chip, with
  `--adopt left` (or `right`), or a bare `--adopt` to be asked to type the
  half. The same goes for a new chip whose firmware says it is the other half.
  Adoption is remembered, so it happens once per chip; a controller that moves
  to another keyboard is dropped with `sofleii flash forget <serial>`.

The firmware files are checked the same way: each `.uf2` is identified by its
content, so a right-half file can't end up on the left half by its name.
Dragging the file by hand checks nothing.

If the `NICENANO` drive never appears, or the half doesn't boot after the
copy, see [Troubleshooting](../reference/troubleshooting/#flashing).
