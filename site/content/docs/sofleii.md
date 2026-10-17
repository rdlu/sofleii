---
title: The sofleii tool (planned)
linkTitle: Overview (planned)
description: sofleii, a planned Rust/ratatui CLI and TUI for the Eyelash Sofle - split and connection status, BLE profiles, battery, output priority, firmware update checks and the flash and build routines - over the Studio RPC protocol.
keywords: [sofleii, CLI, TUI, ratatui, Rust, Studio RPC, BLE profiles, battery, roadmap]
weight: 17
---

> [!NOTE]
> **Planned, not released.** Nothing on this page exists yet. It describes
> what the tool is meant to do, so the details will change as it gets built.
> See the [roadmap](#roadmap).

**sofleii** will be a command-line tool and a terminal UI (Rust,
[ratatui](https://ratatui.rs/)) for managing the keyboard from a terminal.
It will live in this repo, next to the firmware it talks to.

## How it talks to the keyboard

Through the **Studio RPC protocol**, the same one the Studio web app uses,
to the **left half**, over USB serial or Bluetooth. The firmware already
carries what it needs: v3.0.0 kept cormoran's custom-RPC plumbing, with the
`ble-management` subsystem for BLE profiles and the runtime-sensor-rotate
subsystem for the encoder, and the `zmk-feature-device-info` subsystem
reports which build is running (see [Firmware](../reference/firmware/#modules)).

## What it will do

- **Status**: whether the halves are connected to each other, and the
  connection to the host.
- **BLE profiles**: list, switch, rename and unpair.
- **Battery** levels.
- **Output priority**: USB or BLE first.
- **Firmware updates**: check the running firmware against the
  [GitHub releases](https://github.com/rdlu/sofleii/releases), and flash
  from the tool.
- **The repo's routines** as CLI subcommands and TUI actions, instead of
  remembering task names: `flash-release`, `flash-release-reset`,
  `build-flash`, `doctor`, `deps-check` and `draw`. Today they are
  [mise tasks](../building/).

## Roadmap

- [#44](https://github.com/rdlu/sofleii/issues/44): sofleii, the
  Rust/ratatui CLI and TUI for status, control and repo routines (the epic).
- [#45](https://github.com/rdlu/sofleii/issues/45), firmware: forward the
  right half's battery level to the host.
- [#46](https://github.com/rdlu/sofleii/issues/46), firmware: expose
  build and version info over Studio RPC, which is what makes update checks
  possible.
- [#47](https://github.com/rdlu/sofleii/issues/47): firmware update check
  and flashing from the tool.
- [#48](https://github.com/rdlu/sofleii/issues/48): run the repo's
  user-facing mise routines from the CLI and TUI.

Until it exists, use [Studio](../studio/) for profiles and live keymap
edits, and the mise tasks for everything else.
