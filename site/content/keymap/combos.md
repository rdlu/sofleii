---
title: Combos
description: The Eyelash Sofle combos - soft off on Q+S+Z, the |> pipe operator, and the game-layer exit and re-pick - read from the keymap.
keywords: [combos, soft off, Q+S+Z, power off, pipe operator, game exit, key positions]
weight: 13
---

A combo is a set of keys pressed together that does something none of them
does alone. This table is read from `config/eyelash_sofle.keymap` on every
site build. The keys are named by what BASE has in those positions; see
[Key positions](../keymap/#key-positions).

{{< keymap-combos >}}

## Soft off

<kbd>Q</kbd> + <kbd>S</kbd> + <kbd>Z</kbd>, held, powers the keyboard
off: no motion input, no super meter, just three keys and two seconds. It
works on every layer. To wake the board, press the **reset** button once.

Separately from soft off, the board goes to sleep by itself after an hour
without a key press.

## The pipe operator

<kbd>\\</kbd> + <kbd>.</kbd> on BASE types `|>`, Elixir's pipe operator,
without reaching for the CODE layer. The short timeout keeps it from firing
during normal typing. CODE also has `|>` as a single key.

## Game exits

On the game layers only:

- the two top corners (positions 0 + 12) run `GAME_EXIT`: underglow off,
  back to BASE;
- the top-left key and the 5 key (positions 0 + 5) reopen the GAME? picker,
  to switch games without going through BASE.

See [Gaming mode](../../gaming-mode/).
