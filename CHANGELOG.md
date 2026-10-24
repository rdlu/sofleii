# Changelog

All notable changes to the Eyelash Sofle firmware and keymap, released as
`vX.Y.Z` tags. The `sofleii` CLI/TUI has its own changelog in
[`sofleii/CHANGELOG.md`](sofleii/CHANGELOG.md).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/). Generated
by [git-cliff](https://git-cliff.org) from the commit history: edit
`cliff.toml`, not this file.

## [Unreleased]

### Docs

- note Dependabot security updates are enabled ([2cd179c](https://github.com/rdlu/sofleii/commit/2cd179c4c469359683f015e83c5ad6f12e8c3407))

### Dependencies

- bump crates and rust-cache, add Dependabot ([33f5066](https://github.com/rdlu/sofleii/commit/33f50666758dfcbd4db6a5d0ee4acc804795c229))

## [v3.2.0](https://github.com/rdlu/sofleii/compare/v3.1.0...v3.2.0) - 2026-10-21

### Features

- **keymap:** MYGAME custom game slot at picker joystick right ([8999ef3](https://github.com/rdlu/sofleii/commit/8999ef314df2cc48fb5fd960e20b66f081a794d5))

### Fixes

- **firmware:** build ble-management from the rdlu fork with the get_split_info fix ([e3f6321](https://github.com/rdlu/sofleii/commit/e3f632156457d177131f506f6805c401c345c195))

### Docs

- **claude:** latest release v3.1.0 ([b45d3cf](https://github.com/rdlu/sofleii/commit/b45d3cf4f436d414b4bd7a8c8f990229b76d62fb))
- MYGAME custom slot on the site, PDF, README and CLAUDE.md ([0858966](https://github.com/rdlu/sofleii/commit/08589661d123bb6f6112fc3feef9deb68e0c403c))

## [v3.1.0](https://github.com/rdlu/sofleii/compare/v3.0.0...v3.1.0) - 2026-10-19

### Features

- **layout:** own physical layout from ZMK's Sofle, JSON generated from it ([a40ed8f](https://github.com/rdlu/sofleii/commit/a40ed8fb594a6b8a76b1c5304f9dc1a9fa6f2054))
- **site:** printable keymap PDF, built in CI from the site's own grids ([4913aae](https://github.com/rdlu/sofleii/commit/4913aae44e3e07fff0a342aa162cea440773d189))
- **site:** keymap PDF in A4 portrait, three layers a page ([6a3c2e1](https://github.com/rdlu/sofleii/commit/6a3c2e1887698ee1ccd267a3c8a61466940919b1))
- **site:** keymap PDF in colour, carried by lines on white ([67f2507](https://github.com/rdlu/sofleii/commit/67f250790ceee054561c8f01f2e5e0bb50da5cdc))
- **firmware:** forward the right half's battery level to the host ([#45](https://github.com/rdlu/sofleii/pull/45)) ([2373850](https://github.com/rdlu/sofleii/commit/2373850167742bd9e03822d5d9de6e0562b80c37))
- **firmware:** build/version info over Studio RPC (zmk-feature-device-info) ([1e468b7](https://github.com/rdlu/sofleii/commit/1e468b7d60db959bc0ae967abe8bf1556e479421))

### Fixes

- **pages:** deploy when the changes job was skipped ([1c12ef9](https://github.com/rdlu/sofleii/commit/1c12ef94dc0692eabf00563de1412074fc132588))
- **site:** label the held key in the layer grids ([a7b316f](https://github.com/rdlu/sofleii/commit/a7b316f90734795130730ea9a8303c0e156d183e))
- **site:** grid labels wrap at | and +, never mid-word ([25a2853](https://github.com/rdlu/sofleii/commit/25a2853d96335d579ee2b7ebcab5a6822229a089))

### Docs

- **site:** scaffold the sofleii site (Hugo + vendored Hextra) ([9bac0f7](https://github.com/rdlu/sofleii/commit/9bac0f7a9f30d6ba55e73ee13287f5be10f89666))
- **site:** firmware guides, reference, and the planned sofleii tool ([0f8527e](https://github.com/rdlu/sofleii/commit/0f8527eb15671ee8d655ed6eee647fa76d5c12e8))
- **site:** link the sofleii roadmap issues ([#44](https://github.com/rdlu/sofleii/pull/44)-[#48](https://github.com/rdlu/sofleii/pull/48)) ([bfde208](https://github.com/rdlu/sofleii/commit/bfde208fe89d62501a2552a260563e1132e863ab))
- **claude:** v3.0.0 is out; active branch and the docs site section ([55472f9](https://github.com/rdlu/sofleii/commit/55472f9ee3e41f98854cc00016970121962b121b))
- **readme:** link the docs site, list the docs tasks ([72ff8db](https://github.com/rdlu/sofleii/commit/72ff8dbc49dc621b64af4df353d82838a13ad3fb))
- **site:** generated layer grids (layer shortcode) ([d55a1c4](https://github.com/rdlu/sofleii/commit/d55a1c4b1d8ba3b13e0fb3e9e54dea7c1373cce5))
- **site:** the keymap as its own top-level section ([88ed556](https://github.com/rdlu/sofleii/commit/88ed556a64db18cf20aaeb334b6cf62f59688490))
- **site:** module-owned SYS|NUM keys, keymap Overview link, scoped ligatures ([3b6516f](https://github.com/rdlu/sofleii/commit/3b6516f59994edb05631380e432c653cc8d27050))
- **readme:** a short landing page that defers to the site ([af3cc3e](https://github.com/rdlu/sofleii/commit/af3cc3eca0a3f752bba37d58d21d70e2b14195d2))
- **site:** building on the system Python ([f70d7ab](https://github.com/rdlu/sofleii/commit/f70d7ab6b563197b88efe33e56072d4fa506aeb7))
- **site:** pkg_resources package name on plain Arch ([ecc5ad6](https://github.com/rdlu/sofleii/commit/ecc5ad62425d18997d085ef54015014af04bc6a3))
- fix the stale hold-tap and keymap statements ([#50](https://github.com/rdlu/sofleii/pull/50)) ([cfc2494](https://github.com/rdlu/sofleii/commit/cfc249464c84a8447031262975106f6910a56b71))
- **license:** no unlicensed vendor files left; credit ZMK's Sofle layout ([4a51c6f](https://github.com/rdlu/sofleii/commit/4a51c6f570aeb3ed8853420461acb82b75eb1ac3))
- **keymap:** game macros' colour shows on USB too ([245d821](https://github.com/rdlu/sofleii/commit/245d821764ec5ab637ff6fdfb512acd8f6cfd68c))
- **site:** give sofleii its own magenta palette ([1df10b1](https://github.com/rdlu/sofleii/commit/1df10b1c0339bec0c73dcd76082d9eb4f12a8cc4))
- **claude:** layout JSON changes do redeploy Pages ([27c4890](https://github.com/rdlu/sofleii/commit/27c48909ee1b945dfecfe7c022ae07e7b5a0d688))
- **site:** catchier home-page phrases ([4df76f6](https://github.com/rdlu/sofleii/commit/4df76f648f4528ea1403de6f59f91e08f8449498))
- **site:** memory budget and battery levels with the [#45](https://github.com/rdlu/sofleii/pull/45) proxy ([d6a7a88](https://github.com/rdlu/sofleii/commit/d6a7a8868e3c121c5b1c1656502dd976e9932e76))
- **firmware:** device-info module, zmk_config_version, memory budget ([a36b9dd](https://github.com/rdlu/sofleii/commit/a36b9ddd77a384069011aa0edd5a5c71eaa9438f))
- **claude:** device-info module, zmk_config_version derivation, CI fix ([b75d60c](https://github.com/rdlu/sofleii/commit/b75d60cd7a838e1af35439d68856006dc96f665c))

### CI

- **pages:** build and deploy the sofleii site to GitHub Pages ([b52b355](https://github.com/rdlu/sofleii/commit/b52b355243779429439f391ac0c16f4e2ab2e651))
- split firmware and docs workflow triggers ([f31d0ad](https://github.com/rdlu/sofleii/commit/f31d0adf12079dc381c79327a2d3f2001d7460d6))

### Build

- use the system Python, drop the 3.12 pin and .venv ([c3b3099](https://github.com/rdlu/sofleii/commit/c3b30990e7675d22468af22e20aa75ebd80347fb))
- **setup:** separate pacman and AUR install hints ([e565f05](https://github.com/rdlu/sofleii/commit/e565f05b6b2e0d061b28f5a8b2781253a030056f))

## [v3.0.0](https://github.com/rdlu/sofleii/compare/v2.8.0...v3.0.0) - 2026-10-17

### Features

- **mise:** add doctor / doctor-fix / doctor-ci workspace drift checks ([5b1f066](https://github.com/rdlu/sofleii/commit/5b1f0668e183e03714ccc988cf3ae79aa1714679))
- **firmware:** move to ZMK main / Zephyr 4.1 (cormoran main+dya) ([92f6c35](https://github.com/rdlu/sofleii/commit/92f6c35b49f84cdb4022e431e44931297daa5977))

### Docs

- memory budget from CI run 39026110927; note v2.8.0 latest, v3.0.0 planned ([bba130d](https://github.com/rdlu/sofleii/commit/bba130d403839eb8cd0905e3e8a4c8858759955d))
- soft-off combo is Q+S+Z (positions 14 28 40), not Q+A+Z ([44fa4bf](https://github.com/rdlu/sofleii/commit/44fa4bf1c59f3660d281af1d6ae9fde92d0ec304))

### Dependencies

- pin zmk + zephyr to SHAs, add deps-check for upstream updates ([e3e13ce](https://github.com/rdlu/sofleii/commit/e3e13cee5007b891181370be4b8bd4c3e238d997))

## Earlier releases

Releases before v3.0.0 (v1.0.0 to v2.8.0: the ZMK v0.3 / Zephyr 3.5 base,
gaming mode, ZMK Studio, the zmk-rgb-layer module, the No Man's Sky layer)
predate this changelog. Their notes are on the
[GitHub releases page](https://github.com/rdlu/sofleii/releases).

