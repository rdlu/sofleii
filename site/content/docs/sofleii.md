---
title: The sofleii tool
linkTitle: Overview
description: sofleii, a Rust CLI and ratatui TUI for the Eyelash Sofle - a live dashboard, split link, batteries, BLE profiles, output priority, firmware update checks and the live keymap (with its drift from the .keymap) over the Studio RPC protocol, with a scriptable --json contract, stable exit codes and a waybar module.
keywords: [sofleii, CLI, TUI, ratatui, Rust, Studio RPC, BLE profiles, battery, waybar, JSON, exit codes, keymap, drift]
weight: 17
---

**sofleii** (Sofle Interactive Interface) is a command-line tool and a
terminal UI for the keyboard: see whether both halves are up, read the
batteries, list and switch BLE profiles, flip the output between USB and
Bluetooth, check the firmware against the releases, and see the keymap the
keyboard runs and how it differs from the repo's. No browser, no
Chrome flags. It lives in this repo, in [`sofleii/`](https://github.com/rdlu/sofleii/tree/main/sofleii),
next to the firmware it talks to.

The CLI comes first: everything the TUI shows is also a plain subcommand,
with `--json` output and exit codes meant for scripts, cron and status bars.

> [!NOTE]
> **Works today, not released yet.** You build it from the repo. The first
> release (`sofleii-tui-v0.1.0`, with prebuilt binaries) is tracked in
> [#83](https://github.com/rdlu/sofleii/issues/83); see the
> [roadmap](#roadmap) for what's coming. Linux only: Bluetooth goes
> through BlueZ.

## How it talks to the keyboard

Through the **Studio RPC protocol**, the same one the Studio web app uses,
to the **left half**, over USB serial or Bluetooth. The firmware carries
cormoran's custom-RPC subsystems: `ble-management` for BLE profiles, the
output priority and the split link, and `zmk-feature-device-info` for the
running build (see [Firmware](../reference/firmware/#modules)). The
batteries are read separately, over Bluetooth GATT through BlueZ, because
they aren't part of Studio RPC.

## Installing it

Prebuilt binaries for x86_64 and aarch64 Linux come with each
[`sofleii-tui-vX.Y.Z` release](https://github.com/rdlu/sofleii/releases)
(the `sofleii-fw-vX.Y.Z` releases are the firmware).

**With mise** (its `github` backend; `version_prefix` keeps it to the CLI's
releases, since GitHub's "latest" release is always the firmware):

```bash
mise use -g 'github:rdlu/sofleii[version_prefix=sofleii-tui-v]'
sofleii status
```

mise holds back releases younger than its `minimum_release_age` setting, so
for a day or so after a release `latest` still resolves to the previous one.
Name the version to get it at once:
`mise use -g 'github:rdlu/sofleii[version_prefix=sofleii-tui-v]@0.2.0'`.

**By hand:** download `sofleii-<version>-<target>.tar.gz` and its `.sha256`
from the release, check them, and put the binary on your `PATH`:

```bash
sha256sum -c sofleii-<version>-<target>.tar.gz.sha256
gh attestation verify sofleii-<version>-<target>.tar.gz -R rdlu/sofleii
tar xzf sofleii-<version>-<target>.tar.gz
install -Dm755 sofleii-<version>-<target>/sofleii ~/.local/bin/sofleii
```

**From source** (Rust 1.99+ and `protoc`, both from pacman:
`pacman -S rust protobuf`), into `~/.cargo/bin`:

```bash
cargo install --git https://github.com/rdlu/sofleii sofleii --locked
```

Or, from a clone of the repo, without installing:

```bash
mise run sofleii                    # the TUI
mise run sofleii -- status          # any subcommand after --
mise run sofleii -- profiles --json
```

`sofleii update check --cli` tells you when a newer release is out.

Over USB your user needs access to the serial port (the `uucp` group on
Arch). Over Bluetooth, the keyboard only has to be paired and connected as
usual; sofleii reuses that connection.

## The TUI

`sofleii` with no arguments, on a terminal, opens a live dashboard:

![The sofleii TUI: link state, transport, output, profile, batteries, firmware and split link at the top, the profile list, then the log](/img/sofleii/tui-dashboard.svg)

From the top:

- **The title bar** names the keyboard and shows the **link state** on the
  right (see below).
- **The summary**: the transport in use, the output (where keystrokes go,
  with the priority you set), the active profile, Studio's lock state, both
  batteries as bars, the firmware version and whether it's up to date, and
  the **split link** between the halves.
- **Profiles**: the five BLE slots, with the active one marked `▶`.
- **Log**: what the session did, newest at the bottom. `d` adds the debug
  lines.

Profiles, the output and the split link refresh every 5 s (every 30 s while
the terminal is in the background), the batteries every minute, and the
update check uses a 6-hour cache shared with the CLI. The connection is
only held while a batch of requests runs, so a waybar module calling
`sofleii status` keeps working next to the TUI.

### Keys

![The help overlay listing the keys](/img/sofleii/tui-help.svg)

| Key | Does |
|---|---|
| `r` | Refresh everything now |
| `u` | Check for a firmware update now, past the cache |
| `↑` `↓` / `j` `k` | Select a profile |
| `d` | Debug lines in the log on/off |
| `?` | Help (`?` or `Esc` closes it) |
| `q`, `Esc`, `Ctrl-C` | Quit |
| `⏎` / `s` | Switch to the selected profile |
| `o` | Toggle the output priority USB ↔ BLE |

### Switching from the TUI

`⏎` on a profile switches to it; on the active one it just says it's
already active. `o` flips the output priority. These are the same writes as
[`profile switch`](#writes) and [`output`](#writes), with the same
read-back.

When a switch would drop the link the TUI is using, a box asks first and
says what will happen, for example:

```
┌─ Switch output? ─────────────────────────────────────────┐
│                                                          │
│ Switching output to BLE: this USB link goes quiet;       │
│ sofleii continues over BLE (verified by read-back).      │
│                                                          │
│ Continue?                                                │
└ y confirm · n / Esc cancel ──────────────────────────────┘
```

It asks for output away from the transport in use, a profile switch over
Bluetooth or with priority BLE, and a switch to an **empty** slot (that
starts pairing on it and drops this computer's Bluetooth connection). Only
`y` confirms; any other key, `⏎` included, cancels, so a double Enter
can't confirm by accident. Other switches go straight through.

Output BLE depends on the **active** profile. The keyboard only uses
Bluetooth while that profile is connected, and falls back to USB otherwise.
- **Active profile not connected:** the box says the keyboard stays on USB
  for now. Keystrokes and Studio move once that profile's host connects.
- **Active profile connected to another computer** (sofleii compares its
  address with this computer's Bluetooth adapter): keystrokes and Studio go
  to that computer, and sofleii loses the keyboard until the output is
  switched back on the keyboard or from there. The box says so, in red.
  To switch to BLE from here, first switch to this computer's profile.

While a switch runs, the bottom edge shows a spinner
(`⠋ switching to profile 1…`) and further switch keys are ignored. Then it
shows the result, e.g. `✓ profile 1 active (verified over usb)`, for a few
seconds; an error stays until you press a key. Both go to the log. When the
switch moves the connection to the other transport, the corner follows it:
`◌ ble connecting`, then `● ble …`.

### Link states

The corner of the title bar says what the connection is doing:

| Shows | Means |
|---|---|
| `○ searching` | Looking for the keyboard: no serial port, nothing connected over Bluetooth. |
| `◌ usb connecting` | Opening the port or the Bluetooth link. |
| `● usb /dev/ttyACM1 (auto)` | Connected and answering; `(auto)` when `--transport auto` picked it. |
| `◐ ble not answering` | The link opened but the keyboard doesn't reply. Almost always the [selected-output rule](#transports): over Bluetooth while USB is the selected output. Plug in USB, or `sofleii output ble`. |
| `✕ lost, retrying in 3s` | The link dropped; it retries on its own. |

When the link drops the last values stay on screen. A value older than three
refresh intervals is dimmed, and a failed read shows `! …` next to the last
good value.

In the summary:

- **Split `disconnected`** means the right half is off or out of range. The
  right battery then reads `R disconnected`, not the 0% the left half keeps
  reporting for it.
- **`R reading…`** shows for up to 10 s after the right half rejoins, while
  the left half fetches its level (until then it reports 0%). A 0% that
  lasts longer is shown as 0%.
- **Split `unknown`** means firmware older than the split-info fix; update
  it to see the link.

## CLI

Every subcommand prints text by default and the [JSON contract](#json) with
`--json`. They never start the TUI.

| Command | Does |
|---|---|
| `sofleii status` | One-shot summary: transport, output, active profile, Studio lock, split link, both batteries |
| `sofleii battery` | Both halves' battery levels (Bluetooth only) |
| `sofleii info` | Device name, serial number, the firmware build, custom RPC subsystems |
| `sofleii profiles` | The BLE profiles: active, connected, bonded, address, name |
| `sofleii update check` | The running firmware against the newest firmware [release](https://github.com/rdlu/sofleii/releases), and sofleii against the newest sofleii release |
| `sofleii keymap show [--layer N\|NAME]` | The live keymap, one grid per layer on the physical layout |
| `sofleii keymap diff` | Keys and layers that differ between the keyboard and `config/eyelash_sofle.keymap` |
| `sofleii profile switch <n>` | **Write.** Make profile `<n>` active |
| `sofleii profile rename <n> <name>` | **Write.** Name a bonded profile (at most 31 bytes; `""` clears it) |
| `sofleii profile unpair <n>` | **Write, destructive.** Remove the profile's bond; asks first unless `--yes` |
| `sofleii output usb\|ble` | **Write.** Set the output priority |
| `sofleii flash left\|right\|both` | **Write.** Flash firmware (the newest release by default), checking each half before copying |
| `sofleii flash reset left\|right\|both` | **Write, destructive.** Flash only `settings_reset` (clears settings and Bluetooth bonds) |
| `sofleii flash chips` / `flash forget <serial>` | The chips sofleii remembers by serial number; forget one |
| `sofleii flash identify [--expect left\|right]` | Which keyboard and half the mounted `NICENANO` bootloader drive is, before you flash it |

Profile numbers are **0-based**, as `sofleii profiles` lists them and as
`&bt BT_SEL n` counts.

```console
$ sofleii status
Eyelash Sofle
transport       usb /dev/ttyACM1
output          usb (priority usb)
ble profile     0 connected 00:11:22:33:44:55
bonded slots    2 of 5
studio          unlocked
split link      connected
battery         left 82%, right 61%
```

```console
$ sofleii battery
left  82%
right 61%
```

```console
$ sofleii profiles
#   active connected bonded address           name
0   *      yes       yes    00:11:22:33:44:55 desk
1          -         yes    00:11:22:33:44:66 laptop
2          -         -      -                 -
3          -         -      -                 -
4          -         -      -                 -
2 of 5 slots bonded; output priority: usb
```

```console
$ sofleii update check
up to date: v3.2.0 (built from 0858966)
running         v3.2.0 (built from 0858966)
latest          v3.2.0 96f3a2d, published 2026-10-21T21:01:59Z
note            built from 0858966; differs from v3.2.0 only outside the firmware inputs (35 files)
sofleii         up to date: v0.1.1
```

`update check` compares the firmware with the newest `sofleii-fw-vX.Y.Z`
release and sofleii itself with the newest `sofleii-tui-vX.Y.Z` release
(drafts and prereleases don't count), from one GitHub request. The text
shows plain versions (`v3.2.0`); the JSON has the full tags. Since 0.2.0
only these tag names count (they were `vX.Y.Z` and `sofleii-vX.Y.Z` until
2026-10-24), so an older sofleii finds no releases: update it. Only the **firmware
inputs** count, the same paths that trigger a firmware build (`config/`,
`build.yaml`, `zmk-rgb-layer/`, `zmk-sofleii-id/` and the build workflow): a build from
another commit that differs from the release only elsewhere (a PR branch
before its merge, docs-only commits) is up to date, shown as `v3.2.0
(built from 0858966)`. When GitHub's file list can't settle it (it stops at
300 files), the commits decide and a `note` says so. `--firmware` or `--cli`
checks just one; `--cli` doesn't need the keyboard. Answers come from the
6-hour cache the TUI shares (`~/.cache/sofleii/update.json`), with a
`checked … ago, cached` line; `--refresh` asks GitHub anyway. With no
sofleii release yet, the sofleii part is `unknown` and doesn't change the
exit code. `sofleii --version` also names the firmware line the binary was
built for (the vendored protocol pins).

```console
$ sofleii info
Eyelash Sofle
transport       usb /dev/ttyACM1
serial number   0011223344556677
studio          unlocked
firmware        v3.1.0
zmk             v0.3-198-ge5c9b691
zephyr          4.1.0
board           nice_nano
built           2026-10-19T19:02:11Z
…
```

On firmware without the build-info subsystem (v3.0.0), `info` says
`build unavailable` and still exits 0; `update check` needs it and exits 4.

### Keymap

`keymap show` reads the keymap the keyboard is running (Studio edits
included) and draws each layer on the physical layout: the tap on top, the
hold side of a home-row mod or the kind of key (`mo`, `bt`, `rgb_ug`) below,
`▽` for transparent and blank for `&none`. `--layer` takes an index or a
name. `--json` has every binding in ZMK form (`"zmk": "&hml LALT S"`), its
raw parameters and the behaviours' metadata.

```console
$ sofleii keymap show --layer NAV
1 layer(s) on the keyboard, layout "Eyelash Sofle"

── 1 NAV ──────────────────────────────────────────────────────────────────
 F12     F1     F2     F3     F4     F5                ↑                F6     F7  …

 G+U    G+I     ▽      ▽      ▽      ▽          ←     RET     →        HOME   PGDN …
```

`keymap diff` compares it with `config/eyelash_sofle.keymap`, found from the
current directory upwards (or `--keymap <path>`). Aliases don't count as
drift (`LSHIFT` is `LSHFT`, `LBRC` is `LS(LBKT)`), and neither does a layer
that only moved, as long as its `&mo`s still point at it. It exits 1 on
drift, which is what saved [Studio](../studio/#saved-edits-outlive-a-flash)
edits look like after a flash:

```console
$ sofleii keymap diff
keyboard vs /home/me/sofleii/config/eyelash_sofle.keymap

5 GAME?: 1 key differs
  pos 45  file &GAME_MY_TO              keyboard &none

only in the file: MYGAME (index 9)

drift: 1 key(s) differ on 1 layer(s), 1 layer change(s)
```

Custom behaviours get their labels (`hml`, `M_PIPE`) from that file; without
it, `show` uses the names the firmware reports (`PIPE_OP`, the macro's
`label`). What the parser can't evaluate is a note on stderr, not a failure.

### Flash

`flash` writes firmware through the nice!nano's UF2 bootloader, one half at
a time, and checks each half before it copies anything (see
[Flashing](../flashing/#which-half-is-this)). It needs no Studio connection
to flash; the left half's version is read back over Studio at the end when
it answers.

```sh
sofleii flash both                     # newest sofleii-fw release, left then right
sofleii flash left --reset             # settings_reset, then the firmware
sofleii flash right --release sofleii-fw-v3.2.0
GH_TOKEN=$(gh auth token) sofleii flash both --ci          # newest green main build
GH_TOKEN=$(gh auth token) sofleii flash left --ci feat/x   # a branch build
sofleii flash both --file eyelash_sofle_left.uf2 --file eyelash_sofle_right.uf2
sofleii flash left --adopt left        # a chip nothing vouches for yet
```

| Option | Does |
|---|---|
| `--release <tag>` | A release by tag instead of the newest one |
| `--ci [branch]` | The newest successful CI build of `branch` (default `main`). GitHub only serves build artifacts to a signed-in request: set `GH_TOKEN` or `GITHUB_TOKEN`. A branch only has a build if one was started for it (`gh workflow run build.yml --ref <branch>`) |
| `--run <id>` | One CI build run (token as for `--ci`) |
| `--file <path>` | Local `.uf2` files, repeatable. Each is identified by its content (left, right or `settings_reset`), so names and order don't matter |
| `--reset` | `settings_reset` before the firmware, on each half flashed |
| `--adopt [left\|right]` | Flash a chip nothing vouches for (a first flash, other or erased firmware), or whose firmware says it is the other half. Bare `--adopt` asks you to type the half |
| `--wait <s>` | How long to wait for each bootloader drive (default 60) |

The double-tap prompts are instructions, not questions, so `flash` runs
without a terminal too. For each half it:

1. asks for the double-tap and waits for **one** bootloader drive (two at
   once is an error: unplug the one you don't mean);
2. reads the chip's **serial number** and the firmware on it, and decides:
   a chip it remembers is flashed as its half (even over the wrong firmware)
   and refused as the other one; a new chip is flashed when its firmware
   says it is that half, refused when it says the other half or another
   keyboard, and otherwise needs `--adopt`;
3. copies the file with a progress bar, remembers the chip, and waits for
   the drive to go away (the half rebooting).

A refusal stops the job, with a hint. The chips it knows live in
`~/.local/state/sofleii/chips.json` (`$XDG_STATE_HOME`); `flash chips` lists
them and `flash forget <serial>` drops one, e.g. a controller moved to
another keyboard. `--json` prints the result (one entry per step: file, chip
serial, what the chip held, the decision, the outcome, whether it rebooted,
and the left half's `running` version); the commentary stays on stderr.

### Flash identify

`flash identify` looks at a half in bootloader mode (double-tap reset) and
says whose chip it is, from the firmware still on it: it reads the drive's
`CURRENT.UF2`, which the bootloader generates from the chip's flash. It
needs no keyboard connection and writes nothing. `flash` makes the same
check before every copy; see [Flashing](../flashing/#which-half-is-this) for what
it recognises.

```console
$ sofleii flash identify --expect left
drive       /run/media/rdlu/NICENANO
bootloader  nice!nano, UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) …
firmware    settings_reset
keyboard    eyelash_sofle
half        left
chip        94708422E5F2FD3E (remembered as your left half, verified)
expected    left half: match
note        the chip holds settings_reset; keyboard and half come from the remains of the firmware it replaced
```

The drive is found by its `INFO_UF2.TXT` among the mounted filesystems, not
by its label; `--mount <dir>` picks one when several are mounted. It exits 0
when the drive is this keyboard (and the `--expect` half), 1 when it's
another keyboard, the other half or can't be told: don't flash on 1.
`firmware` is `sofleii` (identity record), `legacy` (an older build, by its
strings), `settings_reset` or `unknown`; `verdict` in the JSON is `match`,
`mismatch` or `unknown`. `chip` is the chip's USB serial (its nRF52840 ID,
the same in bootloader and keyboard mode) and what sofleii remembers about
it; a chip is remembered once `sofleii flash` has flashed it.
The verdict itself only looks at the firmware.

### Global options

| Option | Does |
|---|---|
| `--transport auto\|usb\|ble` | Pin the transport for Studio RPC (default `auto`, see [Transports](#transports)) |
| `--device <path>` | Serial port to use instead of looking up "Eyelash Sofle"; implies USB |
| `--bt-address <MAC>` | Bluetooth device instead of the one named "Eyelash Sofle"; implies BLE |
| `--timeout <ms>` | How long to wait for each reply (default 3000) |
| `--json` / `--format human\|json\|waybar` | Output format; `waybar` is `status` and `battery` only |

Contradicting flags (`--device` with `--bt-address`, `--transport ble
--device …`) are a usage error, exit 3.

### JSON

`--json` prints a versioned contract, so scripts don't break on cosmetic
changes:

- a top-level **`"schema": 1`**; a breaking change bumps it, additions don't;
- **snake_case** keys;
- **explicit `null`** for anything unknown, never an omitted key;
- timestamps in **ISO 8601** UTC (`2026-10-19T19:11:48Z`);
- no colour or progress on stdout, ever, with `--json`; diagnostics go to
  stderr. `NO_COLOR` turns colour off in the text output too.

`sofleii status --json`:

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "device": { "name": "Eyelash Sofle", "serial_number": "0011223344556677" },
  "studio": { "locked": false },
  "output_priority": "usb",
  "max_profiles": 5,
  "bonded_profiles": 2,
  "active_profile": 0,
  "profile": { "index": 0, "active": true, "connected": true, "bonded": true,
               "address": "00:11:22:33:44:55", "name": "desk" },
  "split": { "known": true, "connected": true, "is_split": true,
             "is_central": true },
  "split_note": null,
  "battery": { "left": { "percent": 82, "state": null },
               "right": { "percent": 61, "state": null }, "note": null }
}
```

- `split.connected` is whether the right half is on the split link. On
  firmware without the split-info fix, `split.known` is `false` and the rest
  is `null`: never read that as disconnected.
- With the split link down, `battery.right` is `{"percent": null, "state":
  "disconnected"}` instead of a stale 0%.
- When Bluetooth isn't available the batteries are `null` and `note` says
  why; that never changes `status`'s exit code.

Over Bluetooth, `transport` is `{"kind": "ble", "address": "…"}`. On an
error, stdout carries an error object instead, with the same exit code:

```json
{ "schema": 1, "error": { "kind": "timeout", "message": "…", "exit_code": 4 } }
```

`kind` is one of `not_found`, `usage`, `io`, `timeout`, `protocol`, `rpc`,
`bluez`, `unsupported`, `github`. The fields of every command are described
field by field in the [crate README](https://github.com/rdlu/sofleii/blob/main/sofleii/README.md).

### Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Attention needed: `update check` with the firmware or sofleii behind (or a firmware build it can't place), `battery --warn-below` with a half below it, `keymap diff` with drift, `flash identify` without a match, `flash` refused or stopped at a chip |
| 2 | Keyboard not found or not connected; for `flash identify`, no bootloader drive mounted; for `flash`, none within `--wait` |
| 3 | Usage error: a bad or contradicting flag, a profile out of range, a name too long, `unpair` not confirmed, no layer by that name, a missing or unparseable `.keymap`, several bootloader drives without `--mount` (or at once for `flash`), `--ci` without a token, `--adopt` naming a half not flashed |
| 4 | Transport or protocol error: timeout, a busy port, an RPC error, a BlueZ or GitHub failure |

So checks need no parsing:

```bash
sofleii update check || notify-send "Sofle firmware update"
sofleii keymap diff >/dev/null || notify-send "Sofle keymap differs from the repo"
sofleii battery --warn-below 20 || notify-send "Sofle battery low"
```

### waybar

`--format waybar` prints one line of waybar custom-module JSON. It **still
prints valid JSON** when the keyboard is missing (class `disconnected`), so
the bar keeps rendering, and in waybar mode a low battery never exits 1.

```jsonc
"custom/sofleii": {
  "exec": "sofleii status --format waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{}",
  "on-click": "foot -e sofleii",
  "tooltip": true
},
"custom/sofleii-battery": {
  "exec": "sofleii battery --format waybar",
  "return-type": "json",
  "interval": 300,
  "format": "{}",
  "tooltip": true
}
```

```css
#custom-sofleii.disconnected,
#custom-sofleii-battery.disconnected { opacity: 0.5; }
#custom-sofleii-battery.warning { color: #ebcb8b; }
#custom-sofleii-battery.critical { color: #bf616a; }
```

- `status`: the text is an output icon plus the active profile (`󰕓 0`);
  `class`/`alt` are `usb`, `ble` or `disconnected`; the tooltip is the full
  summary.
- `battery`: `󰌌 L 82 R 61`; `class` is `ok`, `warning` (below
  `--warn-below`, default 20), `critical` (below `--critical-below`, default
  10), `unknown` or `disconnected`; `percentage` is the lower half.
- The icons are Nerd Font glyphs; use `format-icons` with `{alt}` for others.
- `on-click` opens the TUI in a terminal; swap `foot` for yours.

## Transports

`--transport auto` (the default) uses **USB** if the "Eyelash Sofle" serial
port exists and answers within a second, else **Bluetooth** if BlueZ has
the keyboard connected, else exits 2 with the reason for both. A few things
to know:

- **Studio only answers on the selected output.** The firmware listens for
  Studio RPC on whichever endpoint keystrokes currently go to. With USB
  plugged in and output priority `usb`, Bluetooth requests are silently
  dropped (they time out, exit 4), and with Bluetooth selected the USB port
  goes deaf, which is why `auto` falls back to Bluetooth. Over Bluetooth,
  this computer also has to be the **active profile**.
- **Only the left half does USB.** The right half's USB port only charges,
  so a cable in the right half looks like no USB at all; `status` says so.
- **The serial port is exclusive.** Only one sofleii holds it at a time. A
  busy port is retried briefly, then fails with "port busy" (exit 4). The
  TUI only holds it while requests run (about 1.5 s idle, then it lets go),
  so waybar and one-off commands get their turn.
- **`battery` is always Bluetooth**, whatever `--transport` says: the
  batteries come from BlueZ GATT. BlueZ's own battery reading (and upower,
  and most desktop widgets) only sees the left half; sofleii reads both.
- sofleii never pairs, unpairs, connects or disconnects Bluetooth devices on
  the host. The `profile unpair` write removes a bond **on the keyboard**.

## Writes

`profile switch|rename|unpair` and `output` change the keyboard, so they
check first and confirm after:

- **Read, write, read back.** Each reads the current state (a profile past
  the last slot is exit 3), sends the write, and reads the result back. A
  read-back that contradicts the write is exit 4.
- **`unpair` always confirms.** It asks `[y/N]` unless you pass `--yes`, and
  with no terminal and no `--yes` it refuses before connecting (exit 3).
  The host on that slot has to pair again.
- **The handover.** Because Studio follows the selected output, some writes
  move Studio to the *other* transport before the reply goes out: `output
  ble` over USB, `output usb` over Bluetooth, `profile switch` over
  Bluetooth. There the missing reply is expected, not an error: sofleii
  opens the other transport, reads the result back there, and says so.

```console
$ sofleii output ble          # over USB, with a BLE profile connected
output priority: ble (was usb)
studio rpc now answers on ble
note: no reply over usb (expected: the firmware hands Studio over to ble before replying); further RPC needs --transport ble (or auto); read back over ble
```

If there's no other transport to read back on, the write still counts
(exit 0) and the JSON has `"verified": false` with a `note`. After a
handover, run the next command with `--transport auto` (the default) or the
new transport.

## Roadmap

Done: the CLI above, the live TUI dashboard with profile and output
switching, the BLE transport, both
batteries ([#45](https://github.com/rdlu/sofleii/issues/45)), the build info
([#46](https://github.com/rdlu/sofleii/issues/46)), the update check, and
`keymap show`/`keymap diff` ([#76](https://github.com/rdlu/sofleii/issues/76), CLI part).
Coming, roughly in this order (epic: [#44](https://github.com/rdlu/sofleii/issues/44)):

- [#83](https://github.com/rdlu/sofleii/issues/83): the **first release**,
  `sofleii-tui-v0.1.0`: CI, prebuilt binaries, install with mise or cargo, and
  `update check` checking the CLI too.
- [#76](https://github.com/rdlu/sofleii/issues/76): the **live keymap in the
  TUI**, a layer at a time with the drifted keys highlighted.
- [#77](https://github.com/rdlu/sofleii/issues/77): **live keymap editing**:
  bindings, layers and the encoder; apply, save, discard.
- [#78](https://github.com/rdlu/sofleii/issues/78): **snapshots**: export
  live changes to a file and restore them.
- [#79](https://github.com/rdlu/sofleii/issues/79): a **custom game slot**
  on the game picker, with sofleii game presets.
- [#80](https://github.com/rdlu/sofleii/issues/80): **bake** live changes
  into the firmware: write them into the `.keymap`, draw, PR, flash.
- [#100](https://github.com/rdlu/sofleii/issues/100): `sofleii flash` is in;
  a guided **Flash mode in the TUI** comes next.
- Later: the repo's mise routines as commands ([#48](https://github.com/rdlu/sofleii/issues/48)).

Until editing lands, use [Studio](../studio/) for live keymap edits.
