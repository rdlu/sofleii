---
title: About
description: Who maintains sofleii, where the Eyelash Sofle firmware comes from, and what the name means.
weight: 21
---

## The name

**sofleii** is the **Sofle Interactive Interface**: this keyboard's
firmware and, soon, the tool that manages it. Say "sofle II" out loud and it
sounds a lot like a certain 1991 fighting game. The II is not a coincidence;
it is a pun, not an affiliation.

## The project

[rdlu/sofleii](https://github.com/rdlu/sofleii) is
[Rodrigo Dlugokenski](https://github.com/rdlu)'s ZMK configuration for their
Eyelash Sofle. It started from the vendor's ZMK config for the board and has
since grown its own keymap, home-row mod tuning, gaming mode, underglow
module and tooling. It is a personal configuration: changes follow one
person's hands and games, and releases are cut when something is worth
keeping.

Credits:

- the Eyelash Sofle hardware and its original firmware, by the vendor
  ([original README](https://github.com/rdlu/sofleii/blob/main/README_JP.md));
- [ZMK](https://zmk.dev/) and [Zephyr](https://www.zephyrproject.org/);
- [cormoran](https://github.com/cormoran)'s ZMK fork, DYA Studio and
  custom-RPC modules;
- [keymap-drawer](https://github.com/caksoylar/keymap-drawer) for the
  keymap render;
- [Sunaku's home-row mods guide](https://sunaku.github.io/home-row-mods.html)
  for the timing ideas.

## The sofleii tool

A CLI and TUI in Rust with ratatui, planned to live in this repo and talk to
the left half over the Studio RPC protocol. The v3.0.0 move to ZMK main kept
that RPC plumbing in place as its groundwork. Nothing is released yet; see
[the sofleii tool](../sofleii/).

## This site

[sofleii.dlu.one](https://sofleii.dlu.one/) is built with
[Hugo](https://gohugo.io/) and the [Hextra](https://imfing.github.io/hextra/)
theme, from the `site/` directory of the repo, and served by GitHub Pages.
The keymap render, the hold-tap and combo tables and the dependency pins are
read from the firmware's own files on every build.
