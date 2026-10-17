---
title: Building locally
description: Build the Eyelash Sofle firmware on Arch Linux - the Zephyr SDK 0.17.0, mise run setup, doctor and deps-check, the build and flash tasks, the keymap render and the printable PDF.
keywords: [build, west, Zephyr SDK, mise, setup, doctor, doctor-fix, deps-check, build-left, build-right, nice_nano//zmk, keymap-drawer]
weight: 9
---

You don't need to build anything to use the firmware: every push to `main`
that touches `config/` builds it on GitHub Actions, and
[Flashing](../flashing/) downloads the result. Build locally to try keymap
changes before pushing them, or to work on the firmware itself.

The repo is a [ZMK](https://zmk.dev/) user config with a local
[west](https://docs.zephyrproject.org/latest/develop/west/) workspace, and
[mise](https://mise.jdx.dev/) runs every task. These steps are for Arch Linux.

## 1. System packages

```sh
sudo pacman -S cmake ninja dtc git dfu-util
yay -S python-west mise
```

## 2. Zephyr SDK 0.17.0

The ARM toolchain goes into `tools/`, which git ignores:

```sh
mkdir -p tools
wget -P tools https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v0.17.0/zephyr-sdk-0.17.0_linux-x86_64_minimal.tar.xz
tar xf tools/zephyr-sdk-0.17.0_linux-x86_64_minimal.tar.xz -C tools/
tools/zephyr-sdk-0.17.0/setup.sh -t arm-zephyr-eabi -h -c
rm tools/zephyr-sdk-0.17.0_linux-x86_64_minimal.tar.xz
```

> [!WARNING]
> Don't use the AUR `zephyr-sdk` package. It installs SDK 1.0, which targets
> Zephyr 4.2 and later; this firmware is on Zephyr 4.1, and 0.17.0 is the
> matching SDK. `.mise.toml` points `ZEPHYR_SDK_INSTALL_DIR` at
> `tools/zephyr-sdk-0.17.0` and pins `ZEPHYR_TOOLCHAIN_VARIANT`.

## 3. The workspace (once)

mise provisions Python 3.12 through uv and activates a `.venv` when you `cd`
into the repo:

```sh
mise install
mise run setup
```

`setup` runs `west init`, `west update` and `west zephyr-export`, and
installs the Python dependencies into the venv. ZMK lands in `zmk/`, Zephyr
in `zephyr/`, the other west projects in `modules/` and the repo root; all of
them are git-ignored.

## 4. Check the workspace

```sh
mise run doctor        # fast, offline: is the workspace what config/west.yml says?
mise run doctor-fix    # repair it
mise run doctor-ci     # optional: compare with the last green main build (needs gh)
mise run deps-check    # the other direction: has upstream moved past the pins?
```

A west workspace can drift from `config/west.yml` without `west update`
saying so: west names a project's remote from the manifest but never
repoints an existing clone. `doctor` compares every project's `HEAD` with the
`manifest-rev` ref west keeps in it, checks every remote URL against the
manifest, and checks that the `nice_nano//zmk` board exists and that Zephyr
is 4.1. Run it first whenever a build fails in a way that looks
environmental; see [Troubleshooting](../reference/troubleshooting/#builds).

`deps-check` reads the `# track:` comment beside each pin and reports the
projects whose upstream has moved, with one `git ls-remote` each. See
[Firmware](../reference/firmware/#dependencies) for the pins and how to take
an update.

## 5. Build

```sh
mise run build-left    # left half: Studio, the custom RPC modules, zmk-rgb-layer
mise run build-right   # right half
mise run build-reset   # settings_reset.uf2
```

They write `eyelash_sofle_left.uf2`, `eyelash_sofle_right.uf2` and
`settings_reset.uf2` in the repo root. Under the hood, each is one
`west build -s zmk/app -b nice_nano//zmk` with the shield
(`eyelash_sofle_left nice_view` or `eyelash_sofle_right nice_view`), the
`config/` directory and the in-repo `zmk-rgb-layer` module; the left half
adds Studio and the `studio-rpc-usb-uart` snippet for USB.

## 6. Flash

```sh
mise run flash-left    # or flash-right, flash-reset
mise run build-flash   # build both halves, flash both, print the memory budget
```

Each `flash-*` task asks you to double-tap reset on the half, waits up to 60
seconds for `NICENANO` to mount, copies the file and syncs.

## The keymap render and PDF

After editing `config/eyelash_sofle.keymap`, regenerate the drawing:

```sh
mise run draw
```

It renders `keymap-drawer/eyelash_sofle.svg` with
[keymap-drawer](https://github.com/caksoylar/keymap-drawer), using the same
version and physical layout as CI, so the result matches what the Draw Keymap
workflow commits on `main`. That SVG is what the
[Keymap](../../keymap/) page on this site shows.

For a printable A4 PDF of the layers:

```sh
mise run draw-pdf         # from the local SVG
mise run draw-pdf-fresh   # run draw first, then build the PDF
mise run draw-pdf-ci      # from the parsed keymap on origin/main
mise run open-pdf         # build from the local SVG and open it
```

The PDF goes to `keymap-drawer/eyelash_sofle.pdf` (git-ignored).

## This site

The docs site lives in `site/` and builds with Hugo:

```sh
mise run docs         # dev server on http://127.0.0.1:1313/
mise run docs-build   # minified build into site/public/
```

The keymap render, the hold-tap and combo tables and the dependency pins on
this site are read from the repo's own files on every build, so they never
need updating by hand.
