---
title: Keymap
description: Every layer of the Eyelash Sofle keymap - BASE, NAV, CODE, MEDIA, SYS|NUM and the game layers - with the keymap-drawer render and what each layer is for.
keywords: [keymap, layers, BASE, NAV, CODE, MEDIA, SYS, NUM, numpad, macros, joystick, thumb cluster, keymap-drawer]
weight: 11
---

The drawing below is `keymap-drawer/eyelash_sofle.svg`, which the Draw
Keymap workflow regenerates from `config/eyelash_sofle.keymap` on every
change, and this site rebuilds after it. It is always the current keymap.
The text after it explains what the drawing can't.

{{< keymap-svg >}}

## Layers

| # | Name | How to get there |
| --- | --- | --- |
| 0 | BASE | always on |
| 1 | NAV | hold `mo(NAV)`, left thumb |
| 2 | CODE | hold `mo(CODE)`, right thumb (position 60) |
| 3 | MEDIA | hold `mo(MEDIA)`, right-thumb outer key |
| 4 | SYS\|NUM | hold `mo(SYS|NUM)`, left thumb |
| 5 | GAME? | from SYS\|NUM, tap the right-thumb outer key (position 63): the game picker |
| 6 | MINECRFT | picker, joystick up |
| 7 | GAME | picker, joystick down: generic FPS/MOBA |
| 8 | NOMANSKY | picker, joystick left: No Man's Sky |

Layers 5 to 8 are [Gaming mode](../../gaming-mode/). The layer's name shows
on the nice!view, and the underglow takes its [colour](../../underglow/).

## BASE (0)

QWERTY with home-row mods, and the thumbs doing the rest.

- **Home-row mods**: <kbd>S</kbd> Alt, <kbd>D</kbd> Ctrl, <kbd>F</kbd> Shift
  on the left; <kbd>J</kbd> Shift, <kbd>K</kbd> Ctrl, <kbd>L</kbd> Alt on the
  right. Same finger, same modifier on both hands. Timing and rules:
  [Hold-tap timing](../hold-tap/).
- **Left thumb**: `mo(SYS|NUM)` · `F11` · `LGUI` · `mo(NAV)` · `Backspace`.
- **Right thumb**: `Space` · `mo(CODE)` · `Caps Word` · `F12`, or AltGr when
  held · `mo(MEDIA)`.
- **Joystick**: volume up and down, play/pause, next track, and mute on the
  centre press. Media without holding a layer. Position 52 is a second
  Mute key.
- **Encoder**: volume. Editable live in [Studio](../../studio/).
- **Delete** is `mo(CODE)` + `Backspace`: hold CODE with the right thumb, tap
  Backspace with the left. Two hands, no hold-tap.

AltGr on the thumb, rather than on a letter key, keeps accented letters
(AltGr+E, AltGr+O) reachable with either hand, and fast typing no longer
turns a letter into AltGr.

## NAV (1)

Navigation, function keys and window-manager shortcuts.

- **Top row**: `F12`, then `F1` to `F11`.
- **Right side**: `Home` · `PgDn` · `PgUp` · `End` · `Ins` · `PrtSc` on the
  second row; arrows on <kbd>H</kbd> <kbd>J</kbd> <kbd>K</kbd> <kbd>L</kbd>,
  then `Del` and `Pause` on the home row.
- **Left side**: Super-key combos for workspaces and monitors (focus, move a
  window). The right bottom row has more: workspace 1, next, previous, last,
  and monitor left and right.
- **Joystick**: arrows, with Enter on the centre press.
- **Thumbs**: `Del` on the left; app menu, `LCTRL`, `Left` and `Right` on the
  right.

The exact Super combos, and matching config for niri, GNOME and COSMIC, are
in [WM_KEYBINDS.md](https://github.com/rdlu/zmk-sofle/blob/main/WM_KEYBINDS.md).

## CODE (2)

Brackets on the left hand, and programming digraphs and snippets as one-key
macros.

- **Left home row**: `` ` `` · `{` · `}` · `[` · `]` · `+`. The `}` `[` `]`
  keys are mod-taps, Alt, Ctrl and Shift when held, like the home-row mods
  under them; they are plain `&mt` with ZMK's default timing.
- **Left bottom row**: `-` · `_` · `(` · `)` · `=`.
- **Left upper rows**: Elixir sigils (`~s()`, `~w()`, `~r//`, `~D[]`), `<>`,
  and the arrows `<-` `|>` `->` `=>`.
- **Right side**: `::`, `|_|`, `dbg!()`, `todo!()`, the turbofish `::<>`,
  bracket and quote pairs (`()`, `[]`, `{}`, `""`, with the cursor placed
  inside), comparison operators (`==`, `!=`, `<=`, `>=`), and HEEx `{ }` and
  `<% %>`.
- The left-thumb Backspace position is `Del`.

The pipe operator `|>` is also a combo on BASE; see [Combos](../combos/).

## MEDIA (3)

- **Right home row**: volume down · volume up · play/pause · stop.
- **Right bottom row**: `F20` (mic mute) · mute · previous · next.
- **Joystick**: the same media cluster as BASE.
- `Del` on the left index home key (the <kbd>F</kbd> spot).

## SYS|NUM (4)

Bluetooth, underglow, a mouse fallback, and a numpad.

- **Left top row**: `BT_CLR`, then profiles `BT0` to `BT4`. See
  [Pairing and Bluetooth](../../bluetooth/).
- **Left, underglow**: brightness on the Tab and Esc spots, saturation, hue
  and effect up and down, the mood keys on Z X C, the RGB toggle on the
  bottom-left key. See [Underglow](../../underglow/).
- **Mouse, for when there is no other pointer**: back and forward on
  <kbd>R</kbd> <kbd>T</kbd>; left, right and middle click on <kbd>F</kbd>
  <kbd>G</kbd> <kbd>V</kbd>; the joystick moves the pointer, and its centre
  press is a left click.
- **Right, numpad**, aligned with the 7 8 9 of the number row:

```text
NUM  7   8   9   -
 /   4   5   6   +
 *   1   2   3  Ent
 =   0   ,   .   %
```

- **Right-thumb outer key**: `&to 5`, the game picker.

## Key positions

Combos and hold-tap rules refer to keys by position, 0 to 63, counted left
to right, top to bottom:

```text
Rows 0-3: 13 keys each (6 left, the joystick column, 6 right)   0-51
Row 4:    12 keys (thumbs and the joystick centre)              52-63
```

Some positions worth knowing: 14 <kbd>Q</kbd>, 28 <kbd>S</kbd>, 40
<kbd>Z</kbd> (soft off); 0 and 12, the top corners; 57 Backspace; 60
`mo(CODE)`; 63 the right-thumb outer key. The joystick is 6 (up), 19 (down),
32 (left), 45 (right) and 58 (centre).
