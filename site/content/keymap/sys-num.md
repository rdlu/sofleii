---
title: SYS|NUM
description: The Eyelash Sofle SYS|NUM layer (4) - Bluetooth profiles, underglow controls, a mouse fallback, a numpad, and the way into gaming mode.
keywords: [SYS, NUM, numpad, Bluetooth, BT_SEL, BT_CLR, underglow, RGB, mouse keys, gaming]
weight: 6
---

Layer 4, held with `mo(SYS|NUM)` on the left thumb: system controls on the
left hand, a numpad on the right. Release the key to go back to BASE.

{{< layer "SYS|NUM" >}}

## What's on it

- **Left top row**: `BT_CLR`, then profiles 0 to 4 (legends 1 to 5). See
  [Pairing and Bluetooth](../../docs/bluetooth/).
- **Left, underglow**: saturation, hue and effect up on the <kbd>Q</kbd>
  <kbd>W</kbd> <kbd>E</kbd> spots and down on <kbd>A</kbd> <kbd>S</kbd>
  <kbd>D</kbd>, the RGB toggle on the bottom-left key.
- **Keys the underglow module owns** (dotted outline): the bindings are
  `&none`, but the `zmk-rgb-layer` module watches these positions while
  SYS|NUM is the top layer. <kbd>Bri+</kbd> and <kbd>Bri−</kbd> on the
  <kbd>Tab</kbd> and <kbd>Esc</kbd> spots step the master brightness by 8
  (range 4 to 60). <kbd>Dark</kbd>, <kbd>Dim</kbd> and <kbd>FX</kbd> on
  <kbd>Z</kbd> <kbd>X</kbd> <kbd>C</kbd> pick the BASE mood: off, solid dim
  teal, or the next of breathe, spectrum and swirl. Both settings survive a
  reboot. See [Underglow](../../docs/underglow/).
- **Mouse, for when there is no other pointer**: back and forward on
  <kbd>R</kbd> <kbd>T</kbd>; left, right and middle click on <kbd>F</kbd>
  <kbd>G</kbd> <kbd>V</kbd>; the joystick moves the pointer, and its centre
  press is a left click.
- **Right, numpad**, aligned with the 7 8 9 of the number row, with `/ * =`
  down the left edge and `- + Enter %` down the right.
- **Right-thumb outer key**: `&to 5`, which opens the
  [game picker](../gaming/#game-picker).
