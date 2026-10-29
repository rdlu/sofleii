---
title: NAV
description: The Eyelash Sofle NAV layer (1) - arrows on HJKL, paging, F1-F12, and Super-key window-manager shortcuts.
keywords: [NAV, navigation, arrows, HJKL, function keys, window manager, niri, workspaces]
weight: 3
---

Layer 1, held with `mo(NAV)` on the left thumb: navigation, function keys
and window-manager shortcuts. Release the key to go back to BASE.

{{< layer "NAV" >}}

## What's on it

- **Top row**: `F12`, then `F1` to `F11`.
- **Right side**: `Home` · `PgDn` · `PgUp` · `End` · `Ins` · `PrtSc` on the
  second row; arrows on <kbd>H</kbd> <kbd>J</kbd> <kbd>K</kbd> <kbd>L</kbd>,
  then `Del` and `Pause` on the home row.
- **Left side**: Super-key combos for workspaces and monitors (focus one, or
  move a window to it). The right bottom row has more: workspace 1, next,
  previous, last, and monitor left and right.
- **Screenshots**: a column on the inner edge, one tap each: `Ctrl+PrtSc`
  on <kbd>T</kbd>, `Alt+PrtSc` on <kbd>G</kbd>, `Shift+PrtSc` on <kbd>B</kbd>.
- **Home-row mods stay live**: <kbd>S</kbd> and <kbd>D</kbd> are hold-taps
  on NAV too. Tapped, they send the monitor-up/right shortcuts; held, they are
  `Alt` and `Ctrl` (the same `hml` behaviour as on BASE), and <kbd>F</kbd>
  passes through as `Shift`. So NAV + a held mod + a right-hand NAV key works
  (Alt+PrtSc, Ctrl+Home, Shift+arrows…). Because they're hold-taps, the
  shortcut fires on release, and a mod only holds against right-hand keys
  (or S/D/F), so Alt + the screenshot column comes out as the shortcut.
- **Joystick**: arrows, with Enter on the centre press.
- **Thumbs**: `Del` on the left; app menu, `Ctrl`, `Left` and `Right` on the
  right, so the right thumb can hold Ctrl and step by word.

The exact Super combos, and matching config for niri, GNOME and COSMIC, are
in [WM_KEYBINDS.md](https://github.com/rdlu/sofleii/blob/main/WM_KEYBINDS.md).
