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
> Roadmap: TODO(issue-link)

**sofleii** will be a command-line tool and a terminal UI (Rust,
[ratatui](https://ratatui.rs/)) for managing the keyboard from a terminal.
It will live in this repo, next to the firmware it talks to.

## How it talks to the keyboard

Through the **Studio RPC protocol**, the same one the Studio web app uses,
to the **left half**, over USB serial or Bluetooth. The firmware already
carries what it needs: v3.0.0 kept cormoran's custom-RPC plumbing, with the
`ble-management` subsystem for BLE profiles and the runtime-sensor-rotate
subsystem for the encoder (see [Firmware](../reference/firmware/#modules)).

## What it will do

- **Status**: whether the halves are connected to each other, and the
  connection to the host.
- **BLE profiles**: list, switch, rename and unpair.
  TODO(issue-link)
- **Battery** levels. TODO(issue-link)
- **Output priority**: USB or BLE first. TODO(issue-link)
- **Firmware updates**: check the running firmware against the
  [GitHub releases](https://github.com/rdlu/zmk-sofle/releases).
  TODO(issue-link)
- **The repo's routines** as CLI subcommands and TUI actions, instead of
  remembering task names: `flash-release`, `flash-release-reset`,
  `build-flash`, `doctor`, `deps-check` and `draw`. Today they are
  [mise tasks](../building/). TODO(issue-link)

Until it exists, use [Studio](../studio/) for profiles and live keymap
edits, and the mise tasks for everything else.
