# Changelog: sofleii CLI/TUI

All notable changes to the `sofleii` crate (the CLI and TUI in this
directory), released as `sofleii-vX.Y.Z` tags. The firmware has its own
changelog in [`../CHANGELOG.md`](../CHANGELOG.md).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/). Generated
by [git-cliff](https://git-cliff.org) from the commit history: edit
`cliff.toml`, not this file.

## [Unreleased]

### Dependencies

- bump crates and rust-cache, add Dependabot ([33f5066](https://github.com/rdlu/sofleii/commit/33f50666758dfcbd4db6a5d0ee4acc804795c229))

## [sofleii v0.1.1](https://github.com/rdlu/sofleii/compare/sofleii-v0.1.0...sofleii-v0.1.1) - 2026-10-21

### Fixes

- update check judges firmware by its inputs, not commits ([e2548c9](https://github.com/rdlu/sofleii/commit/e2548c900428ee588455355a5c76d99054865a61))

## [sofleii v0.1.0](https://github.com/rdlu/sofleii/releases/tag/sofleii-v0.1.0) - 2026-10-21

### Features

- vendor Studio RPC and ble_management protos ([b3e7f62](https://github.com/rdlu/sofleii/commit/b3e7f62dd615bb7782be7416ff412e934fcdf752))
- CLI spike with info and profiles over USB Studio RPC ([35263d7](https://github.com/rdlu/sofleii/commit/35263d77609e3754f9562e0301616b7555832eb2))
- status command with --json and --format waybar ([90c9421](https://github.com/rdlu/sofleii/commit/90c942131878b702b82dcefda44c0690fc4b1c31))
- battery command over BlueZ GATT; fill status battery ([06196aa](https://github.com/rdlu/sofleii/commit/06196aae8a134d8616d50c703be73966aaeb6ad4))
- BLE transport for Studio RPC; --transport auto (default) ([8c839e4](https://github.com/rdlu/sofleii/commit/8c839e42ee2dcb98ae414835a4044dcd87a6efdb))
- info reads firmware build info from zmk__device_info ([f9a665f](https://github.com/rdlu/sofleii/commit/f9a665f475c344a0bc33a2fdacd5ad5ce8997641))
- update check against the latest GitHub release ([#47](https://github.com/rdlu/sofleii/pull/47)) ([dc486d0](https://github.com/rdlu/sofleii/commit/dc486d046e7e7db30b976a5f72c2d1b4186a5797))
- update check names the running release ([861c1ad](https://github.com/rdlu/sofleii/commit/861c1ad26bbbf4f5150ddb990a11c76c32a5de7d))
- fold update check's diverged status into behind ([1f3fc9d](https://github.com/rdlu/sofleii/commit/1f3fc9d33fa4e29d74b69c3ca72d1269836fa79e))
- profile switch/rename/unpair and output usb|ble writes ([9ad8053](https://github.com/rdlu/sofleii/commit/9ad8053c169445291bba036c272dea86b7b61322))
- TUI skeleton and core diagnostics via log ([53db548](https://github.com/rdlu/sofleii/commit/53db5482637640bb975b303e06433a73de34c75e))
- connect in the core, Send transports, random first id ([962ef06](https://github.com/rdlu/sofleii/commit/962ef06bf11797550883453ce0da93cb6e9521c0))
- session threads and the state reducer ([27d7236](https://github.com/rdlu/sofleii/commit/27d723641b83eea66dcc94871dcff376b2d896b6))
- live read-only TUI over the session ([9bf6771](https://github.com/rdlu/sofleii/commit/9bf6771ab6287d17caf12aa219c00b77730fa6e8))
- show the split link from get_split_info ([952d0c1](https://github.com/rdlu/sofleii/commit/952d0c1cf3530d7835f98b9a663d1ed4430f6c25))
- SVG TUI screenshots from the demo fixture ([053815d](https://github.com/rdlu/sofleii/commit/053815d9f283faf31b659fe0ef1d7aa47f5d375d))
- switch profile and toggle output from the TUI ([2ca449c](https://github.com/rdlu/sofleii/commit/2ca449c64152e01350b5c8d33db9f6dcfdb3bbe9))
- confirm TUI writes with y only ([83b54f5](https://github.com/rdlu/sofleii/commit/83b54f52aa63e617f9d77664d3775c81e51ee4bd))
- output BLE follows who the active profile is ([3030184](https://github.com/rdlu/sofleii/commit/3030184a2e21f4e5ebb005ec61e919c732fb838e))
- update check covers the CLI too, shares the 6 h cache ([9ad2ecf](https://github.com/rdlu/sofleii/commit/9ad2ecfd5af0a10b0599900180d6bc3054c46a26))
- keymap show and keymap diff ([#76](https://github.com/rdlu/sofleii/pull/76), CLI) ([cc29532](https://github.com/rdlu/sofleii/commit/cc2953219d67ea77efc16a35640b9bf6b6875652))

### Fixes

- steady link state on deaf links; info-level log pane ([fa3ab87](https://github.com/rdlu/sofleii/commit/fa3ab87d6ac25c668026a9b8af88b30e0ddf4d48))
- always serialise the battery state as an explicit null ([c10ef5b](https://github.com/rdlu/sofleii/commit/c10ef5b0ba837d193d5b61b9393164a77716ec5f))
- re-read the batteries on a split edge; settle a rejoin's 0% ([fcbc08c](https://github.com/rdlu/sofleii/commit/fcbc08cada2a55eba99ddf8a24acaa5d4016cf74))

### Docs

- battery command, JSON, waybar module, BlueZ Battery1 caveat ([48b12cd](https://github.com/rdlu/sofleii/commit/48b12cd7a42dff07f6a0361fde0b2c4ff54cc38d))
- transports, auto selection, BLE requirements and limits ([f9c4df6](https://github.com/rdlu/sofleii/commit/f9c4df6f890d799b4052267086416debee99cdcf))
- user docs on the site, CLI and JSON section, README overhaul ([cc89794](https://github.com/rdlu/sofleii/commit/cc89794cc36be5a21322f7fafeaf0c0b77c80ba3))
- release workflows, update check's CLI part, install note ([eb6b975](https://github.com/rdlu/sofleii/commit/eb6b9752682ca56b734716c1b23d83a7f9eb54aa))

### Build

- size-tuned release profile (3.3 MB -> 1.6 MB) ([ce517f1](https://github.com/rdlu/sofleii/commit/ce517f19955e88012d98f2ff20ac4b19f8e3bead))

