---
title: Keymap printout
description: The printable Eyelash Sofle keymap - a one-page basics manual and every layer's grid, laid out for A4 portrait. Download it as /sofleii-keymap.pdf.
url: /keymap/print/
# A standalone page (layouts/keymap-print.html): no navbar, sidebar, pager or
# footer, always the light palette. CI prints it to /sofleii-keymap.pdf with
# headless Chromium (.github/scripts/print-keymap-pdf.sh); locally,
# `mise run docs-pdf`. It lives outside content/keymap/ so it never shows in
# that section's pager.
layout: keymap-print
sidebar:
  exclude: true
excludeSearch: true
noindex: true
sitemap:
  disable: true
# Pages after the basics: each list is one A4 page of layer grids (up to
# three fit a portrait page), by display-name. The first one also carries the
# key-style legend. The four game layers go two to a page.
sheets:
  - layers: ["BASE", "NAV", "CODE"]
  - layers: ["MEDIA", "SYS|NUM", "GAME?"]
  - layers: ["MINECRFT", "GAME"]
  - layers: ["NOMANSKY", "MYGAME"]
captions:
  BASE: "Always on. Hold-tap keys: tap for the big legend, hold for the small one under it."
  NAV: "Hold the left thumb's NAV key. Arrows on HJKL, function keys on the top row, window-manager shortcuts on the left."
  CODE: "Hold the right thumb's CODE key. Brackets, digraphs and code snippets as one-key macros; ▏ is where the cursor ends up."
  MEDIA: "Hold the right thumb's outer key (MEDIA). Volume, playback, brightness, Delete, touchpad (F21), spare F14–F19/F24 for host binds."
  "SYS|NUM": "Hold the left thumb's outer key. Bluetooth, underglow and the mouse on the left, a numpad on the right; the outer right-thumb key opens GAME?."
  "GAME?": "The game picker: push the joystick to choose, press its centre to cancel. Every other key does nothing."
  MINECRFT: "F3 top-left. No hold-taps: plain keys, Ctrl on the pinky, Shift on the left thumb, F13 push-to-talk."
  GAME: "Generic FPS/MOBA: the same chassis as MINECRFT, with ` (console) top-left."
  NOMANSKY: "No Man's Sky: Shift on the pinky and Ctrl on the thumb (swapped), F12 (Steam screenshot) top-left."
  MYGAME: "Your own game: starts as a copy of GAME, rebind it in Studio. Yellow underglow."
---

<div class="cols">

## Layers

| Layer | How to get there |
| --- | --- |
| BASE | always on |
| NAV | hold the left thumb's 4th key (next to Backspace) |
| CODE | hold the right thumb's 2nd key (next to Space) |
| MEDIA | hold the right thumb's outer key |
| SYS\|NUM | hold the left thumb's outer key |
| GAME? | on SYS\|NUM, tap the right thumb's outer key |

Release the key and you're back on BASE. Thumbs, outside in: left
SYS\|NUM · F11 · GUI · NAV · Backspace; right MEDIA · F12/AltGr · Caps Word
· CODE · Space.

## Home-row mods

<kbd>S</kbd> Alt · <kbd>D</kbd> Ctrl · <kbd>F</kbd> Shift and
<kbd>J</kbd> Shift · <kbd>K</kbd> Ctrl · <kbd>L</kbd> Alt: tap for the
letter, hold for the mod. Pause briefly before you hold one (230 ms, 70 ms
on F and J) and press the other key with the other hand; in a fast typing
burst they stay letters.

## Thumb tricks

- **Delete** = hold CODE (right thumb) + tap Backspace (left thumb).
- **Caps Word**: the right thumb's middle key types the next word in
  capitals, through `_` `-` and digits, until a space.
- **AltGr**: hold the right thumb's F12 key; tap it for F12.

## Power

- **Soft off**: hold <kbd>Q</kbd> + <kbd>S</kbd> + <kbd>Z</kbd> together for
  2 s. **Wake**: press the reset button once.

## Bluetooth (on SYS\|NUM)

The <kbd>1</kbd>–<kbd>5</kbd> spots select host profiles 1–5; the
<kbd>&#96;</kbd> spot (top-left) is `BT_CLR`, which forgets the selected
profile's host. Pair again after clearing.

## Underglow (on SYS\|NUM)

Each layer has its own colour. BASE's look: <kbd>Z</kbd> spot dark,
<kbd>X</kbd> dim teal, <kbd>C</kbd> effects (each press: breathe, spectrum,
swirl). Brightness: <kbd>Tab</kbd> spot up, <kbd>Esc</kbd> spot down.
Remembered across reboots.

## Gaming mode

Hold SYS\|NUM, tap the right thumb's outer key: the **GAME?** picker.
Joystick ↑ MINECRFT · ↓ GAME · ← NOMANSKY · → MYGAME (yours to rebind) ·
centre cancels. Game layers have
no hold-taps and stay on until you leave: <kbd>&#96;</kbd> + <kbd>-</kbd> (top
corners) back to BASE, <kbd>&#96;</kbd> + <kbd>5</kbd> back to the picker.

## Studio

Edit the keymap live: connect the **left** half over USB or Bluetooth at
**studio.dya.cormoran.works** (Chrome or Edge). Saved edits survive a
firmware flash; a settings reset clears them.

</div>

<div class="combos">

## Combos

{{< keymap-combos >}}

</div>
