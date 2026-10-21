---
title: Keymap
description: The Eyelash Sofle keymap - every layer, how to reach it, the keymap-drawer render, and a page per layer with its bindings generated from the firmware.
keywords: [keymap, layers, BASE, NAV, CODE, MEDIA, SYS, NUM, gaming, keymap-drawer, key positions]
aliases: [/docs/reference/keymap/]
# The Docs sidebar layout for this section too; Hextra gives the sidebar only
# to `type: docs` pages. Each section's sidebar lists only its own pages
# (the sidebar's root is the page's first section).
cascade:
  type: docs
# Listed first in this section's sidebar (patched layouts/_partials/sidebar.html).
sidebar:
  overview: Overview
---

The keymap is the heart of the firmware. This drawing is
`keymap-drawer/eyelash_sofle.svg`, which the Draw Keymap workflow regenerates
from `config/eyelash_sofle.keymap` on every change; the site rebuilds after
it. The layer pages below draw each layer again on its own, from the same
keymap, and explain what the drawing can't.

<div class="zs-actions">
{{< hextra/hero-button text="Download PDF" link="/sofleii-keymap.pdf" >}}
{{< hextra/hero-button text="Print view" link="/keymap/print/" >}}
</div>

The PDF is four A4 portrait pages: a one-page summary of the basics, then
every layer's grid, three to a page. It is printed from the [print view](print/) on every site
build, so it matches the keymap the site shows.

{{< keymap-svg >}}

## Layers

| # | Name | How to get there | Back |
| --- | --- | --- | --- |
| 0 | [BASE](base/) | always on | |
| 1 | [NAV](nav/) | hold `mo(NAV)`, left thumb | release |
| 2 | [CODE](code/) | hold `mo(CODE)`, right thumb (position 60) | release |
| 3 | [MEDIA](media/) | hold `mo(MEDIA)`, right-thumb outer key (position 63) | release |
| 4 | [SYS\|NUM](sys-num/) | hold `mo(SYS|NUM)`, left thumb | release |
| 5 | [GAME?](gaming/#game-picker) | from SYS\|NUM, tap the right-thumb outer key (position 63) | joystick centre |
| 6 | [MINECRFT](gaming/#minecrft) | picker, joystick up | top corners (0 + 12) |
| 7 | [GAME](gaming/#game) | picker, joystick down: generic FPS/MOBA | top corners (0 + 12) |
| 8 | [NOMANSKY](gaming/#nomansky) | picker, joystick left: No Man's Sky | top corners (0 + 12) |
| 9 | [MYGAME](gaming/#mygame) | picker, joystick right: your own game | top corners (0 + 12) |

Layers 1 to 4 are momentary: they last while you hold their key. Layers 5
to 9 are locked: they stay until you leave them ([Gaming mode](../docs/gaming-mode/)).
The layer's name shows on the nice!view, and the underglow takes its
[colour](../docs/underglow/).

{{< cards >}}
  {{< card link="/keymap/base/" title="BASE" subtitle="QWERTY, home-row mods, the thumb clusters." icon="view-grid" >}}
  {{< card link="/keymap/nav/" title="NAV" subtitle="Arrows, paging, F-keys, window-manager shortcuts." icon="arrows-expand" >}}
  {{< card link="/keymap/code/" title="CODE" subtitle="Brackets and one-key programming digraphs." icon="code" >}}
  {{< card link="/keymap/media/" title="MEDIA" subtitle="Volume, playback, mic mute." icon="volume-up" >}}
  {{< card link="/keymap/sys-num/" title="SYS|NUM" subtitle="Bluetooth, underglow, mouse fallback, numpad." icon="cog" >}}
  {{< card link="/keymap/gaming/" title="Gaming layers" subtitle="The picker, MINECRFT, GAME, NOMANSKY and MYGAME." icon="puzzle" >}}
  {{< card link="/keymap/hold-tap/" title="Home-row mods & timing" subtitle="The hold-tap behaviors, read from the keymap." icon="clock" >}}
  {{< card link="/keymap/combos/" title="Combos" subtitle="Soft off, the pipe operator, game exits." icon="key" >}}
{{< /cards >}}

## Key positions

Combos and hold-tap rules refer to keys by position, 0 to 63, counted left
to right, top to bottom (hover a key in a layer grid to see its number):

```text
Rows 0-3: 13 keys each (6 left, the joystick column, 6 right)   0-51
Row 4:    12 keys (thumbs and the joystick centre)              52-63
```

Some positions worth knowing: 14 <kbd>Q</kbd>, 28 <kbd>S</kbd>, 40
<kbd>Z</kbd> (soft off); 0 and 12, the top corners; 57 Backspace; 60
`mo(CODE)`; 63 the right-thumb outer key. The joystick is 6 (up), 19 (down),
32 (left), 45 (right) and 58 (centre).
