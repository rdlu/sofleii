Build and/or flash the Eyelash Sofle firmware.

## Quickest path — flash from GH Actions (no local build)

One command downloads the latest successful main build and walks through flashing both halves:

```bash
mise run flash-release
```

This is the preferred path — no local toolchain, no drag-and-drop, just follow the on-screen prompts to double-tap reset on each half. Requires `gh` authenticated.

To build on demand (e.g. from a feature branch) before downloading:
```bash
gh workflow run "Build ZMK firmware" --ref <branch-name>
```
Wait for the run to succeed, then `mise run flash-release` picks up the latest main artifacts. To flash from a specific non-main run, download manually with `gh run download <run-id>`.

## ⚠️ ZMK Studio overlay persists across flashes

If the user reports that a flashed keymap change isn't taking effect, the most likely cause is a stored ZMK Studio overlay on the **left** half. Studio edits are persisted to settings flash and re-applied on boot, overriding the compiled firmware's keymap.

**Fix** — always use this two-step on the left half:
```bash
mise run flash-release-reset   # double-tap LEFT — flashes settings_reset (clears overlay + BT bonds)
mise run flash-release          # double-tap LEFT, then RIGHT — flashes normal firmware on top
```

After this, the BT pairing between halves and with the host will need to be redone.

## Local build (optional)

Only needed if you want to iterate without pushing to GH Actions. Requires `mise run setup` on first use.

```bash
mise run build-flash   # build both halves sequentially, then interactive flash
mise run build-left    # left half alone (ZMK Studio enabled)
mise run build-right   # right half alone
mise run build-reset   # settings reset
```

`build-flash` is the fastest local iteration path: one command, builds both halves in sequence, aborts if either build fails, then walks you through double-tap-reset on each half.

**Do not run `build-left` and `build-right` in parallel** — they share `build/` and step on each other. `build-flash` runs them sequentially for exactly this reason.

Output files: `eyelash_sofle_left.uf2`, `eyelash_sofle_right.uf2`, `settings_reset.uf2` (same names the `flash-release` flow uses after renaming the upstream-workflow artifacts).

Flash the locally-built artifacts:
```bash
mise run flash-left    # prompts, waits for NICENANO mount, copies, syncs
mise run flash-right
mise run flash-reset
```

These tasks wait up to 60s for the `NICENANO` mount to appear, so you can run them first and then double-tap reset — no race condition.

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
