---
title: Gaming mode
description: The Eyelash Sofle's locked game layers - entering the GAME? picker from SYS|NUM, the MINECRFT, GAME and NOMANSKY layers, the MYGAME custom slot, push-to-talk, and the exit and re-pick combos.
keywords: [gaming, game mode, Minecraft, No Man's Sky, MYGAME, custom game, FPS, MOBA, WASD, push-to-talk, F13, picker]
weight: 7
---

Home-row mods and hold-taps are good for typing and bad for games: holding
<kbd>S</kbd> to walk back would turn into Alt. The game layers have **no
home-row mods, no hold-tap and no caps word**. Every key is a plain key
press, so movement, run and crouch hold the way a game expects. Every
binding is drawn on [Gaming layers](../../keymap/gaming/). A game layer
stays on until you leave it; nothing needs to be held.

## Entering

1. Hold `mo(SYS|NUM)` (left thumb).
2. Tap the **right-thumb outer key** (position 63, `&to 5`). The **GAME?**
   picker opens.
3. Push the **joystick**:

| Joystick | Goes to | Display | Underglow |
| --- | --- | --- | --- |
| up | **MINECRFT** (layer 6) | `MINECRFT` | green |
| down | **GAME**, generic FPS/MOBA (layer 7) | `GAME` | blue |
| left | **NOMANSKY**, No Man's Sky (layer 8) | `NOMANSKY` | white |
| right | **MYGAME**, your own game (layer 9) | `MYGAME` | yellow |
| centre | back to BASE (cancel) | | |

Every other key on the picker does nothing, so a stray press can't fall
through to BASE. The layer name on the nice!view is the main sign of which
mode you are in; the underglow colour shows on USB and on battery.

## Inside a game layer

All four layers share one layout:

- The alpha block and the right half are plain QWERTY, so chat and consoles
  work as usual. Abilities and items stay at their usual letters and numbers.
- **LCTRL** on the bottom-left pinky key (where BASE has <kbd>\\</kbd>),
  **LSHIFT** on the inner left thumb key.
- **Space on both thumbs**, **Backspace** and **LALT** on the right thumb.
- **F13** on the right thumb, for push-to-talk: bind it in Discord or OBS.
- `mo(MEDIA)` stays on the right-thumb outer key.
- The joystick is an audio cluster (volume up and down, mute, play/pause),
  with Enter on the centre press. The encoder stays on volume.

They differ in the top-left key and, on NOMANSKY, in two modifiers. MYGAME
starts as a copy of GAME:

| Layer | Top-left key | Bottom-left pinky | Inner left thumb |
| --- | --- | --- | --- |
| MINECRFT (6) | `F3` (debug screen) | `LCTRL` | `LSHIFT` |
| GAME (7) | `` ` `` (console) | `LCTRL` | `LSHIFT` |
| NOMANSKY (8) | `F12` (Steam screenshot) | `LSHIFT` | `LCTRL` |
| MYGAME (9) | `` ` `` (console) | `LCTRL` | `LSHIFT` |

No Man's Sky runs on Shift and doesn't use Ctrl, so NOMANSKY swaps the two:
the run modifier moves to the pinky.

## Your own game

**MYGAME** is a slot for a game that isn't built in. It ships as an exact
copy of GAME and is meant to be rebound: edit layer 9 in ZMK Studio (over
USB or BLE) to fit your game, and the change sticks without rebuilding the
firmware. The exit and re-pick combos go by key position, so they keep
working whatever you bind on positions 0, 5 and 12. The name on the display
stays `MYGAME` until you rename the layer, and the yellow underglow is
compiled in.

## Leaving

Two combos work on the game layers only:

| Press together | Does |
| --- | --- |
| the two **top corners** (positions 0 + 12) | back to BASE, underglow off |
| the **top-left key and the 5 key** (positions 0 + 5) | back to the picker, to switch game |

The corners are far apart on purpose: hard to brush in a fight, easy to hit
when you mean it. See [Combos](../../keymap/combos/) for their timeouts, and
[Gaming layers](../../keymap/gaming/) for every binding.
