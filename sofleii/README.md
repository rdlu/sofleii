# sofleii

**Sofle Interactive Interface**: a command-line tool and a ratatui TUI that
talk to the Eyelash Sofle over the **ZMK Studio RPC** protocol, the same
protocol the Studio web app uses, without a browser. Design and roadmap:
[#44](https://github.com/rdlu/sofleii/issues/44).

**User docs live on the site: [sofleii.dlu.one/docs/sofleii](https://sofleii.dlu.one/docs/sofleii/)**
(source: `site/content/docs/sofleii.md`): installing and running, the TUI
and its keys, every command with an example, the `--json` contract, the
exit codes, the waybar modules, the transport gotchas and the write safety
rules. This README is for working on the crate: the layout, how each piece
works, the full JSON field reference, testing and the vendored protos.

Status: **pre-release** (`0.1.0`, not tagged yet; release work in
[#83](https://github.com/rdlu/sofleii/issues/83)). Studio RPC over USB
serial (left half) or BLE GATT through BlueZ; batteries over BLE GATT;
update checks over the GitHub REST API; the reads and the profile and
output-priority writes (confirmed against the keyboard on 2026-10-19); a
live TUI dashboard that also switches profile and output (with a
confirmation when the link would drop); and the live keymap, read-only, with
its drift from `config/eyelash_sofle.keymap` (`keymap show|diff`, #76).

## Build, run and test

Needs Rust (edition 2024, 1.99+) and `protoc`, both from pacman
(`rust`, `protobuf`). Dependencies follow their latest stable releases and
`rust-version` follows the current stable rustc: no MSRV holdbacks. From
the repo root:

```bash
mise run sofleii                    # build (release) and open the TUI
mise run sofleii -- status          # any subcommand after --
mise run sofleii -- profiles --json
mise run sofleii-test               # cargo test + clippy -D warnings + fmt --check
mise run tui-screenshots            # regenerate the docs' TUI screenshots
```

Or directly: `cd sofleii && cargo run -- info`.

- **No hardware needed for the tests.** The transports, BlueZ and GitHub
  sit behind traits with fakes (`transport/mock.rs`, a fake BlueZ tree,
  canned GitHub JSON, `session/fake.rs`); the TUI renders to ratatui's
  `TestBackend`. `tests/session_live.rs` is `#[ignore]`d and runs a
  read-only session against the real keyboard
  (`cargo test --test session_live -- --ignored --nocapture`). Only send
  read requests to the owner's keyboard unless asked.
- **Screenshots** for the site and the root README come from the demo
  fixture, never the live keyboard: `SOFLEII_TUI_DEMO=1 sofleii` shows
  `tui::snapshot::demo` (fake addresses, no session).
  `scripts/tui-screenshot.py` (stdlib Python + tmux) runs it at 80x24,
  captures the pane with its ANSI colours, opens the help, and writes
  `site/static/img/sofleii/tui-{dashboard,help}.svg`. Rerun
  `mise run tui-screenshots` after any visible TUI change.
- `battery` and the BLE transport talk to BlueZ through `zbus` 6 (pure
  Rust, each call run with `zbus::block_on` on zbus's own runtime): no
  libdbus, no tokio, nothing extra to install beyond a running
  `bluetooth.service`.
- Over USB the keyboard is found by its USB product string `Eyelash Sofle`
  (the VID/PID is shared by every ZMK board, and the `/dev/ttyACM*` number
  is not stable).

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
                 parsing, compare; tested with canned GitHub JSON), keymap (the keymap reads,
                 the per-run behaviour cache, finding the .keymap); the writes profile and output,
                 sharing handover (the expected silence, the read-back over the other transport):
                 plain data, never print or prompt, no UI code
  connect.rs     opening the link without clap types: Config, the `auto` plan with the 1 s USB probe
                 and BLE fallback, Connector (remembers the plan between connects; invalidate),
                 open_other (handover read-back), ble_connected, read_battery
  error.rs       Error, the exit codes, ErrorView (Clone + Serialize: kind, message, exit_code)
  gatt.rs        BlueZ's object tree as plain data, and the keyboard lookup (battery + BLE)
  bluez.rs       system D-Bus (zbus + block_on; no tokio, no libdbus): the Gatt trait, the tree
  github.rs      GitHub REST API over ureq (blocking, rustls): the Http trait
  keymap/        the keymap model, pure (see "Keymap" below): mod.rs (Behavior, Labels, Layer,
                 Binding, Context); render.rs (ZMK form + legends); keys.rs + keys_table.rs (HID usage
                 <-> ZMK names, generated by scripts/zmk_keys_table.py); dts.rs (.keymap parser);
                 expr.rs (cell expressions); diff.rs (live vs file)
  output.rs      human, JSON and waybar rendering; the keymap grids
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
                 the cormoran_ble write payloads (worked out from the proto; confirmed by the 2026-10-19 live test),
                 and the keymap reads: requests and replies from tests/fixtures/keymap_live.hex
tests/keymap_capture.rs  #[ignore]d: re-captures that fixture from the keyboard over USB (read-only),
                 and freezes the matching .keymap beside it as tests/fixtures/keymap_live.keymap
tests/cli.rs     exit codes and error output with no keyboard (not found, usage; no network)
tests/layering.rs  no eprintln! outside main.rs; transport/, client/, commands/, session/ never name the TUI
tests/session_live.rs  #[ignore]d: a read-only session against the keyboard, printing its events for 10 s
```

## `status` details

How the human `status` rows (and the TUI summary) are derived:

`split link` comes from `cormoran_ble`'s `get_split_info`: `connected`,
`disconnected (right half off or out of range)`, or `unknown (firmware
without the split-info fix)`. The last is cormoran's module before our fork's
fix (`rdlu/zmk-module-ble-management` `5ea5b4f`), which answers with no
`info` at all; sofleii never reads that as disconnected. With the right half
off, the central's battery proxy keeps reporting 0% for it, so when the link
is known to be down the battery row reads `right disconnected` instead of
`right 0%` (an unknown link keeps the level as read). With the link up, a
right half at exactly 0% keeps its level but gets a `battery.note`: `right
0% right after a reconnect may be stale; re-run in a few seconds` (a half
that's really flat isn't on the link). The waybar tooltip has the same
`split link:` and `battery:` lines.

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

## TUI internals

What the user sees (the screenshot, the keys, the link states) is on the
[site](https://sofleii.dlu.one/docs/sofleii/#the-tui). How it works:

- **Live.** The TUI runs the core's session (`src/session/`)
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
- **Keys** are listed on the site and in `?`. Quitting is instant: no
  thread is joined.
- **Writes** (`tui/actions.rs` words them, `App` runs the flow): `⏎`/`s`
  sends `SwitchProfile(selected)` (the active slot only logs "already
  active"), `o` sends `SetOutput(other)`. When `Request::risk` says
  `DropsCurrentLink` or `Destructive` (output away from the transport in
  use, a switch over BLE, with priority ble or to an empty slot; unpair),
  a confirm modal opens first: **only `y` sends**; any other key, `⏎`
  included, cancels, so a double Enter never confirms (Ctrl-C still
  quits). The core never prompts. One write in flight (`App::action`):
  the bottom edge shows a spinner and action keys are ignored until its
  `Done`, then the result (5 s; an error until the next key) and log lines.
  After a handover the session follows Studio (`◌ ble connecting`, then
  `● ble …`).
- **Output BLE and the active profile:** the firmware selects BLE only
  while the *active* profile is connected and otherwise falls back to USB
  (`get_selected_transport`, `zmk/app/src/endpoints.c`). Studio then
  answers on that profile's host. `Snapshot::active_ble()` says which case
  applies: it compares the active slot's peer address with this host's
  adapter addresses (`Event::Adapters`, from BlueZ `Adapter1.Address` on
  the watch thread). Without them it falls back to `connected` plus
  presence.
  - **Not connected:** `DropsCurrentLink`, with a "stays on USB" modal. The
    reply comes back over USB, so there's no handover and no replan.
  - **Another host's:** `Destructive`. This host loses Studio; the session
    goes `Deaf` over BLE.
  - **This host's or unknown:** as before.

  The fake keyboard (`session/fake.rs`) models this with `host_profile`
  and per-slot `connected`.
- **Split** is polled with the profiles: `connected`, `disconnected (right
  half off or out of range)` or `unknown (firmware without the split-info
  fix)` (shorter below 70 columns). While it's disconnected the battery row
  shows `R disconnected` instead of the proxy's stale 0%. A change of the
  link (either way) re-reads the batteries at once and again 5 s later: a
  rejoined right half's level takes the central a moment to fetch, and until
  then the proxy says 0%. For 10 s after the right half rejoins, a right 0%
  shows as `R reading…`; a non-zero read ends that early, and a 0% that
  lasts past it is shown as 0% (a flat battery is real).
- Square panes as on the docs site; below 70 columns the summary is one
  column. Battery bars use `battery`'s defaults (warning below 20%,
  critical below 10%). `NO_COLOR` turns colour off (bold and dim remain).
- The terminal is restored on quit, on SIGTERM/SIGHUP/SIGINT, and on a
  panic (ratatui's panic hook runs before the release profile's
  `panic = "abort"`).

## Keymap

`keymap show` and `keymap diff` (#76) read `get_keymap`,
`get_physical_layouts`, `check_unsaved_changes`, `list_all_behaviors`, then
`get_behavior_details` for each behaviour id the bindings use (once per run:
`commands::keymap::BehaviorCache`). Nothing writes. The model in `keymap/`
is pure, so the TUI view, editing (#77) and snapshots (#78) reuse it.

- **Labels.** Bindings carry a firmware-local behaviour id, and the details
  only a display name: ZMK's `display-name` ("Key Press"), else the node's
  `label` property (macros: "PIPE_OP" for `M_PIPE`), else the node name
  ("home_row_mod_left" for `hml`, "z_so_off" for `soft_off`). `Labels`
  maps them back to the devicetree label: a built-in table for ZMK's own,
  then every labelled behaviour node in the `.keymap` (node name, `label`,
  `display-name`), then an identifier-like display name as is (`rsr_vol`).
  `Behavior.label` is `null` only when nothing fits; the ZMK form then uses a
  sanitised placeholder. Snapshots (#78) store this label, never the id.
- **Parameters** are typed from the behaviour's metadata: the
  `BehaviorBindingParametersSet` whose `param1` accepts the value decides
  (`&bt BT_SEL 0` takes a profile, `&bt BT_CLR` nothing). `HidUsage` renders
  through `keys.rs` (ZMK's encoding: `page << 16 | id`, implicit modifiers
  in the high byte; a full-value lookup first keeps `LBRC`, then
  `LG(LS(H))`), `LayerId` as the layer's current index (on the keyboard the
  parameter is the layer **id**, which survives Studio moving layers),
  everything else as a number. `render.rs` has tables for the commands the
  metadata doesn't name (`bt`, `rgb_ug` incl. `RGB_COLOR_HSB(h,s,v)`,
  `out`, `bl`, `ext_power`, `mkp`, `mmv`/`msc` with the file's
  `ZMK_POINTING_DEFAULT_*_VAL`). Unknown values stay numeric; nothing
  panics. `keys_table.rs` is generated from ZMK's `keys.h` with `cpp` by
  `scripts/zmk_keys_table.py` (rerun after a `zmk` bump); the rendered name
  is the shortest non-deprecated alias plus a few overrides (`GRAVE`,
  `SPACE`, `LGUI`).
- **Legends** (`Legend {tap, hold}`) are for grids: at most 6 columns, the
  tap on top, the hold side of a hold-tap or the behaviour's label below.
  `output::keymap_grid` places cells by the physical layout (7 columns per
  key unit, rows by rounded `y`, overlapping keys pushed right), so the
  Sofle fits in 112 columns.
- **The parser** (`dts.rs`) is a tolerant devicetree subset: comments,
  `\` continuations, object-like `#define`s (the file's override
  `expr::BUILTIN_DEFINES` from ZMK's dt-bindings headers; a define can
  supply several cells or a whole `&kp A`), nodes, labels, `&label { }`
  overrides, string/cell/byte properties. `#include`s are skipped and
  `#if`s not evaluated (a note says so). Layers are the children of the
  `compatible = "zmk,keymap"` node in order, named by `display-name` (else
  the node name), any number of them. Each cell expression is evaluated
  like `cpp` would (`expr.rs`: C operators, key names, `LS()`…`RG()`,
  `MOVE_X/Y()`, `BIT()`, `ZMK_HID_USAGE()`, `RGB_COLOR_HSB_VAL()`). What
  doesn't evaluate (an unknown name, a function-like macro) becomes
  `FileParam::Unresolved` plus a note; only a file without a keymap node is
  an error (exit 3).
- **The diff** (`diff.rs`) normalises both sides to `{label, [value;2]}`:
  aliases are the same value (`LSHIFT` = `LSHFT`, `LBRC` = `LS(LBKT)`), and
  a layer parameter becomes the layer **pair** it points at, so a moved or
  renamed layer keeps its `&mo`s equal. Layers pair by name, then the
  leftovers by index (`renamed`). `reordered` lists pairs whose relative
  order changed (a shift from an added or removed layer isn't one).
  `added` = only on the keyboard, `removed` = only in the file. `drift` is
  any of those or a differing key; the command exits 1 on drift.

JSON (`keymap show --json`), besides `schema` and `transport`:

- `keymap_file`: the `.keymap` used for labels, or `null`.
- `layers[]`: `index`, `id`, `name`, `bindings[]`. Each binding:
  `position`; `behavior {id, name, display_name}` (`name` = the label);
  the raw `param1`/`param2`; `params[]` (as many as the behaviour takes for
  that `param1`): `{value, kind: "hid_usage"|"layer"|"number", zmk, name}`
  (`name`: the layer name, or the metadata's name for a constant such as
  `"Select Profile"`, else `null`); `zmk` (`"&hml LALT S"`); `legend {tap,
  hold}`.
- `behaviors[]`: the behaviours in use, `{id, display_name, label,
  metadata[]}`; `metadata[]` are the parameter sets, `{param1[], param2[]}`
  of `{type: nil|constant|range|hid_usage|layer_id, name, …}`.
- `physical_layout`: `{index, name, keys[]}`, each key `{position, x, y,
  width, height, r, rx, ry}` in ZMK's units (100 = 1u, `r` in 1/100 °).
- `available_layers`, `max_layer_name_length`, `unsaved_changes` (`null`
  if the firmware didn't say), `notes[]`.

`keymap diff --json`: `keymap_file`, `unsaved_changes`, `drift`,
`layers[]` (`{name, file_index, live_index, live_id, differences}`),
`differences[]` (`{layer, live_index, position, file, live}`, each side the
ZMK form or `null` where that side has no key), `added[]`/`removed[]`
(`{index, name}`), `reordered[]` (`{name, file_index, live_index}`),
`renamed[]` (`{file_name, live_name, file_index, live_index}`) and the
parser's `notes[]`.

The decode tests run on a real capture: `tests/fixtures/keymap_live.hex`
holds every request sent and reply received (unframed, request ids from 1),
recorded read-only by `tests/keymap_capture.rs`
(`SOFLEII_CAPTURE=1 cargo test --test keymap_capture -- --ignored`). It
writes `tests/fixtures/keymap_live.keymap` in the same run, a copy of
`config/eyelash_sofle.keymap`, and the capture tests (`commands::keymap`,
`tests/golden.rs`) diff the capture against **that copy**, never the live
file, so a keymap edit never breaks them. Capture only from a keyboard
flashed with the current file and without Studio edits (`sofleii keymap
diff` says no drift), so the pair agrees. The live file is still covered:
`keymap::dts`'s `the_repo_keymap_parses_cleanly` parses it with no notes,
and `config/eyelash_sofle.keymap` is in `sofleii.yml`'s `paths:` so keymap
PRs run the crate CI. Behaviour details are captured in ascending id
order, as `keymap::read` asks for them: the firmware's list isn't sorted (a
MYGAME build lists id 62 first), and a capture in list order made the
replayed ids line up with the wrong behaviours. Re-capture after a firmware
change the tests should follow.

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
`{"kind": "ble", "address": "00:11:22:33:44:99"}` (the BLE shape is new,
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
      "address": "00:11:22:33:44:55", "name": null },
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
  "device": { "name": "Eyelash Sofle", "serial_number": "0011223344556677" },
  "studio": { "locked": false },
  "output_priority": "usb",
  "max_profiles": 5,
  "bonded_profiles": 1,
  "active_profile": 0,
  "profile": { "index": 0, "active": true, "connected": true, "bonded": true,
               "address": "00:11:22:33:44:55", "name": null },
  "split": { "known": true, "connected": true, "is_split": true,
             "is_central": true },
  "split_note": null,
  "battery": { "left": { "percent": 100, "state": null },
               "right": { "percent": 71, "state": null }, "note": null }
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
  "state": "disconnected"}` rather than the central proxy's stale 0%. With
  the link up or unknown the levels are as read and `state` is `null` (it is
  always present, never omitted; see [Battery](#battery)).
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
serial number   0011223344556677
studio          unlocked
firmware        v3.0.0-65-g1e468b7
zmk             v0.3-198-ge5c9b691
zephyr          4.1.0
board           nice_nano
built           2026-10-17T20:50:06Z
build hash      1e37f75107aeb7993596e9d27b668701ed790839
device id       0011223344556677
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
  "device": { "name": "Eyelash Sofle", "serial_number": "0011223344556677" },
  "build": {
    "available": true, "note": null,
    "zmk_version": "v0.3-198-ge5c9b691", "zmk_dirty": false,
    "zmk_config_version": "v3.0.0-65-g1e468b7", "zmk_config_dirty": false,
    "module_version": "8cedd2d", "module_dirty": false,
    "zephyr_version": "4.1.0", "build_timestamp": "2026-10-17T20:50:06Z",
    "board": "nice_nano",
    "build_hash": "1e37f75107aeb7993596e9d27b668701ed790839"
  },
  "hardware": { "device_id": "0011223344556677", "reset_cause": 1,
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
serial number   0011223344556677
studio          unlocked
build           unavailable: this firmware has no zmk__device_info subsystem (built before #46); flash a newer build
subsystems
   0  cormoran_ble
   1  cormoran_rsr
```

### `update check`

Two parts (#83): the **firmware** (`--firmware` alone), and **sofleii
itself** (`--cli` alone; no keyboard needed). With neither flag, both. The
firmware part reads `zmk_config_version` from the keyboard (as `info`
does) first, so "not found" exits 2 without a network call. Then the
shared cache (below), else the GitHub REST API, without `gh`:

1. `GET /repos/{repo}/releases?per_page=50`, **once, for both parts**
   (never `releases/latest`: sofleii's own releases are published with
   `--latest=false`, but a manual release could still steal "latest").
   Drafts and prereleases are dropped, then the tags split into two
   families, each sorted newest first by semver: **firmware** `vX.Y.Z`
   and **sofleii** `sofleii-vX.Y.Z`. Anything else (`canary-zephyr-4.1`,
   a non-semver tag) is ignored. The newest firmware release is
   `latest`; `GET /repos/{repo}/commits/{tag}` gives its commit.
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
4. **Firmware inputs decide, not commits** (v0.1.1). If none of the
   changed files is a firmware input, the status is `up_to_date` whatever
   the counts say: a PR-branch build of a merged PR (live case: `0858966`
   vs v3.2.0's merge commit `96f3a2d`, 8 commits and 35 files, all docs
   and crate), or docs-only commits on `main`. The inputs are
   `update::FIRMWARE_INPUTS`, exactly the `paths:` that trigger
   `.github/workflows/build.yml` (`config/**`, `build.yaml`,
   `zmk-rgb-layer/**`, and the workflow itself, which pins the build
   container); a test reads the workflow and fails if the two drift. A
   rename counts when either side is an input. GitHub's compare file list
   runs **from the merge base to the head**, so which list covers what
   depends on the status:
   - `ahead` (keyboard behind): the merge base is the build, so the one
     list is the whole diff. No extra request.
   - `behind` (keyboard ahead): the list is empty by construction (the
     merge base is the release), so the build's side comes from the
     reverse compare `{latest}...{running}`. One extra request.
   - `diverged`: both lists, the forward one and the reverse; changed on
     either side counts. One extra request.

   `files_changed` / `firmware_files_changed` carry the counts, and the
   `note` says `built from 0858966; differs from v3.2.0 only outside the
   firmware inputs (35 files)` (or `same source as v3.2.0: the commits in
   between change no files`). **Can't prove it:** GitHub lists at most 300
   files per compare, so a list of 300 (or one with no `files` key, or a
   failed reverse compare, rate limiting included) leaves both counts
   `null`, goes by the commits as before, and says so in the `note`
   (`can't tell whether a firmware input changed: …; going by the
   commits`). Before v0.1.1 the rule asked for no changed files at all,
   so the live case above read `v3.2.0 available` and exited 1; and since
   an `ahead` keyboard's list is always empty, every build ahead of the
   release read as "same source".
5. Then it names the **release the build is, or follows**
   (`running.release`). **The firmware can't report its own tag:** CI
   builds the merge commit before the release is tagged, from a shallow
   checkout with no tags, so every CI build reports a bare SHA. sofleii
   maps it back, cheapest way first (anonymous GitHub allows 60 requests an
   hour):
   - `up_to_date` at the release's own commit: the latest release, exact.
     By the firmware-inputs rule: exact with `equivalent: true`, shown as
     `v3.2.0 (built from 0858966)`. No extra request.
   - `ahead`: the latest release, `commits_after` = `commits_ahead`. No
     extra request.
   - A local `git describe` string already names the tag and the count
     (`v3.0.0-68-gf9a665f` = `v3.0.0+68`; `v3.1.0` = exact). No extra
     request; this also covers `unknown`.
   - `behind` with a bare SHA: the firmware releases from step 1's list
     (no extra request; the latest one skipped, since it was already
     compared), newest first, `compare/{tag}...{running}` until one fits.
     `identical` gives exact; the build being `ahead` of the tag gives
     `equivalent` when no firmware input changed (the list is the whole
     diff here), else `commits_after` = `ahead_by`. `behind` and
     `diverged` never fit (their lists say nothing about the build's
     side; before v0.1.1 an empty `behind` list read as "same tree"). At
     most 10 releases. If none fits, or
     that lookup fails (e.g. rate limiting), `release` is `null` (with a
     `note` for a failure) and the verdict stands.
6. The **sofleii part** compares `CARGO_PKG_VERSION` with the newest
   `sofleii-v*` release by semver (a pre-release such as `0.2.0-dev` sorts
   before `0.2.0`): `up_to_date`, `behind`, `ahead` (a newer dev build), or
   `unknown` with a `note` when there is no sofleii release yet. Unknown
   **never** makes the exit code 1.

**Cache.** `update check` and the TUI share `session::jobs::Checker` and
its file, `$XDG_CACHE_HOME/sofleii/update.json` (else
`~/.cache/sofleii/`), honoured for 6 h. Schema 3 (v0.1.1: the firmware-inputs
rule, so a 0.1.0 "behind" isn't served again; schema 2, #83, split the
parts) has a firmware part
(keyed by the running `zmk_config_version` + dirty flag) and a sofleii part
(the newest `sofleii-v*` release as listed; the verdict is worked out
against the running binary when read, so two installed versions can share
it), each with its own timestamp. A check is a cache hit only when every
part asked for is cached and fresh; otherwise one list request renews the
sofleii part too. `--refresh` skips the cache (and still stores the
answer). A failed request never replaces a good entry; a file of another
schema is ignored and rewritten. The text output then ends with `checked 2h ago,
cached (--refresh asks GitHub again)`; the JSON has `from_cache` and
`checked_at_ms` (for a cached answer, the older part's time).

The text leads with versions; the commit counts are in the JSON. A
`-dirty` build is compared by its commit, with `running.dirty: true` and a
note in the text output. Real output (2026-10-21; the left half runs PR
#90's branch build `0858966`, and v3.2.0 is that PR's merge commit, which
adds 35 files of docs and crate changes on top):

```
$ sofleii update check --refresh
up to date: v3.2.0 (built from 0858966)
running         v3.2.0 (built from 0858966)
latest          v3.2.0 96f3a2d, published 2026-10-21T21:01:59Z
note            built from 0858966; differs from v3.2.0 only outside the firmware inputs (35 files)
sofleii         up to date: v0.1.0
```

sofleii 0.1.0 said `v3.2.0 available, running v3.1.0+36 (0858966)` there
and exited 1.

The `sofleii` row reads `up to date: v0.1.0`, `v0.2.0 available, running
v0.1.0` or `ahead of v0.1.0: running v0.2.0` otherwise. `--cli` prints that
row alone (plus the cache line).

Other verdicts: `v3.1.0 available, running v3.0.0+68 (f9a665f)`, `ahead of
v3.1.0: running v3.1.0+3 (abc1234)`, `can't compare with v3.1.0: running f9a665f (no
release)` (with the reason as a `note`). `(no release)` replaces the
version when no release precedes the build.

```json
{
  "schema": 1,
  "transport": { "kind": "usb", "device": "/dev/ttyACM0" },
  "running": { "version": "0858966", "commit": "0858966", "dirty": false,
               "release": { "tag": "v3.2.0", "exact": true, "commits_after": 0,
                            "equivalent": true } },
  "latest": { "tag": "v3.2.0",
              "commit": "96f3a2d54a4d912033ce296188b102771717632e",
              "published_at": "2026-10-21T21:01:59Z" },
  "status": "up_to_date",
  "commits_behind": 8,
  "commits_ahead": 0,
  "files_changed": 35,
  "firmware_files_changed": 0,
  "note": "built from 0858966; differs from v3.2.0 only outside the firmware inputs (35 files)",
  "cli": { "version": "0.1.0", "latest": "0.1.0", "tag": "sofleii-v0.1.0",
           "published_at": "2026-10-21T21:05:43Z", "status": "up_to_date",
           "note": null },
  "checked_at_ms": 1792617374295,
  "from_cache": false
}
```

- `cli` (additive, schema stays 1): `version` is this binary's,
  `latest`/`tag`/`published_at` the newest `sofleii-v*` release's (`null`
  without one), `status` as below, `note` set for `unknown`. It is `null`
  under `--firmware`. Under `--cli`, `transport` and every firmware field
  (`running` … `note`) are `null`.

- `status`: `up_to_date`, `behind`, `ahead` or `unknown` (#47's schema).
  `commits_ahead` > 0 on `behind` means the build also has unreleased
  commits.
  `commits_behind` / `commits_ahead` are `null` only when `unknown`.
- `running.version` is the raw string, `-dirty` included. `running.commit`
  is the abbreviated SHA from it, or the full SHA an exact tag resolved to.
  `latest.commit` is always the full SHA.
- `running.release`: `{"tag", "exact", "commits_after", "equivalent"}`
  (`commits_after` is 0 when `exact`; `equivalent`, added in v0.1.1, is
  true when it's exact by the firmware inputs only, not the same commit),
  or `null` when no release precedes the build.
- `files_changed` / `firmware_files_changed` (additive, v0.1.1): files
  that differ between the build and the release (for `diverged`, on either
  side since the merge base) and how many of those are firmware inputs; 0
  and 0 for the release's own commit; `null` when `unknown`, or when the
  file list couldn't be trusted (see step 4).
- `note` explains an `unknown` status, an `up_to_date` from another
  commit, a file list that couldn't decide, or a failed release lookup;
  otherwise `null`.
- **Exit codes:** 0 `up_to_date` or `ahead`; **1** if the firmware is
  `behind` or `unknown`, or sofleii is `behind` (the worse checked part
  wins; sofleii `unknown` counts as 0; usable as `sofleii update check ||
  notify-send "Sofle update"`); 2 keyboard not found (checked before any
  network call); 3 usage (e.g. a malformed `--repo`); **4** transport or
  protocol error, firmware **without** `zmk__device_info` (`kind:
  "unsupported"`; unlike `info`, which exits 0 there), or a GitHub failure
  (`kind: "github"`: network, a 404 release for `--repo`, a rejected token,
  rate limiting).
- **Token:** none needed for public releases. `GH_TOKEN`, else
  `GITHUB_TOKEN`, is sent as a bearer token if set, which lifts the
  anonymous limit of 60 requests per hour (a check usually makes 2 to 4,
  `--cli` alone 1, a cache hit 0; the 4th is the reverse compare of a build
  ahead of or diverged from the release; a bare SHA that's behind adds up to 10
  compares for the release lookup). A rate
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
            "address": "00:11:22:33:44:66", "name": null },
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
  "device": { "name": "Eyelash Sofle", "address": "00:11:22:33:44:99" },
  "left": { "percent": 100, "state": null },
  "right": { "percent": 76, "state": null },
  "note": null
}
```

- `state` is always present and always `null` here (`battery` reads GATT
  only and never knows the split link; added in schema 1 alongside the
  split link, see the next bullets). `status` sets it to `"disconnected"`.
- A half is `{"percent": null, "state": null}` when it's unknown, e.g. only one Battery
  Service is exposed (firmware before #45, or the right half isn't
  forwarded); `note` then says why. `note` is `null` when there's nothing to
  add. Notes and warnings also go to stderr in human mode.
- **Right half off:** the central's proxy keeps reporting `right 0%` for a
  disconnected right half, and GATT can't tell that 0 from a flat battery.
  `battery` stays GATT-only on purpose and shows it as read (`state`
  stays `null`): knowing the
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


## waybar

The module config and the classes are on the
[site](https://sofleii.dlu.one/docs/sofleii/#waybar). The output, for tests
and parsers:

```json
{"text":"󰕓 0","tooltip":"Eyelash Sofle\ntransport: usb /dev/ttyACM1\noutput: usb (priority usb)\n…","class":"usb","alt":"usb"}
{"text":"󰌌 L 100 R 76","tooltip":"Eyelash Sofle\nleft: 100%\nright: 76%\nsource: BlueZ GATT 00:11:22:33:44:99","class":"ok","alt":"ok","percentage":76}
```

- `status`: an output glyph plus the active profile index. Nerd Font
  Material Design icons: `󰕓` nf-md-usb (U+F0553), `󰂯` nf-md-bluetooth
  (U+F00AF), `󰂲` nf-md-bluetooth_off (U+F00B2). `class`/`alt` follow the
  `output` rule in [`status` details](#status-details).
- `battery`: `󰌌` nf-md-keyboard (U+F030C), `-` for an unknown half; not
  found is `󰌐` nf-md-keyboard_off (U+F0310). `class` by the **lower** known
  half: `critical` below `--critical-below` (10), `warning` below
  `--warn-below` (20), else `ok`; `unknown` if neither half was read;
  `disconnected` when not found or the read failed. `percentage` is the
  lower half.
- Errors still print valid JSON (class `disconnected`, the error in the
  tooltip) with the real exit code; in waybar mode `--warn-below` never
  exits 1.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Check failed / attention needed: `battery --warn-below` with a half below it; `update check` behind or unknown; `keymap diff` with drift |
| 2 | Keyboard not found / not connected (a `--device` path that does not exist; `auto` with neither USB nor BLE; over BLE or for `battery`, not connected or BlueZ/D-Bus unavailable) |
| 3 | Usage error (bad flag; `--device`/`--bt-address`/`--transport` contradicting each other; a profile out of range; a rename too long or on an empty slot; `unpair` without a terminal or `--yes`, or not confirmed; `keymap show --layer` naming no layer; `keymap diff` with no `.keymap` found, or an explicit `--keymap` that can't be read or has no keymap node) |
| 4 | Transport or protocol error: a busy serial port (another sofleii has it), timeout (over BLE, usually USB being the selected output; not after an [endpoint handover](#endpoint-handover)), bad frame, RPC error (including a failing `zmk__device_info` call, and a write's `error` or `success: false` reply), D-Bus/GATT error, GitHub API failure (`update check`). A firmware *without* `zmk__device_info` is not an error for `info` (exit 0, `build.available: false`), but is for `update check` |

## Protos

The `.proto` files in `proto/` are vendored and must match the firmware's
pins: bump them when `config/west.yml` moves `zmk` (its
`zmk-studio-messages` import), `zmk-module-ble-management` or
`zmk-feature-device-info`. `proto/README.md` lists the source commits and
licenses. (The directory isn't `proto/zmk/` like upstream because the
repo's unanchored `zmk/` ignore rule would swallow it.) `build.rs` reads
the Commit column of that table into `sofleii --version` (the "firmware
line" it prints), and fails the build if a row is missing or malformed, so
keep the table's shape when bumping.

## CI and releases

`.github/workflows/sofleii.yml` runs on PRs and pushes to main touching
`sofleii/**`: fmt, clippy `-D warnings`, the tests (latest stable Rust,
distro `protoc`), and a release build that must run `--version` and answer
`battery` with "not found" on a runner with no system bus.

`.github/workflows/sofleii-release.yml` runs on a `sofleii-vX.Y.Z` tag: it
checks the tag against `Cargo.toml`'s `version` and that the commit is on
main, builds `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`
(native arm64 runner), packages
`sofleii-<version>-<target>.tar.gz` (+ `.sha256`) with
`.github/scripts/package-sofleii.sh`, attests them, and creates the
release with `--latest=false` (the firmware stays "latest"). The release
notes are that version's section of [`CHANGELOG.md`](CHANGELOG.md), which
git-cliff generates from the conventional commits that touch `sofleii/`
(`cliff.toml` + `scripts/changelog.sh` at the repo root; `mise run
changelog` regenerates it, and the version bump's PR must carry the
regenerated file, which `.github/workflows/changelog.yml` checks). Its
`workflow_dispatch` is a dry run: it prints the notes, then builds and
packages only, as workflow artifacts. The procedure is in the root
`CLAUDE.md`.

License: MIT (see the repository's `LICENSE`). The vendored protos keep
their own MIT licenses, listed in `proto/README.md`.
