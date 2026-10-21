---
title: Underglow
description: Per-layer underglow colours on the Eyelash Sofle from the in-repo zmk-rgb-layer module - layer colours, the switchable BASE moods, master brightness, persistence and the stock RGB keys.
keywords: [underglow, RGB, layer colour, zmk-rgb-layer, mood, brightness, rgb_ug]
weight: 8
---

The underglow shows which layer is active. This is done by
**`zmk-rgb-layer`**, a small Zephyr module that lives in this repo
(`zmk-rgb-layer/`). It listens for layer changes on the left half and paints
the colour of the highest active layer through the global `&rgb_ug`
behavior, which ZMK forwards to both halves.

Each half has seven LEDs, one per column plus the thumb, so the colour
lights zones, not single keys.

## Layer colours

| Layer | Colour | Hue, saturation |
| --- | --- | --- |
| NAV | indigo | 250, 100 |
| CODE | magenta | 290, 100 |
| MEDIA | amber | 35, 100 |
| SYS\|NUM | red | 0, 100 |
| GAME? | violet | 280, 100 |
| MINECRFT | green | 120, 100 |
| GAME | blue | 220, 100 |
| NOMANSKY | white | 0, 0 |
| MYGAME | yellow | 60, 100 |

The brightness of every colour is the master brightness (below). Momentary
layers light up while their key is held and go back to the BASE look on
release.

## BASE moods

The keys below are drawn on the [SYS|NUM](../../keymap/sys-num/) layer page.
How BASE looks at rest is up to you. Hold `mo(SYS|NUM)` and press one of the
three mood keys on its bottom-left row; the change shows straight away:

| SYS\|NUM key (BASE key in that spot) | Position | Mood |
| --- | --- | --- |
| <kbd>Z</kbd> | 40 | **dark**: underglow off |
| <kbd>X</kbd> | 41 | **dim**: solid dim teal (the default) |
| <kbd>C</kbd> | 42 | **effects**: each press steps through breathe, spectrum and swirl |

## Brightness

A master brightness applies to every colour, so it sticks when the module
repaints. On SYS|NUM, the key in the <kbd>Tab</kbd> spot (position 13) turns
it up and the key in the <kbd>Esc</kbd> spot (position 26) turns it down. The
dim teal BASE sits at about 30% of it, so it stays a glow.

## Persistence

The mood and the brightness are saved to the left half's settings (a few
seconds after the last change) and restored on boot, so a power cycle or a
soft off brings back the look you last chose.

## The stock RGB keys

SYS|NUM also has ZMK's own `&rgb_ug` keys: saturation, hue and effect up on
the <kbd>Q</kbd> <kbd>W</kbd> <kbd>E</kbd> spots, down on the <kbd>A</kbd>
<kbd>S</kbd> <kbd>D</kbd> spots, and the RGB toggle on the bottom-left key
(the <kbd>\\</kbd> spot). The module repaints the layer colour on every
layer change, so for a look that lasts, use the moods and the brightness
keys.

## Notes

- The underglow works the same on USB and on battery
  (`CONFIG_ZMK_RGB_UNDERGLOW_AUTO_OFF_USB=n`). It turns off when the board
  goes idle.
- To keep traffic between the halves low, the module remembers what it last
  sent and only sends what changed: one `&rgb_ug` call per layer change in
  the steady state.
- A reactive effect, lighting the column of the key you press, would need a
  module that drives the LED strip directly. It is deferred to
  [issue #35](https://github.com/rdlu/sofleii/issues/35).
