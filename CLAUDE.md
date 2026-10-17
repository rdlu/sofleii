# ZMK Eyelash Sofle — Agent Context

This repo is a ZMK firmware configuration for an **Eyelash Sofle** split keyboard (NRF52840, nice!nano v2). The owner actively maintains and tunes the keymap — expect frequent changes to keys, layers, and hold-tap timing.

## Repo layout

```
config/
  eyelash_sofle.keymap   # main keymap — the file you'll edit most
  eyelash_sofle.conf     # ZMK Kconfig options
  eyelash_sofle.json     # physical key layout, GENERATED from the shield's eyelash_sofle-layouts.dtsi (`mise run layout`)
  west.yml               # west manifest pinning ZMK version
keymap-drawer/
  eyelash_sofle.svg      # rendered keymap image — regenerate after every edit
  eyelash_sofle.yaml     # intermediate parsed representation
KEYMAP.md                # pointer to the site's keymap pages (the old hand-written reference drifted)
.mise.toml               # tasks: setup / build-* / flash-* / draw
keymap_drawer.config.yaml # keymap-drawer render config (icons, colors, labels)
scripts/layout-to-json.py # shield .dtsi physical layout -> config/eyelash_sofle.json
```

## Toolchain (already set up on the owner's machine)

- **Python**: the **system** Python from pacman (3.14 as of 2026-10-17), no venv. The owner's global mise config sets `disable_tools = ["python"]`, so the project no longer pins one. Build deps are distro packages: `python-yaml`, `-pyelftools`, `-packaging`, `-protobuf` and `-pkg_resources` from the repos (`pkg_resources` is a separate package on CachyOS; plain Arch ships it in `python-setuptools`), plus **`python-west` and `python-pykwalify` from the AUR** (not in the official repos; already installed here). `mise run setup` checks them and prints separate pacman and AUR lines. `uvx` is still used for keymap-drawer.
- **west workspace**: initialized; ZMK source at `zmk/` is **cormoran's fork** on the ZMK-main line (`main+dya`, **Zephyr 4.1**, HWMv2) plus 2 cormoran custom-RPC modules (`ble-management`, `runtime-sensor-rotate`); Zephyr at `zephyr/`
- **Zephyr SDK**: 0.17.0 at `tools/zephyr-sdk-0.17.0` — do NOT use AUR `zephyr-sdk` (SDK 1.0, targets Zephyr 4.2+; we are on 4.1)
- **Serial access**: user is in `uucp` group (required for DYA/ZMK Studio over USB)

On a **new machine**, run `mise run setup` after cloning.

## Common workflows

```bash
mise run doctor        # verify the west workspace matches config/west.yml (run this first if a build fails oddly)
mise run deps-check    # see whether any pinned dependency has upstream updates available
mise run draw          # re-parse keymap + render SVG (do this after every keymap edit)
mise run build-left    # build left half (DYA Studio enabled)
mise run build-right   # build right half
mise run flash-left    # copy .uf2 to mounted NICENANO (double-tap reset first)
mise run docs          # docs site dev server on http://127.0.0.1:1313/ (site/)
mise run docs-build    # minified docs site build into site/public/
mise run docs-pdf      # build the site + print the keymap PDF (site/public/sofleii-keymap.pdf); docs-pdf-open opens it
```

Alternatively, push to the branch and download `.uf2` artifacts from GitHub Actions
(`.github/workflows/build.yml` triggers on `config/**` changes).

## Keymap structure

Eight layers, accessed as follows:

| # | Name    | Access              |
|---|---------|---------------------|
| 0 | BASE    | always on           |
| 1 | NAV     | hold `mo1` (left thumb) |
| 2 | CODE    | hold `mo2` (right thumb, pos 60) |
| 3 | MEDIA   | hold `mo3` (right thumb) |
| 4 | SYS\|NUM | hold `mo4` (left thumb) |
| 5 | GAME?   | `&to 5` from SYS (right-thumb outer key, pos 63) — transient picker |
| 6 | MINECRFT | picker joystick ↑ (`&GAME_MC_TO` → `&to 6`) |
| 7 | GAME    | picker joystick ↓ (`&GAME_GEN_TO` → `&to 7`) — generic FPS/MOBA |
| 8 | NOMANSKY | picker joystick ← (`&GAME_NMS_TO` → `&to 8`) — No Man's Sky |

Right thumb cluster (BASE): `SPACE · mo(CODE) · CapsWord · ralt_mt(RALT, F12) · mo(MEDIA)`
**DEL = `mo(CODE) + BSPC`** — hold the CODE key (right thumb, pos 60) and tap the
left-thumb BSPC position (pos 57). Cross-hand, no hold-tap cost. CODE is a plain
`&mo 2`; the old `&lt 2 DEL` was removed in `ebc0602` for hold-tap lag, which left
BASE with no DEL at all until this was added. DEL is also on MEDIA pos 30.
Left thumb cluster (BASE): `mo(SYS|NUM) · F11 · LGUI · mo(NAV) · BSPC`

### Gaming mode (layers 5–8)

Locked gaming layers that strip all HRM/hold-tap/caps_word so WASD + modifiers are pure `&kp`. Designed in issue #11; re-applied onto current main in `feat/gaming-mode` (the older `gaming` branch / `v2.0.0-rc1` was built on a pre-v1.4 base and is superseded).

- **Entry:** hold `mo(SYS|NUM)` → tap SYS **pos 63** (`&to 5`). The original design used the SYS joystick-center (pos 58), but PR #25 turned the SYS joystick into a mouse (pos 58 = `&mkp LCLK`), so entry moved to the free right-thumb outer key.
- **Picker (layer 5, "GAME?"):** joystick ↑ → MINECRFT, ↓ → generic GAME, ← → NOMANSKY, center → `&to 0` (cancel). All other keys `&none`.
- **Game chassis (layers 6, 7 & 8):** `LCTRL`@39 (pinky), `LSHIFT`@56 (left thumb inner), `SPACE`@57 + @59 (both thumbs), `BSPC`@60, `LALT`@61, `F13`@62 (PTT), `mo(MEDIA)`@63. Joystick = audio cluster (Vol±/Mute/PlayPause) + center Enter. pos 0: MC = `F3`, generic = `` ` ``, NMS = `F12` (Steam screenshot).
- **NOMANSKY (8) swaps `LSHIFT`/`LCTRL`** → `LSHIFT`@39 (pinky), `LCTRL`@56 (thumb). Requested explicitly: NMS runs on Shift, Ctrl is unused. It does move the held run modifier off the thumb onto the pinky — the reverse of the MC/GAME rationale — so if the owner ever reports run being awkward, flipping those two positions back is the fix.
- **Exit/switch combos:** `0 + 12` → `&GAME_EXIT` (RGB off + `&to 0`); `0 + 5` → `&to 5` (re-pick). Both scoped `layers = <6 7 8>`.
- **RGB feedback** (`GAME_MC_TO`/`GAME_GEN_TO`/`GAME_NMS_TO` set `RGB_COLOR_HSB`; NMS is `0,0,50` — sat 0 = white/silver): visible on both USB and battery since v2.6.0 (`AUTO_OFF_USB=n`). `GAME_EXIT` does `RGB_OFF` to return to the dark smart-off base. OLED `display-name` is still the primary indicator.

## Key position numbering

The keymap has 64 positions (used in `hold-trigger-key-positions`):

```
Rows 0–3: 13 keys each (6 left + 1 encoder-click + 6 right), positions 0–51
Row 4:    12 keys      (5 left + 1 encoder-click + 5 right, + 1 encoder), positions 52–63

Left half:  cols 0–5 of each row
Right half: cols 7–12 of each row (col 6 = encoder click)
```

Left-side positions:  `0-5, 13-18, 26-31, 39-44, 52-57`
Right-side positions: `7-12, 20-25, 33-38, 46-51, 58-63`

## Hold-tap behaviors

Five behaviors in the `behaviors {}` block. The site's **Home-row mods & timing**
page (`site/content/keymap/hold-tap.md`) renders their table straight from the
keymap, so treat the keymap as the source of truth for the numbers below.

| Behavior | Used on | Flavor | `require-prior-idle-ms` | Positional restriction |
|----------|---------|--------|-------------------------|------------------------|
| `hml` | S(LALT) D(LCTRL) | balanced | 230 | right-hand keys **+ left S/D/F (28–30)** |
| `hml_idx` | F(LSHFT) | hold-preferred | 70 | same list as `hml` |
| `hmr` | L(LALT) K(RCTRL) | balanced | 230 | left-hand keys incl. centre column 6/19/32/45 **+ right J/K/L (34–36)** |
| `hmr_idx` | J(RSHFT) | hold-preferred | 70 | same list as `hmr` |
| `ralt_mt` | right-thumb F12 (tap = F12, hold = RALT) | balanced | 230 | none — AltGr combos span both hands and live on the thumb |

Shared: `tapping-term-ms=280`, `quick-tap-ms=175`. All four HRM behaviors set
`hold-trigger-on-release`; `ralt_mt` doesn't.

- **The same-hand home-row positions in each trigger list are intentional**
  (`12fb68e`, #17): they let two mods on one hand (Ctrl+Shift on D+F or K+J)
  resolve as holds. With `hold-trigger-on-release` the positional check runs
  when the other key is *released*, so same-hand letter rolls still come out
  as taps. Don't "clean them up" to a strict opposite-hand list.
- **Index variants** (`*_idx`): a shorter idle window and `hold-preferred` so
  Shift on F/J commits on the next key's press. A 230 ms idle made CamelCase
  and fast rolls feel sluggish (after Sunaku's home-row-mods guide).

## After every keymap edit

1. Run `mise run draw` to regenerate the SVG
2. Stage `config/eyelash_sofle.keymap`, `keymap-drawer/eyelash_sofle.yaml`, `keymap-drawer/eyelash_sofle.svg` together
3. Commit and push — GitHub Actions will build the firmware automatically
4. Update the open PR description to reflect the current state of all layers and behaviors
5. Post a PR comment summarising what changed in this interaction (what the user asked for, what was changed and why)

## Branch / PR conventions

- Always work on a feature branch and keep an open PR — **never commit directly to `main`** unless explicitly asked
- No long-lived feature branch. Recent docs/CI work: the docs site (PR #49), separate firmware/docs workflow triggers (#52, #53; see **Which workflow runs when**), the keymap as its own site section plus a landing-page README (#54).
- Latest release is **v3.0.0** (2026-10-17, PR #43, merged `dc7a81c`): the firmware moved from the v0.3 / Zephyr 3.5 base to cormoran's `main+dya` (ZMK main, Zephyr 4.1), the shield was vendored into `config/boards/shields/`, and the DYA modules were trimmed to `ble-management` + `runtime-sensor-rotate`. Upgrading needs a settings reset on both halves + re-pair. That is the groundwork for the planned **sofleii** Rust/ratatui CLI + TUI (epic #44, issues #45–#48). v2.8.0 (2026-09-02) added the No Man's Sky layer. v2.7.0 added the event-driven `zmk-rgb-layer` module (per-layer colours, switchable base moods, master brightness, reboot persistence, split resync; PR #34, closed #33). The in-repo module is built in CI via `-DZMK_EXTRA_MODULES` in `build.yaml` (see the in-repo-module CI gotchas below). Gaming mode (PR #27), Studio firmware (PR #31), and the v2.6.0 macro-based RGB feedback (PR #32, since superseded by the module) are also in `main`. The reactive per-column underglow effect is deferred to issue #35. Start a new branch/PR for the next topic.
- Main branch is `main`
- **License: MIT** (`LICENSE`, © 2026 Rodrigo Dlugokenski). Its "Third-party components" section lists what it does not cover: the vendored site parts, files with their own SPDX headers, the physical layout adapted from ZMK's MIT Sofle layout (`eyelash_sofle-layouts.dtsi` + the generated `config/eyelash_sofle.json`), the vendor-history note (no unlicensed vendor file remains at HEAD since #51) and `keymap_drawer.config.yaml`. The vendor's 3D model, original README and other-board renders were removed; the Hardware docs page links them in the vendor repo instead. Keep that list current when vendoring anything. The future `sofleii/` crate uses `license = "MIT"` in Cargo.toml.
- The GitHub repo is **`rdlu/sofleii`** (renamed from `rdlu/zmk-sofle` on 2026-10-17; GitHub redirects the old name), and the local checkout is `~/Projects/sofleii`. `a741725193/zmk-sofle` is the vendor's repo, a different project.

### PR description hygiene

The PR description is the canonical summary of everything on the branch. Keep it current:
- After every push, update the **Summary** section to reflect the actual current state of all layers and behaviors
- If a behavior or key was changed, the old description is wrong — fix it

### PR comments as a change log

After each push, post a comment on the PR documenting what happened in that interaction:
```
gh pr comment <number> --body "..."
```
Include: what the user requested, what was changed (layer/key/behavior), and any non-obvious rationale (e.g. why ralt_mt has no positional restriction). This creates a useful history of decisions for future sessions.

## Docs site (sofleii)

- **sofleii** = "Sofle Interactive Interface": the name of the project's site and of the planned CLI/TUI. Site at https://sofleii.dlu.one (GitHub Pages, custom domain via `site/static/CNAME`; DNS CNAME → rdlu.github.io).
- Lives in `site/`: Hugo + Hextra v0.13.0 **vendored** in `site/themes/hextra/`, FlexSearch vendored in `site/assets/lib/flexsearch/` — the build needs no network. Template and skin come from plugii's site (`~/Projects/plugii/site/`): iA Writer Duo + 0xProto self-hosted in `site/static/fonts/`, square panes, `┌─ Title ───` headings (`site/assets/css/custom.css`).
- `mise run docs` (dev server, 127.0.0.1:1313) / `mise run docs-build`. hugo comes from **pacman**, not mise; the version is pinned in **`site/.hugo-version`** (inside `site/`, so a bump only redeploys the docs), which CI's `.github/scripts/install-hugo.sh` reads (checksum-verified) and the docs tasks warn about on mismatch. Bump that one value to move CI.
- **Single source of truth via `module.mounts`** in `site/hugo.toml`: `keymap-drawer/eyelash_sofle.{svg,yaml}` → `assets/keymap/`, `config/eyelash_sofle.{keymap,json}` and `config/west.yml` → `assets/config/`. Shortcodes parse them on every build: `keymap-svg` (the render), `layer "NAME"` (one layer's grid on the physical layout from the `.json`; legends from keymap-drawer's parsed `.yaml` so they match the SVG, glyphs from the SVG's `<defs>`, raw bindings and macro text from the `.keymap`; the build **warns** if the `.yaml` is stale vs the keymap — run `mise run draw`; `&none` keys a module acts on, e.g. zmk-rgb-layer's SYS|NUM brightness/mood keys, get a label from `site/data/keymap_overrides.yaml`, and the build warns if one of those positions stops being `&none` — keep it in sync with `zmk-rgb-layer/src/rgb_layer.c`), `keymap-behaviors`, `keymap-combos`, `west-pins`. **Regenerate nothing by hand** — the SVG/YAML come from `draw.yml`. Prose under each layer grid is hand-written, so after keymap edits re-read `site/content/keymap/*.md` and the guides.
- **Site sections:** `site/content/docs/` (guides, reference, project) and `site/content/keymap/` (overview, one page per layer, gaming layers, home-row mods & timing, combos). Each is its own top-level menu entry with its own sidebar: `keymap/_index.md` cascades `type: docs`, and the sidebar's root is the page's first section. Old `/docs/reference/{keymap,hold-tap,combos}/` URLs are `aliases:` redirects. The keymap sidebar's "Overview" link to the section index comes from `sidebar.overview` in `keymap/_index.md` (a marked patch in `site/layouts/_partials/sidebar.html`). Ligatures are off only in the grids and in inline code under `/keymap/` (`custom/head-end.html`); the rest of the site keeps them.
- **Keymap PDF** (`/sofleii-keymap.pdf`, linked from the Keymap overview, the home hero and the README): the print view `/keymap/print/` (`site/content/keymap-print.md` + the standalone `site/layouts/keymap-print.html`, no baseof; `assets/css/keymap-print.css`) printed with headless Chromium by **`.github/scripts/print-keymap-pdf.sh`** — the one script both the Pages build job and `mise run docs-pdf` run (serves `public/` over `python3 -m http.server`, so it assumes a baseURL without subpath; fails if the PDF is missing or < 50 KB). Page 1 is the hand-written basics manual in the page's Markdown (re-check it after keymap edits, like the layer pages; it must stay one page), then one A4-portrait sheet per entry in its `sheets` front matter (three layers a page: BASE+NAV+CODE, MEDIA+SYS|NUM+GAME?, the three game layers — 4 pages in all), grids drawn by `_partials/zs/layer.html` (the same partial the `layer` shortcode calls; `legend`/`caption` options). Always light and grayscale-safe. Out of the sidebar, pager (it sits outside `content/keymap/`), search and sitemap. The footer's firmware version is `params.firmware.version` in `site/hugo.toml`, which the home hero badge also reads: **bump it with each release**. On CI the browser is the runner image's Chrome (`CHROME_BIN`). The old weasyprint `draw-pdf*`/`open-pdf` tasks are gone.
- **Pages workflow** (`.github/workflows/pages.yml`): push to main on `site/**` or the workflow/script only; firmware-file pages (keymap render, hold-tap/combo tables, west pins) refresh through **`workflow_run` on "Draw Keymap"** (completed, main). Needed because `draw.yml` commits the SVG with `GITHUB_TOKEN`, and pushes made with `GITHUB_TOKEN` never trigger other workflows. A `changes` job deploys only if Draw succeeded **and** `git diff --name-only <trigger>^ main` touches `config/eyelash_sofle.{keymap,json}`, `config/west.yml` or `keymap-drawer/eyelash_sofle.{svg,yaml}` — so west.yml changes (no Draw commit) still deploy, `.conf`-only changes don't, and nothing deploys twice.

### Which workflow runs when

| Push to main touching | Build firmware | Draw Keymap | Pages |
|---|---|---|---|
| `site/**` (incl. `site/.hugo-version`), `.github/scripts/{install-hugo,print-keymap-pdf}.sh` | – | – | ✓ (push) |
| `config/eyelash_sofle.keymap` | ✓ | ✓ | ✓ once, via Draw |
| `config/west.yml` | ✓ | ✓ | ✓ once, via Draw (no Draw commit; diff check) |
| `config/eyelash_sofle.json` / layout `.dtsi` (via `mise run layout`) | ✓ | ✓ | ✓ once, via Draw (the grids read the `.json`) |
| other `config/**` (`.conf`, shield) | ✓ | ✓ | – (diff check skips) |
| `zmk-rgb-layer/**`, `build.yaml` | ✓ | – | – |
| `.mise.toml` | – | – | – |
| docs-only markdown (README, KEYMAP.md, CLAUDE.md…) | – | – | – |

Firmware **releases are manual** (`gh release create` on a merge commit, attaching that build's artifacts). The docs workflow never builds firmware. Release procedure also: bump `params.firmware.version` in `site/hugo.toml`, and once Pages has deployed, attach the keymap PDF: `curl -fLO https://sofleii.dlu.one/sofleii-keymap.pdf && gh release upload <tag> sofleii-keymap.pdf` (not automated).
- Hextra gotchas inherited from plugii: `hero-button` and `card` drop a subpath base (relURL with a leading `/`), so patched copies live in `site/layouts/`; sidebar group headings need undocumented **separator pages** (`site/content/docs/sidebar-*.md`, `sidebar.separator: true`, `build.render: never`), and the patched `pager.html` skips them; FlexSearch must stay vendored (`params.search.flexsearch.js`) or Hextra fetches it from jsDelivr.
- The **sofleii CLI/TUI** (planned; talks to the left half over Studio RPC, USB serial or BLE) will live in **this repo**, in a top-level `sofleii/` crate directory (not created yet). Its docs page is `site/content/docs/sofleii.md`.

## Important gotchas

- **Do not change the Zephyr SDK version** without verifying ZMK compatibility
- `keymap-drawer` CLI is `keymap` (not `keymap-drawer`) when invoked via `uvx --from keymap-drawer`
- Firmware is **shield-based**: build `-b nice_nano//zmk -DSHIELD="eyelash_sofle_left nice_view"` (HWMv2 board name since the ZMK-main move; default revision 2.0.0 = nice!nano v2). **The shield is vendored in-repo at `config/boards/shields/eyelash_sofle/`** — copied from the vendor module `a741725193/zmk-sofle` (which only targets ZMK v0.3 and hardcodes every DYA module) and adapted: DYA-only includes/Kconfig dropped, its `ext-power` node removed (the `nice_nano//zmk` board already defines `EXT_POWER` on P0.13 — two nodes would bind two drivers to one pin), `BT_CTLR` default removed (deprecated on Zephyr 4.1). Edit it freely; there is no longer an `eyelash_sofle` west project, so no same-name collision.
- **Studio + custom RPC** = cormoran's ZMK fork + modules → keymap editing over USB **and BLE** (DYA Studio web app still works), plus BLE-profile RPC from `ble-management` (the planned Rust/ratatui status TUI talks through it). Central-only studio/RPC/BLE Kconfig lives in the shield's `eyelash_sofle_left.conf`; keep it OUT of the shared `config/eyelash_sofle.conf`. Left half also gets the `studio-rpc-usb-uart` snippet (USB); right half is the peripheral (no central flags).
- **Snippet flag is case-sensitive: `-DSNIPPET`, NOT `-Dsnippet`.** Zephyr reads the uppercase `SNIPPET` CMake var; a lowercase `-Dsnippet=studio-rpc-usb-uart` is *silently ignored*, so the USB CDC-ACM transport never compiles in and the left half ships BLE-only (no `/dev/ttyACM`, Studio can't connect over USB). Verify a left build with `grep -E 'TRANSPORT_UART=|USB_CDC_ACM=' build/zephyr/.config`. CI (`build.yaml` `snippet:` key) is unaffected — this only ever bit the local `mise run build-left` task (fixed in v2.5.0).
- **BLE Studio editing on Linux needs a Chrome flag** — "BLE Not Supported on your Browser" = enable `chrome://flags/#enable-experimental-web-platform-features` (Web Bluetooth is off by default on Chrome/Linux) and relaunch. Not a firmware issue; BLE *HID* (typing) is unaffected. With `CONFIG_ZMK_STUDIO_LOCKING=n` there's no studio-unlock key.
- `soft_off` hold time is 2000 ms (combo: **Q+S+Z** simultaneously = positions 14 28 40; S is the `hml LALT` home-row mod. Wake = one press of reset)
- **`Invalid BOARD: nice_nano//zmk` (or historically `nice_nano_v2`) = a desynced local west workspace, not a keymap or manifest bug.** CMake aborts before it ever reads `config/`, and the "valid boards" list it prints is *Zephyr's* own boards. Cause seen 2026-09-02: the `zmk/` clone's git remote still pointed at `https://github.com/zmkfirmware/zmk` (a leftover from before the fork switch — west names remotes from the manifest but will **not** repoint an existing clone's remote), so `west update` could not fetch `v0.3-branch+dya` and left `zmk/` parked on an unrelated commit from a newer HWMv2 / Zephyr 4.1 line, where the board is `nice_nano/nrf52840/zmk` and `nice_nano_v2` does not exist. Diagnose and fix:
  ```bash
  mise run doctor       # diagnose: every project's HEAD vs its manifest-rev, remote URLs, board + Zephyr version
  mise run doctor-fix   # repair: repoint remotes at the manifest URLs, west update, drop the stale build/
  mise run doctor-ci    # optional: diff local checkouts against the SHAs the last green main build resolved
  mise run deps-check   # the opposite direction: has upstream moved past what the manifest pins?
  ```
  (That incident was on the old v0.3 base. The repo has since moved *to* the ZMK-main / Zephyr 4.1 line on purpose, so the failure is now the mirror image: a stale v0.3 tree has no `nice_nano//zmk`.) Correct state for the current manifest: `zmk` at `e5c9b691`, `zephyr` at `10ba6d0c` (**Zephyr 4.1.0** — if `zephyr/VERSION` says 3.x you are on the wrong tree), and `zmk/app/boards/nicekeyboards/nice_nano/nice_nano_nrf52840_zmk.dts` exists. Run `mise run doctor` before debugging any build failure that looks environmental — it is fast and offline.
- **CI is the source of truth for what the manifest resolves to.** A green CI build with a broken local build means the workspace drifted, not the manifest. `gh run view <id> --log | grep "=== updating"` prints the SHA every project resolved to; diff those against your local checkouts before touching `config/west.yml`.
- **All west projects are SHA/tag-pinned** — `zmk-behavior-runtime-sensor-rotate`, `zmk-module-battery-history` and `zmk-module-settings-rpc` used to track `main`. CI runs a fresh `west update` on every build, so CI and a stale local workspace could compile different module source. If you deliberately bump one, run `west update` and re-pin the resulting SHA in `config/west.yml`. **Everything is now SHA/tag-pinned, including `zmk` and `zephyr`.**
  - `zmk` tracks cormoran's `main+dya` (ZMK main + custom studio protocol + cormoran's BLE/activity extensions); pinned to `e5c9b691`. The v0.3 line (`v0.3-branch+dya`, `4493783e`) is what `main` used before `feat/zmk-v4`.
  - **`zephyr` needs an explicit override in `config/west.yml`.** `zmk/app/west.yml` declares it at the *branch* `v4.1.0+zmk-fixes`, so pinning `zmk` alone does NOT freeze Zephyr — west re-resolves that branch name on every update. A project declared in the top-level manifest wins over the same name in an imported one, so `config/west.yml` redeclares `zephyr` at `10ba6d0c`. (The v4 blocklist differs from v0.3's — `hal_silabs` is no longer blocked — so copy it fresh on every zmk bump rather than editing by hand.) That entry must replicate zmk's verbatim (`clone-depth: 1` + the whole `import: name-blocklist`); only `revision` differs. **If you bump `zmk` and its manifest's zephyr entry changed, mirror the change into `config/west.yml`.**
  - Each pin carries a `# track: <ref>` comment naming the upstream ref its SHA came from. `mise run deps-check` reads those and reports what has moved upstream — it is what replaces the silent auto-updates that pinning gives up. Run it periodically; nothing else will tell you a dependency has moved.
- **`mise run draw` must use `-j config/eyelash_sofle.json`, not `-z eyelash_sofle`** — the CI draw workflow resolves the physical layout from `config/<keyboard>.json`; the two sources differ by 1-2px, so a `-z` local render made every merge produce a `[Draw]` commit rewriting ~20 lines of SVG. With `-j` the local render is byte-identical to CI. The keymap-drawer version is pinned to the same value in `.mise.toml` and in `draw.yml`'s `install_version` — bump both together. The `.json` itself is generated from `config/boards/shields/eyelash_sofle/eyelash_sofle-layouts.dtsi` (the single source of key geometry; 58 Sofle keys from ZMK's `josefadamcik/sofle.dtsi`, plus the joystick and encoder press, in the 64-position order) by `scripts/layout-to-json.py`: edit the `.dtsi`, then `mise run layout` (`mise run draw` runs it first). Draw Keymap's `layout` job fails if the committed JSON is stale.
- **`ZEPHYR_TOOLCHAIN_VARIANT` is pinned for local builds** — it was mandatory on Zephyr 3.5, whose `FindZephyr-sdk.cmake` does an unquoted `if("zephyr" STREQUAL ${ZEPHYR_TOOLCHAIN_VARIANT})`, so an unset variable aborts configure with "Unknown arguments specified" (looks like a CMake-version bug, isn't). Both this and `ZEPHYR_SDK_INSTALL_DIR` are now pinned in `.mise.toml` `[env]`, so `mise run build-*` works from any shell.
- **Moving the checkout breaks the Zephyr SDK host tools.** `dtc` and the other host tools in `tools/zephyr-sdk-0.17.0` embed their absolute path in the ELF interpreter, so after a move CMake fails with `dtc failed with return code: no such file or directory`. Re-run `mise run setup` (it runs the SDK's `setup.sh -h -c`, which relocates them and re-registers the SDK with CMake). Seen on the 2026-10-17 rename from `zmk-sofle/`.
- **The left half's Studio build needs `google.protobuf` and `pkg_resources`** (nanopb codegen), from `python-protobuf` and `python-pkg_resources` (`python-setuptools` on plain Arch). A protobuf "gencode/runtime" VersionError means the protobuf runtime is older than the gencode; update the pacman package.
- **Module choice on the v4 line:** only modules whose `main` builds against `main+custom-studio-protocol` are usable. `zmk-module-battery-history` and `zmk-module-settings-rpc` are still v0.3-only upstream (their test manifests pin `v0.3-branch+custom-studio-protocol*`) and were dropped; `zmk-module-runtime-input-processor` was dropped because nothing but the vendor overlay used it (the keymap uses stock `zip_*` processors). `zmk-behavior-runtime-sensor-rotate` is v0.3-tested upstream but compiles on v4 and is kept for `rsr_vol`. `mise run deps-check` on a `track: tags` module would suggest the newest tag regardless of ZMK line — check a module's `west/west-dependency/west-test-dependency.yml` before taking a bump.
- **CI `build.yml` uses `zmkfirmware/zmk` `build-user-config.yml` at a pinned ZMK-main SHA** (not `@v0.3.0`): that reusable workflow picks the build container, which has to match Zephyr 4.1. Its default artifact name embeds the board (`nice_nano//zmk` → `nice_nano_zmk`), so `build.yaml` sets `artifact-name: settings_reset` explicitly.
- **Gaming entry is SYS pos 63, not pos 58** — pos 58 (joystick-center) is the mouse left-click since PR #25. Don't reuse it for `&to GAME`.
