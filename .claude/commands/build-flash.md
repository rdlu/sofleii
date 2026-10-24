Build and/or flash the Eyelash Sofle firmware.

Every flash goes through `sofleii flash` (#100): it fetches the firmware, asks
for a double-tap of reset on each half, checks that the chip on the drive is
the half it names (chip serial from `chips.json`, then the firmware's identity
record), copies, and waits for the reboot. Announce each reset double-tap to the
user in chat, and before a live flash make sure they've switched to the spare
keyboard.

## Quickest path: no local build

```bash
mise run flash-release   # newest sofleii-fw release, both halves (no token)
mise run flash-ci        # newest green main CI build, both halves (token from gh)
```

A branch build: dispatch it first (`gh workflow run build.yml --ref <branch>`),
wait for it to succeed, then
`GH_TOKEN=$(gh auth token) sofleii flash both --ci <branch>` (or `--run <id>`).

## ⚠️ ZMK Studio overlay persists across flashes

If a flashed keymap change isn't taking effect, the likely cause is a stored
Studio overlay on the **left** half (re-applied on boot over the compiled
keymap). Fix: settings reset, then the firmware:

```bash
mise run flash-release-reset          # both halves: settings_reset + firmware each (4 double-taps)
GH_TOKEN=$(gh auth token) sofleii flash left --reset --ci   # the left half only, latest main build
```

The BT pairing between halves and with the host has to be redone afterwards.

## Unknown chips

A chip sofleii has never flashed whose firmware can't vouch for it (first
flash, vendor firmware, erased) is refused with a hint. Rerun with
`--adopt left|right` (or a bare `--adopt` to type the half); the serial is then
remembered (`sofleii flash chips`, `sofleii flash forget <serial>`).

## Local build (optional)

Requires `mise run setup` on first use.

```bash
mise run build-flash   # build both halves sequentially, then sofleii flash both
mise run build-left    # left half alone (ZMK Studio enabled)
mise run build-right   # right half alone
mise run build-reset   # settings reset
```

**Do not run `build-left` and `build-right` in parallel**: they share
`build/`. `build-flash` runs them sequentially for exactly this reason.

Output files: `eyelash_sofle_left.uf2`, `eyelash_sofle_right.uf2`,
`settings_reset.uf2`. The names don't matter to `sofleii flash`: it identifies
each file by its content.

```bash
mise run flash-left    # sofleii flash left --file eyelash_sofle_left.uf2
mise run flash-right
mise run flash-reset   # settings_reset only, both halves
```

They wait up to 60 s for the drive, so run them first and then double-tap.

## First-time pairing after both halves are reflashed

1. Power off both halves.
2. Power on the **right** half first (peripheral — starts advertising).
3. Power on the **left** half (central — scans and connects).
4. They auto-pair within a few seconds.
5. Pair with the host via Bluetooth (BT0–BT4 slots on SYS|NUM layer — hold SYS|NUM and tap the matching number).

## ZMK Studio (live keymap editing over USB)

Only the left half has the `studio-rpc-usb-uart` snippet enabled.
Connect left half via USB → open ZMK Studio → select the serial port.

Requirements on Arch Linux: user must be in the `uucp` group.
```bash
sudo usermod -aG uucp $USER  # then log out and back in
```

## Toolchain notes

- Zephyr SDK **0.17.0** at `tools/zephyr-sdk-0.17.0` — do NOT upgrade without checking ZMK compatibility
- ZMK is cormoran's fork on the ZMK-main line (`main+dya`, Zephyr 4.1), SHA-pinned in `config/west.yml`
- AUR `zephyr-sdk` package requires Zephyr 4.2+ and is incompatible with this ZMK version
