---
title: Gaming layers
description: The Eyelash Sofle gaming layers (5-9) - the GAME? picker, the MINECRFT, GAME and NOMANSKY layers and the MYGAME custom slot, with no home-row mods or hold-taps.
keywords: [gaming, GAME?, picker, Minecraft, MINECRFT, No Man's Sky, NOMANSKY, MYGAME, custom game slot, FPS, MOBA, WASD]
weight: 7
---

Layers 5 to 9 are locked: you switch to them and they stay until you leave.
None of them has home-row mods, hold-taps or Caps Word, so every key is a
plain press and holds work the way games expect. How to use them is in the
[Gaming mode](../../docs/gaming-mode/) guide; this page is the bindings.

## GAME? picker

Layer 5. From [SYS|NUM](../sys-num/), tap the right-thumb outer key
(position 63). Push the joystick up for MINECRFT, down for GAME, left for
NOMANSKY, right for MYGAME; the centre press cancels back to BASE. Every other key does
nothing, so a stray press can't fall through.

{{< layer "GAME?" >}}

## MINECRFT

Layer 6. Plain QWERTY, `F3` (debug screen) top-left, `Ctrl` on the
bottom-left pinky key, `Shift` on the inner left thumb, `Space` on both
thumbs, `F13` for push-to-talk, and the joystick as an audio cluster with
Enter in the centre.

{{< layer "MINECRFT" >}}

## GAME

Layer 7, for FPS and MOBA games: the same layout as MINECRFT, with `` ` ``
(the console key) top-left.

{{< layer "GAME" >}}

## NOMANSKY

Layer 8, for No Man's Sky: the same layout with `F12` (Steam screenshot)
top-left, and `Shift` and `Ctrl` swapped, since the game runs on Shift and
doesn't use Ctrl.

{{< layer "NOMANSKY" >}}

## MYGAME

Layer 9, a custom slot for your own game. It starts as a copy of GAME, key
for key (`` ` `` top-left, the same chassis), and is there to be rebound:
change its keys live in ZMK Studio, no firmware rebuild needed, and leave the
other game layers alone. Its underglow is yellow.

{{< layer "MYGAME" >}}

## Leaving

On layers 6 to 9, press the two top corners together (positions 0 + 12) to
go back to BASE with the underglow off, or the top-left key and the 5 key
(0 + 5) to reopen the picker. See [Combos](../combos/).
