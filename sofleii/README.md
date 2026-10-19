# sofleii

**Sofle Interactive Interface**: a command-line tool and a ratatui TUI (a
live, read-only dashboard so far) that talk to the Eyelash Sofle over the **ZMK Studio RPC**
protocol, the same protocol the Studio web app uses, without a browser.
Design and roadmap: [#44](https://github.com/rdlu/sofleii/issues/44).

Status: **spike.** Studio RPC over USB serial (left half) or BLE GATT
through BlueZ; batteries over BLE GATT; update checks over the GitHub REST
API; reads, plus the profile and output-priority writes (not yet tested
against a keyboard, see [Writes](#writes)).

## Build and run

Needs Rust (edition 2024, 1.99+) and `protoc`, both from pacman
(`rust`, `protobuf`). Dependencies follow their latest stable releases and
`rust-version` follows the current stable rustc: no MSRV holdbacks. From
the repo root:

```bash
mise run sofleii -- status          # build (release) and run
mise run sofleii -- profiles --json
mise run sofleii-test               # cargo test + clippy -D warnings + fmt --check
```

Or directly: `cd sofleii && cargo run -- info`.

`battery` and the BLE transport talk to BlueZ through `zbus` 6 (pure Rust,
each call run with `zbus::block_on` on zbus's own runtime): no libdbus, no
tokio, nothing extra to install beyond a running `bluetooth.service`.

Over USB the keyboard is found by its USB product string `Eyelash Sofle` (the
VID/PID is shared by every ZMK board, and the `/dev/ttyACM*` number is not
stable). `--device /dev/ttyACMn` overrides the lookup. Your user needs access
to the port (the `uucp` group on Arch). Over BLE, see
[Transports](#transports).

## Commands

| Command | What it reads |
|---|---|
| `sofleii status [--json] [--format waybar]` | One-shot summary: transport, device, Studio lock, active BLE profile (connected, bonded, address, name), output priority, the split link, both batteries. Built from `get_device_info`, `get_lock_state` and `cormoran_ble` `get_profiles` + `get_output_priority` (`get_profiles` doesn't carry the priority) + `get_split_info`, plus the `battery` reader with a 1 s timeout. See [JSON](#json) for `split`. |
| `sofleii battery [--json] [--format waybar] [--warn-below <pct>] [--critical-below <pct>]` | Both halves' levels over BLE GATT (BlueZ, system D-Bus), not Studio RPC. See [Battery](#battery). |
| `sofleii info [--json]` | `core.get_device_info` (name, serial number), Studio lock state, `custom.list_custom_subsystems`, and the firmware build from the `zmk__device_info` subsystem (#46, looked up by identifier): versions, config commit + dirty flag, build time, board, build hash, hardware id and reset cause. See [`info`](#info). |
| `sofleii profiles [--json]` | `cormoran_ble.get_profiles` and `get_output_priority` (the subsystem index is looked up by identifier, never hardcoded). |
| `sofleii profile switch <n> [--json]` | **Write.** `cormoran_ble.switch_profile`, then `get_profiles` to read the active profile back. See [Writes](#writes). |
| `sofleii profile rename <n> <name> [--json]` | **Write.** `set_profile_name` on a bonded slot (at most 31 UTF-8 bytes; `""` clears the name), read back. |
| `sofleii profile unpair <n> [--yes] [--json]` | **Write, destructive.** `unpair_profile`: removes the slot's bond. Asks first unless `--yes`. |
| `sofleii output usb\|ble [--json]` | **Write.** `set_output_priority`, read back with `get_output_priority`, over the other transport if Studio moved there. |
| `sofleii update check [--json] [--repo OWNER/NAME]` | The running build (`zmk_config_version` from `zmk__device_info`) against the latest GitHub release of `--repo` (default `rdlu/sofleii`), shown as release versions (`v3.1.0`, `v3.0.0+68`): up to date, behind (update available), ahead, or unknown. See [`update check`](#update-check). |

Global options: `--transport auto|usb|ble` (default `auto`, see
[Transports](#transports)), `--device <path>` (serial port for Studio RPC;
implies USB), `--bt-address <MAC>` (BLE device instead of the BlueZ device
named `Eyelash Sofle`, for the batteries and Studio RPC; implies BLE),
`--timeout <ms>` per response (default 3000), `--format
human|json|waybar` (default `human`; `--json` is the same as `--format json`;
`waybar` is `status` and `battery` only, a usage error elsewhere). With no
subcommand, sofleii opens the [TUI](#tui) when stdout is a terminal
and prints usage (exit 0) when it isn't; `--json`/`--format` without a
subcommand is a usage error (exit 3).

```
$ sofleii status
Eyelash Sofle
transport       usb /dev/ttyACM1
output          usb (priority usb)
ble profile     0 connected 84:9E:56:52:6A:D2
bonded slots    1 of 5
studio          unlocked
split link      connected
battery         left 100%, right 71%
```

When BLE isn't available, the battery row reads `left -, right -
(unavailable: …)` with the reason, and `status` still exits 0.

`split link` comes from `cormoran_ble`'s `get_split_info`: `connected`,
`disconnected (right half off or out of range)`, or `unknown (firmware
without the split-info fix)`. The last is cormoran's module before our fork's
fix (`rdlu/zmk-module-ble-management` `5ea5b4f`), which answers with no
`info` at all; sofleii never reads that as disconnected. With the right half
off, the central's battery proxy keeps reporting 0% for it, so when the link
is known to be down the battery row reads `right disconnected` instead of
`right 0%` (an unknown link keeps the level as read). The waybar tooltip has
the same `split link:` and `battery:` lines.

`output` is the endpoint keystrokes most likely go to: the output priority,
with ZMK's fallback to the other endpoint when the preferred one isn't
connected. Over USB, USB counts as connected. Over BLE, sofleii can't see
whether USB is plugged in, so a connected active profile means `ble`
(whatever the priority). The firmware agrees: Studio only answers on the
selected endpoint, so a reply over BLE means BLE is selected. With priority
`usb` the row then reads `ble (priority usb); USB not connected to the left
half (the right half's USB only charges)`: only the central (left) half does
USB (ZMK's `ZMK_USB` needs `!ZMK_SPLIT || ZMK_SPLIT_ROLE_CENTRAL`), so a cable
in the right half looks like no USB at all. The JSON keeps the plain
`output_priority`.

## TUI

`sofleii` with no subcommand, on a terminal (`mise run sofleii`), live over
USB:

```
┌─ sofleii · Eyelash Sofle ──────────────────────── ● usb /dev/ttyACM0 (auto) ─┐
│ Transport  USB /dev/ttyACM0 (auto)           Output     USB  (priority usb)  │
│ Profile    0  connected                      Studio     unlocked             │
│ Battery    L ██████████ 100%   R ███████░░░  71%                 read 5s ago │
│ Firmware   v3.1.0 (2474a13) · up to date with v3.1.0          checked 5s ago │
│ Split      connected                                                         │
├─ Profiles ───────────────────────────────────────────────────────────────────┤
│ ▶ 0  —        84:9E:56:52:6A:D2   active · connected · bonded                │
│   1  —                            empty                                      │
│   2  —                            empty                                      │
├─ Log (info) ─────────────────────────────────────────────────────────────────┤
│ 19:07:50 connected over usb /dev/ttyACM0 (auto)                              │
└ q quit · r refresh · u update check · ↑↓/jk select · d debug · ? help ───────┘
```

- **Live, read-only.** The TUI runs the core's session (`src/session/`)
  with the global `--transport`, `--device`, `--bt-address` and `--timeout`
  (contradicting flags exit 3 before the screen opens), and renders a view
  derived from its `Snapshot`. Profiles, output priority and the split link
  are polled every 5 s (30 s while the terminal is unfocused, through focus reporting), the
  batteries every 60 s over BlueZ, and the update check uses the shared 6 h
  cache. The RPC link is leased, so `sofleii status` (waybar) still works
  while the TUI runs. `SOFLEII_TUI_DEMO=1` shows a fixture instead, with no
  session (for screenshots).
- **Link state** in the top-right corner: `○ searching`, `◌ connecting`,
  `● usb /dev/ttyACM0 (auto)`, `◐ ble not answering` (the hint shows in the
  summary until data arrives), `✕ lost, retrying in 3s`. The session only
  reports a link as connected once a request has answered on it, so a link
  that opens but stays deaf (`--transport ble` while USB is the selected
  output) shows a steady "not answering"; retaking the lease on a healthy
  link changes nothing. When the link drops,
  the last values stay on screen.
- **Freshness:** a value older than 3 refresh intervals is dimmed, and the
  status's age goes on the Profiles separator. A failed read shows its error
  as `! …` beside the last good value (cut to fit; the Log pane has more).
- **Log pane:** the session's log and the core's `log::warn!`/`debug!`
  records, merged by time, in local `HH:MM:SS`; a run of the same line shows
  once with `(×N)`. It shows info and up by default (`Log (info)`); `d`
  toggles the debug lines (`Log (debug)`). The CLI prints the same core records as
  `sofleii: warning: …` on stderr.
- **Keys:** `r` refreshes everything now, `u` checks for a firmware update
  now (past the cache), `↑↓`/`jk` select a profile, `d` debug lines in the
  log on/off, `?` help, `q`, `Esc` or
  `Ctrl-C` quit (`Esc` closes the help first). Switching profile (`⏎`) and
  output (`o`) are in the help as coming soon. Quitting is instant: no
  thread is joined.
- **Split** is polled with the profiles: `connected`, `disconnected (right
  half off or out of range)` or `unknown (firmware without the split-info
  fix)` (shorter below 70 columns). While it's disconnected the battery row
  shows `R disconnected` instead of the proxy's stale 0%.
- Square panes as on the docs site; below 70 columns the summary is one
  column. Battery bars use `battery`'s defaults (warning below 20%,
  critical below 10%). `NO_COLOR` turns colour off (bold and dim remain).
- The terminal is restored on quit, on SIGTERM/SIGHUP/SIGINT, and on a
  panic (ratatui's panic hook runs before the release profile's
  `panic = "abort"`).

## Transports

`--transport auto` (the default) picks:

1. **USB** if a serial port with product string `Eyelash Sofle` exists. It
   gets 1 s to answer the first request (`get_device_info`). If it doesn't
   and the keyboard is connected over BLE, sofleii says so on stderr and
   uses BLE (see the firmware limit below).
2. Else **BLE** if BlueZ has the keyboard connected.
3. Else exit 2, with the reason for both (no serial port; not known to
   BlueZ, not connected, BlueZ not running…).

`--device` implies USB and `--bt-address` implies BLE. Giving both, or
contradicting `--transport` (`--transport usb --bt-address …`,
`--transport ble --device …`), is a usage error (exit 3). `battery` always
reads over BlueZ and ignores `--transport`/`--device`.

The serial port is opened exclusively (TIOCEXCL plus `flock`), so only one
sofleii can hold it. A busy port is retried 3 times, 100 ms apart, then
fails with `port busy (another sofleii, e.g. the TUI or waybar, has it
open)`, exit 4.

Request ids start at a per-process value (pid mixed with the clock), not 1,
so two sofleii processes on the same BLE link don't take each other's
replies.

JSON `transport` is `{"kind": "usb", "device": "/dev/ttyACM1"}` or
`{"kind": "ble", "address": "DD:1F:23:76:6E:71"}` (the BLE shape is new,
so no schema bump).

### BLE requirements (Linux/BlueZ)

- BlueZ running (`bluetooth.service`), and the keyboard **already bonded
  and connected** to this host (it normally is, as a HID keyboard). sofleii
  reuses that connection and never pairs, unpairs, connects or disconnects.
  The device is the connected BlueZ `Device1` named `Eyelash Sofle` (`Name`
  or `Alias`), or `--bt-address`.
- **This host must be the keyboard's active BLE profile, and BLE must be
  the selected output.** ZMK Studio only listens on the transport of the
  selected output endpoint (`refresh_selected_transport` in
  `zmk/app/src/studio/rpc.c`). While USB is connected with output priority
  `usb`, the firmware accepts BLE writes and silently drops them, so BLE
  requests time out (exit 4; the error says why). Responses also go only
  to the active profile's connection. The same rule makes USB deaf while
  BLE is selected, which is why `auto` falls back to BLE.
- No Chrome flags or other setup.

### How it talks (BlueZ D-Bus)

- Studio service `00000000-0196-6107-c967-c5cfb1c2482a`, RPC characteristic
  `00000001-0196-6107-c967-c5cfb1c2482a`, flags `read, write, indicate`.
  Framing is the same SOF/ESC/EOF as over USB. Responses arrive as
  indications of at most 27 bytes, so a frame can span several and one can
  carry the end of a frame and the start of the next. The transport turns
  them into a byte stream for the same decoder and request-id matching.
- **Receiving:** `AcquireNotify` (it works on this indicate-only
  characteristic): a `SOCK_SEQPACKET` fd plus the MTU, one indication per
  read, with `--timeout` as the read deadline. If BlueZ refuses it, the
  fallback is `StartNotify` plus `PropertiesChanged` signals on `Value`
  (`SOFLEII_BLE_NOTIFY=signals` forces the fallback, for debugging). The fd
  is closed, or `StopNotify` called, on exit. Subscribing writes the CCCD,
  which is all it changes on the keyboard.
- **Sending:** `WriteValue(bytes, {"type": "request"})`, split to the ATT
  MTU. `AcquireWrite` needs a `write-without-response` characteristic, and
  Studio's isn't one.
- **The Studio service shows up twice** in BlueZ's tree (here at handles
  0x2f and 0x35). The one at 0x2f is a stale entry in BlueZ's GATT cache,
  left over from an older firmware layout: HID's control point (handle
  0x33) sits inside its handle range, which a live GATT database can't
  have, because the current firmware's HID service grew a third report and
  pushed Studio to 0x35. sofleii marks a Studio service *stale* when
  another service's characteristic falls between its handle and the next
  service's, and uses the only live one without probing. If several look
  live, it opens each in handle order and sends a read-only
  `get_device_info` probe (1 s each); the first to answer wins.
  (Removing the device from BlueZ and pairing again would clear the stale
  copy, but sofleii never does that.)

## JSON

`--json` prints a versioned contract: top-level `"schema": 1`, snake_case
keys, `null` for anything unknown (never omitted). Breaking changes bump
`schema`. Diagnostics always go to stderr; stdout has no colour unless it is a
terminal, and never with `--json` or `NO_COLOR`.

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "max_profiles": 5,
  "active_profile": 0,
  "output_priority": "usb",
  "profiles": [
    { "index": 0, "active": true, "connected": true, "bonded": true,
      "address": "84:9E:56:52:6A:D2", "name": null },
    { "index": 1, "active": false, "connected": false, "bonded": false,
      "address": null, "name": null }
  ]
}
```

`status --json` (same envelope; `profile` has the `profiles` entry shape):

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "device": { "name": "Eyelash Sofle", "serial_number": "94708422E5F2FD3E" },
  "studio": { "locked": false },
  "output_priority": "usb",
  "max_profiles": 5,
  "bonded_profiles": 1,
  "active_profile": 0,
  "profile": { "index": 0, "active": true, "connected": true, "bonded": true,
               "address": "84:9E:56:52:6A:D2", "name": null },
  "split": { "known": true, "connected": true, "is_split": true,
             "is_central": true },
  "split_note": null,
  "battery": { "left": { "percent": 100 }, "right": { "percent": 71 },
               "note": null }
}
```

- `split` is `cormoran_ble`'s `get_split_info`. `connected` is whether the
  right half is on the split link (`null` unless the answering half is a
  split central). On firmware without the split-info fix the reply carries
  no `info`: `split` is `{"known": false, "connected": null, "is_split":
  null, "is_central": null}` and `split_note` is `"unknown: …"`, never a
  disconnected link. `split` is `null` only when it couldn't be read (no
  `cormoran_ble`, an RPC error), with `split_note` saying why; a failed split
  read never fails `status`.
- With `split.connected` `false`, `battery.right` is `{"percent": null,
  "state": "disconnected"}` rather than the central proxy's stale 0%. `state`
  appears only then; with the link up or unknown the levels are as read, and
  `sofleii battery`'s JSON never has it (see [Battery](#battery)).
- `battery` comes from the same reader as `sofleii battery`, with a timeout
  of at most 1 s. When BLE isn't available (keyboard not connected over BLE
  to this host, BlueZ not running, a GATT error), `left`/`right` stay `null`
  and `note` is `"unavailable: <reason>"`; the battery never changes
  `status`'s exit code. `note` may also carry a remark from the reader (see
  below) while the levels are filled.
- Without a `cormoran_ble` subsystem, the profile fields and
  `output_priority` are `null` rather than an error.

### `info`

The firmware build comes from zmk-feature-device-info's `zmk__device_info`
custom subsystem (left half, since #46). `build` mirrors its `BuildInfo` and
`hardware` its `HardwareInfo`, with empty strings as `null`. On a #46 build
(values from a local build; the subsystem index is whatever the firmware
lists):

```
$ sofleii info
Eyelash Sofle
transport       usb /dev/ttyACM1
serial number   94708422E5F2FD3E
studio          unlocked
firmware        v3.0.0-65-g1e468b7
zmk             v0.3-198-ge5c9b691
zephyr          4.1.0
board           nice_nano
built           2026-10-17T20:50:06Z
build hash      1e37f75107aeb7993596e9d27b668701ed790839
device id       94708422e5f2fd3e
reset cause     pin (0x1)
subsystems
   0  cormoran_ble
   1  cormoran_rsr
   2  zmk__device_info
```

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "device": { "name": "Eyelash Sofle", "serial_number": "94708422E5F2FD3E" },
  "build": {
    "available": true, "note": null,
    "zmk_version": "v0.3-198-ge5c9b691", "zmk_dirty": false,
    "zmk_config_version": "v3.0.0-65-g1e468b7", "zmk_config_dirty": false,
    "module_version": "8cedd2d", "module_dirty": false,
    "zephyr_version": "4.1.0", "build_timestamp": "2026-10-17T20:50:06Z",
    "board": "nice_nano",
    "build_hash": "1e37f75107aeb7993596e9d27b668701ed790839"
  },
  "hardware": { "device_id": "94708422e5f2fd3e", "reset_cause": 1,
                "reset_causes": ["pin"], "flash_size_kb": 1024,
                "sram_size_kb": 256 },
  "studio": { "locked": false },
  "custom_subsystems": [
    { "index": 0, "identifier": "cormoran_ble", "ui_urls": ["…"] },
    { "index": 1, "identifier": "cormoran_rsr", "ui_urls": ["…"] },
    { "index": 2, "identifier": "zmk__device_info", "ui_urls": ["…"] }
  ]
}
```

- `firmware` (`zmk_config_version`) is `git describe --dirty --always --tags`
  of **this repo** when the build was configured: `v3.0.0-65-g1e468b7` from a
  local build, the bare short hash (e.g. `1e468b7`) from CI, whose checkout is
  shallow and has no tags. Exactly on a tag, a local build gives just the
  tag. A `-dirty` build had uncommitted changes; the human output shows it as
  `(dirty)`. The other `*_version` fields are the same `git describe` of ZMK
  and of the module; each `*_dirty` is `null` when its version is.
- `build_timestamp` is UTC, taken at configure time, not link time.
- `build_hash` is the GNU build-id (SHA-1 of the linked image): it names the
  exact binary, which the git versions alone don't.
- `reset_cause` is the raw `hwinfo` bitmask and `reset_causes` its Zephyr
  `RESET_*` names. On the nRF52840 the register accumulates until cleared, so
  more than one cause can show.
- **Older firmware** (no `zmk__device_info`, like v3.0.0): `build` and
  `hardware` are all `null`, `build.available` is `false` and `build.note`
  says why. `info` still **exits 0**, because the rest of it (name, lock
  state, subsystems) was read fine; scripts that need the build check
  `build.available`. When the subsystem is there but its call fails (an
  `error` reply, an `unlock_required`, a bad payload), `info` exits 4.

Real output from a v3.0.0 left half (2026-10-17):

```
$ sofleii info
Eyelash Sofle
transport       usb /dev/ttyACM1
serial number   94708422E5F2FD3E
studio          unlocked
build           unavailable: this firmware has no zmk__device_info subsystem (built before #46); flash a newer build
subsystems
   0  cormoran_ble
   1  cormoran_rsr
```

### `update check`

Reads `zmk_config_version` from the keyboard (as `info` does), then asks the
GitHub REST API, without `gh`:

1. `GET /repos/{repo}/releases/latest` for `tag_name` and `published_at`,
   then `GET /repos/{repo}/commits/{tag}` for the tag's commit.
2. The running commit comes from the version string: the SHA in
   `v3.0.0-68-gf9a665f`, a bare `5c53b30` (CI builds), or, exactly on a tag
   (`v3.1.0`), that tag resolved through the API. An empty version (CI
   builds before the v3.1.0 fix) or a tag GitHub doesn't know is `unknown`.
3. If the running SHA is a prefix of the release's, it's **up to date**.
   Otherwise `GET /repos/{repo}/compare/{running}...{latest}`: the release
   is the head, so GitHub's `ahead_by` is how far the keyboard is
   **behind**. GitHub's `diverged` (a branch build off an older release)
   is `behind` too: the version decides, so `v3.0.0+9` is behind `v3.1.0`.
   Its own commits stay in `commits_ahead`, and `note` says `also has N
   commits not in v3.1.0 (unreleased changes)`. A 404 there means the commit isn't on GitHub (an unpushed
   local build): `unknown`.
4. When the compare changes **no files**, the status is `up_to_date` even
   though the counts aren't 0. That's a PR-branch build whose branch was
   merged as is: the release only adds the merge commit, and the firmware
   is the same. `note` says so.
5. Then it names the **release the build is, or follows**
   (`running.release`). **The firmware can't report its own tag:** CI
   builds the merge commit before the release is tagged, from a shallow
   checkout with no tags, so every CI build reports a bare SHA. sofleii
   maps it back, cheapest way first (anonymous GitHub allows 60 requests an
   hour):
   - `up_to_date`, same-tree rule included: the latest release, exact. No
     extra request.
   - `ahead`: the latest release, `commits_after` = `commits_ahead`. No
     extra request.
   - A local `git describe` string already names the tag and the count
     (`v3.0.0-68-gf9a665f` = `v3.0.0+68`; `v3.1.0` = exact). No extra
     request; this also covers `unknown`.
   - `behind` with a bare SHA: `GET
     /repos/{repo}/releases?per_page=10` (drafts and prereleases skipped,
     the latest one too, since it was already compared), then newest
     first `compare/{tag}...{running}` until one fits. `identical` or no
     changed files gives exact, and the build being ahead gives
     `commits_after` = `ahead_by`. At most 10 releases. If none fits, or
     that lookup fails (e.g. rate limiting), `release` is `null` (with a
     `note` for a failure) and the verdict stands.

The text leads with versions; the commit counts are in the JSON. A
`-dirty` build is compared by its commit, with `running.dirty: true` and a
note in the text output. Real output (2026-10-19; the left half runs the
`feat/device-info` CI build at `f9a665f`, which PR #70 merged unchanged as
`5c53b30` = v3.1.0):

```
$ sofleii update check
up to date: v3.1.0 (f9a665f)
running         v3.1.0 (f9a665f)
latest          v3.1.0 5c53b30, published 2026-10-19T19:11:48Z
note            same source as v3.1.0: the commits between f9a665f and 5c53b30 change no files
```

Other verdicts: `v3.1.0 available, running v3.0.0+68 (f9a665f)`, `ahead of
v3.1.0: running v3.1.0+3 (abc1234)`, `can't compare with v3.1.0: running f9a665f (no
release)` (with the reason as a `note`). `(no release)` replaces the
version when no release precedes the build.

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM0" },
  "running": { "version": "f9a665f", "commit": "f9a665f", "dirty": false,
               "release": { "tag": "v3.1.0", "exact": true, "commits_after": 0 } },
  "latest": { "tag": "v3.1.0",
              "commit": "5c53b30fce99031611442b1a3ccbbe7052327020",
              "published_at": "2026-10-19T19:11:48Z" },
  "status": "up_to_date",
  "commits_behind": 1,
  "commits_ahead": 0,
  "note": "same source as v3.1.0: the commits between f9a665f and 5c53b30 change no files"
}
```

- `status`: `up_to_date`, `behind`, `ahead` or `unknown` (#47's schema).
  `commits_ahead` > 0 on `behind` means the build also has unreleased
  commits.
  `commits_behind` / `commits_ahead` are `null` only when `unknown`.
- `running.version` is the raw string, `-dirty` included. `running.commit`
  is the abbreviated SHA from it, or the full SHA an exact tag resolved to.
  `latest.commit` is always the full SHA.
- `running.release`: `{"tag", "exact", "commits_after"}` (`commits_after`
  is 0 when `exact`), or `null` when no release precedes the build.
- `note` explains an `unknown` status, a same-source `up_to_date`, or a
  failed release lookup; otherwise `null`.
- **Exit codes:** 0 `up_to_date` or `ahead`; **1** `behind` or
  `unknown` (worth a look; usable as `sofleii update check || notify-send
  "Sofle firmware update"`); 2 keyboard not found (checked before any
  network call); 3 usage (e.g. a malformed `--repo`); **4** transport or
  protocol error, firmware **without** `zmk__device_info` (`kind:
  "unsupported"`; unlike `info`, which exits 0 there), or a GitHub failure
  (`kind: "github"`: network, a 404 release for `--repo`, a rejected token,
  rate limiting).
- **Token:** none needed for public releases. `GH_TOKEN`, else
  `GITHUB_TOKEN`, is sent as a bearer token if set, which lifts the
  anonymous limit of 60 requests per hour (a check usually makes 2 to 4;
a bare SHA that's behind adds up to 11 for the release lookup). A rate
  limit error says so. Requests send `User-Agent: sofleii/<version>` and
  time out after 10 s each.
- HTTPS is `ureq` (blocking, rustls with bundled roots): no tokio, no
  OpenSSL. Proxies come from the usual `HTTPS_PROXY` / `ALL_PROXY` /
  `NO_PROXY` variables.

On failure with `--json`, stdout carries an error object instead:

```json
{ "schema": 1, "error": { "kind": "timeout", "message": "…", "exit_code": 4 } }
```

`kind` is one of `not_found`, `usage`, `io`, `timeout`, `protocol`, `rpc`,
`bluez`, `unsupported`, `github`.

## Writes

`profile switch|rename|unpair <n>` and `output usb|ble` go through the
`cormoran_ble` custom subsystem (looked up by identifier). `<n>` is
**0-based**, as `profiles` lists it and `&bt BT_SEL n` counts. Each one
reads the state first (range check: `<n>` past the keyboard's slots is
exit 3), sends the write, and reads the state back.

- `rename`: the name is at most **31 UTF-8 bytes** (nanopb `max_size:32` in
  the module's `ble_management.options` counts the NUL; a longer one fails
  the firmware's decode), checked before connecting (exit 3). Control
  characters are rejected too. `""` clears the name. The firmware keys
  names by the bond's address, so renaming an empty slot is exit 3.
- `unpair` is destructive: it asks `[y/N]` on stderr unless `--yes`. With
  no terminal on stdin and no `--yes` it refuses before connecting (exit
  3); answering anything but `y`/`yes` cancels (exit 3, nothing sent). An
  empty slot is a no-op (`sent: false`, exit 0). The firmware selects the
  slot, clears its bond and re-selects the active profile.
- There is no `forget_split_bond` command on purpose: it clears every bond,
  the split pairing included.
- A `success: false` or `error` reply is exit 4 (`kind: "rpc"`); a read-back
  that contradicts an acknowledged write is exit 4 (`kind: "protocol"`).

### Endpoint handover

**Studio RPC moves with the selected endpoint, and the reply moves with
it.** The handler runs in the RPC thread and changes the endpoint
(`zmk_endpoint_set_preferred_transport`, or `zmk_ble_prof_select` →
`zmk_ble_active_profile_changed`). That raises `zmk_endpoint_changed`
synchronously, and `studio_rpc_listener_cb` calls
`refresh_selected_transport` before `send_response`
(`zmk/app/src/studio/rpc.c`, `zmk/app/src/endpoints.c`). So the reply goes
out on the **new** transport, and the one sofleii sent on goes silent. Over
BLE, replies also only go to the active profile's connection
(`gatt_rpc_transport.c`).

| Write, over | Hands over when | Then |
|---|---|---|
| `output ble`, over USB | the active BLE profile is connected | Studio answers on BLE only |
| `output usb`, over BLE | USB is enumerated by a host | Studio answers on USB only |
| `profile switch <other>`, over BLE | always (the reply goes to the new profile's connection) | BLE RPC from this host stops |
| `profile switch <other>`, over USB with priority `ble` | the new profile is connected | Studio answers on BLE only |
| `profile unpair <active>`, over BLE | always (the bond, and this connection, go) | BLE RPC from this host stops |

For these writes a reply timeout is **expected, not an error**. sofleii
then opens the other transport (USB if the `Eyelash Sofle` port exists; BLE
if BlueZ has the keyboard connected) and reads the state back there:

- read back and matching: success, `verified: true`, `verified_over` the
  other transport, exit 0;
- no other transport, or the read fails: still **success** (`ok: true`,
  exit 0) with `verified: false` and a `note` saying why;
- read back and contradicting: exit 4.

When the reply *does* arrive after `output <other>`, the firmware kept the
current transport because the other isn't ready; `note` says so (the
priority is set, and applies once it connects). In every other case a
timeout is a real timeout (exit 4). After a handover, run the next command
with `--transport <other>` or `auto`.

```
$ sofleii output ble          # over USB, BLE profile connected
output priority: ble (was usb)
studio rpc now answers on ble
note: no reply over usb (expected: the firmware hands Studio over to ble before replying); further RPC needs --transport ble (or auto); read back over ble
```

`output --json`:

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "action": "output",
  "output_priority": "ble",
  "previous_output_priority": "usb",
  "ok": true,
  "reply": false,
  "verified": true,
  "verified_over": "ble",
  "studio_transport": "ble",
  "note": "no reply over usb (expected: …); read back over ble"
}
```

`profile … --json` (`action` is `profile_switch`, `profile_rename` or
`profile_unpair`; `slot` has the `profiles` entry shape):

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM1" },
  "action": "profile_switch",
  "profile": 1,
  "name": null,
  "ok": true,
  "sent": true,
  "reply": true,
  "verified": true,
  "verified_over": "usb",
  "previous_active_profile": 0,
  "active_profile": 1,
  "slot": { "index": 1, "active": true, "connected": false, "bonded": true,
            "address": "AA:BB:CC:DD:EE:FF", "name": null },
  "studio_transport": "usb",
  "note": null
}
```

- `ok` is always `true`: a failed write prints the error object instead.
- `sent`: `false` only for `unpair` on an empty slot. `reply`: the reply
  arrived on this transport. `studio_transport`: where Studio answers now,
  when known (`null` after a silent switch that couldn't be read back).
- `name` is the new name for `rename` (`null` when cleared, and for the
  other actions). `active_profile` and `slot` are the read-back, `null`
  when not verified.

## Battery

Battery levels aren't part of Studio RPC. Since #45 the left (central) half
exposes **two BLE Battery Services** (`0x180F`): its own, and a proxy of the
right half's. `sofleii battery` reads both over the system D-Bus from BlueZ:

1. `GetManagedObjects` on `org.bluez`; the keyboard is the connected
   `Device1` whose `Name` or `Alias` is `Eyelash Sofle` (or the one at
   `--bt-address`). If several match, the first wins and stderr says so.
2. Under it, each `0x180F` service's Battery Level (`0x2A19`). The one with
   **no** `0x2901` user description is the left half; the one described
   `Peripheral 0` is the right half (`Peripheral 1` would be a second
   peripheral). If the descriptions don't settle it, the halves go by service
   handle order (first = left) and `note` says so.
3. `ReadValue({})` on each level, falling back to BlueZ's cached `Value` (with
   a note) if the read fails.

Nothing assumes `hci0` or fixed handles. It only reads: no writes, no
`StartNotify`, no pairing changes.

**BlueZ's `org.bluez.Battery1`, and so upower and most desktop battery
widgets, only show the first service: the left half.** That's why sofleii
reads GATT directly. (The Studio RPC service also shows up twice in BlueZ's
tree; see [Transports](#transports).)

```
$ sofleii battery
left  100%
right 76%
```

```json
{
  "schema": 1,
  "source": "bluez",
  "device": { "name": "Eyelash Sofle", "address": "DD:1F:23:76:6E:71" },
  "left": { "percent": 100 },
  "right": { "percent": 76 },
  "note": null
}
```

- A half is `{"percent": null}` when it's unknown, e.g. only one Battery
  Service is exposed (firmware before #45, or the right half isn't
  forwarded); `note` then says why. `note` is `null` when there's nothing to
  add. Notes and warnings also go to stderr in human mode.
- **Right half off:** the central's proxy keeps reporting `right 0%` for a
  disconnected right half, and GATT can't tell that 0 from a flat battery.
  `battery` stays GATT-only on purpose and shows it as read: knowing the
  split link takes a Studio RPC call, which means opening the exclusive USB
  port (it would fight a running TUI or `status`) or BLE RPC (deaf while USB
  is the selected output), and the waybar battery module must keep working
  without either. `status` and the TUI, which hold an RPC link anyway, show
  that half as `disconnected` when `get_split_info` says the link is down.
- **Exit codes:** 0 when read. **1 only when `--warn-below` is given** and a
  known half is below it (the default threshold of 20 only sets the waybar
  class, it never fails the command; an unknown half never triggers 1).
  **2** when the keyboard isn't found over BLE: the message says whether it's
  not connected to this host, not known to BlueZ, or BlueZ/D-Bus isn't
  available. **3** usage. **4** a D-Bus or GATT error (`kind: "bluez"`), e.g.
  neither level could be read.
- `--timeout` bounds each D-Bus call (default 3000 ms).

### waybar

`sofleii battery --format waybar`:

```json
{"text":"󰌌 L 100 R 76","tooltip":"Eyelash Sofle\nleft: 100%\nright: 76%\nsource: BlueZ GATT DD:1F:23:76:6E:71","class":"ok","alt":"ok","percentage":76}
```

- `text`: `󰌌` (nf-md-keyboard, U+F030C), then `L <left> R <right>`, `-` for an
  unknown half. Not found: `󰌐` (nf-md-keyboard_off, U+F0310).
- `class` / `alt`, by the **lower** known half: `critical` below
  `--critical-below` (default 10), `warning` below `--warn-below` (default
  20), else `ok`; `unknown` if neither half could be read; `disconnected`
  when the keyboard isn't found or the read failed.
- `percentage`: the lower half, for `format-icons`.
- Not found **still prints valid JSON** and exits 2, as `status` does. In
  waybar mode `--warn-below` only moves the `warning` threshold and never
  exits 1, so a low battery doesn't look like a failed script to the bar.

```jsonc
"custom/sofleii-battery": {
  "exec": "sofleii battery --format waybar",
  "return-type": "json",
  "interval": 300,
  "format": "{}",
  "tooltip": true
}
```

```css
#custom-sofleii-battery.warning { color: #ebcb8b; }
#custom-sofleii-battery.critical { color: #bf616a; }
#custom-sofleii-battery.disconnected { opacity: 0.5; }
```

For a check in a script: `sofleii battery --warn-below 20 || notify-send
"Sofle battery low"` (exit 1 low, 2 not connected).

## waybar

`sofleii status --format waybar` prints one line of waybar custom-module
JSON:

```json
{"text":"󰕓 0","tooltip":"Eyelash Sofle\ntransport: usb /dev/ttyACM1\noutput: usb (priority usb)\n…","class":"usb","alt":"usb"}
```

- `text`: an output glyph plus the active profile index (0-based, as in
  `profiles` and `&bt BT_SEL n`). The glyphs are Nerd Font Material Design
  icons: `󰕓` nf-md-usb (U+F0553), `󰂯` nf-md-bluetooth (U+F00AF), `󰂲`
  nf-md-bluetooth_off (U+F00B2). To use other icons, format with `{alt}` and
  `format-icons` instead of `{}`.
- `tooltip`: the `status` summary as `label: value` lines.
- `class` and `alt`: `usb`, `ble` or `disconnected`, from the `output` rule
  above (over BLE, a connected active profile is `ble`).
- **Errors still print valid JSON** with class `disconnected` (and the error
  in the tooltip), and the exit code still says what happened: 2 when the
  keyboard isn't found, 4 on a transport error. The bar keeps rendering.

```jsonc
"custom/sofleii": {
  "exec": "sofleii status --format waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{}",
  // the TUI (a live, read-only dashboard for now)
  "on-click": "foot -e sofleii",
  "tooltip": true
}
```

```css
#custom-sofleii.disconnected { opacity: 0.5; }
#custom-sofleii.ble { color: #5e81ac; }
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Check failed / attention needed: `battery --warn-below` with a half below it; `update check` behind or unknown |
| 2 | Keyboard not found / not connected (a `--device` path that does not exist; `auto` with neither USB nor BLE; over BLE or for `battery`, not connected or BlueZ/D-Bus unavailable) |
| 3 | Usage error (bad flag; `--device`/`--bt-address`/`--transport` contradicting each other; a profile out of range; a rename too long or on an empty slot; `unpair` without a terminal or `--yes`, or not confirmed) |
| 4 | Transport or protocol error: a busy serial port (another sofleii has it), timeout (over BLE, usually USB being the selected output; not after an [endpoint handover](#endpoint-handover)), bad frame, RPC error (including a failing `zmk__device_info` call, and a write's `error` or `success: false` reply), D-Bus/GATT error, GitHub API failure (`update check`). A firmware *without* `zmk__device_info` is not an error for `info` (exit 0, `build.available: false`), but is for `update check` |

## Layout

```
src/
  proto.rs       prost-generated messages (build.rs, from proto/)
  framing.rs     SOF 0xAB / ESC 0xAC / EOF 0xAD, streaming decoder
  transport/     Transport trait (Send) and `auto` selection (mod.rs); serial.rs (USB, discovery,
                 busy-port retry); ble.rs (BlueZ GATT: Studio service choice, AcquireNotify/StartNotify,
                 WriteValue); mock.rs (tests)
  client/        request ids (random first id per process), response matching, notifications set
                 aside, is_dead (link lost vs deaf); rpc.rs = pure request builders
  commands/      status, info, profiles (status reuses profiles::fetch and split::fetch), split
                 (get_split_info; empty reply = unknown), battery (Gatt trait,
                 half identification; tested with a fake BlueZ tree), update (Http trait, version
                 parsing, compare; tested with canned GitHub JSON); the writes profile and output,
                 sharing handover (the expected silence, the read-back over the other transport):
                 plain data, never print or prompt, no UI code
  connect.rs     opening the link without clap types: Config, the `auto` plan with the 1 s USB probe
                 and BLE fallback, Connector (remembers the plan between connects; invalidate),
                 open_other (handover read-back), ble_connected, read_battery
  error.rs       Error, the exit codes, ErrorView (Clone + Serialize: kind, message, exit_code)
  gatt.rs        BlueZ's object tree as plain data, and the keyboard lookup (battery + BLE)
  bluez.rs       system D-Bus (zbus + block_on; no tokio, no libdbus): the Gatt trait, the tree
  github.rs      GitHub REST API over ureq (blocking, rustls): the Http trait
  output.rs      human, JSON and waybar rendering
  logger.rs      the CLI's stderr logger for core `log` records
  session/       the long-running core the TUI (and a later GUI) runs on, no UI code: mod.rs
                 (Request/Event/Risk, SessionHandle); link.rs (the RPC thread: lease, polling,
                 deaf/lost handling, the write actions through commands/); watch.rs (batteries and
                 presence over BlueZ and sysfs); jobs.rs (update check + its cache); state.rs
                 (Snapshot, the pure reducer); fake.rs + tests.rs (a fake keyboard, no hardware)
  tui/           the TUI: the session, input thread and event loop, terminal restore, signals
                 (mod.rs); snapshot.rs (the view derived from session::Snapshot, demo fixture);
                 view.rs (rendering); log_pane.rs (the TUI's logger); tests.rs (TestBackend renders
                 from scripted events, keys, the loop against a fake session). Nothing in
                 transport/, client/, commands/ or session/ depends on it
  main.rs        clap CLI, exit codes, the unpair confirmation; no subcommand on a TTY opens the TUI
proto/           vendored .proto files (see proto/README.md for sources and licenses)
tests/golden.rs  request bytes captured from the v3.0.0 firmware, plus the zmk__device_info call and
                 the cormoran_ble write payloads (worked out from the proto; confirmed by the 2026-10-19 live test)
tests/cli.rs     exit codes and error output with no keyboard (not found, usage; no network)
tests/layering.rs  no eprintln! outside main.rs; transport/, client/, commands/, session/ never name the TUI
tests/session_live.rs  #[ignore]d: a read-only session against the keyboard, printing its events for 10 s
```

License: MIT (see the repository's `LICENSE`). The vendored protos keep
their own MIT licenses, listed in `proto/README.md`.
