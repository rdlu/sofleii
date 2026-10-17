---
title: Hold-tap timing
description: The Eyelash Sofle's home-row mods and AltGr mod-tap - the hold-tap behaviors, their timing read from the keymap, the index-finger exception, positional rules and tuning tips.
keywords: [home row mods, HRM, hold-tap, tapping-term, require-prior-idle, quick-tap, balanced, hold-preferred, hold-trigger-key-positions, AltGr, ralt_mt]
weight: 12
---

A hold-tap key sends one thing when tapped and another when held: on BASE,
<kbd>F</kbd> is F when tapped and Shift when held. The hard part is telling
the two apart while typing fast, and the settings below are how this keymap
does it.

## The behaviors

This table is read from `config/eyelash_sofle.keymap` every time the site
builds, so it always matches the firmware:

{{< keymap-behaviors >}}

What the columns mean:

- **tapping-term**: hold longer than this and it's a hold.
- **require-prior-idle**: if another key was pressed less than this long
  before, the key is always a tap. This is what keeps fast typing from
  firing modifiers.
- **quick-tap**: tap the key again within this window after a tap and it
  is a tap again, so holding it repeats the letter.
- **flavor**: `balanced` makes it a hold when another key is pressed *and
  released* inside the tapping term; `hold-preferred` makes it a hold as
  soon as another key is pressed.
- **Opposite hand only** (`hold-trigger-key-positions`): it can only become
  a hold when the other key is on the other hand. Same-hand rolls stay
  letters.
- **Hold on release** (`hold-trigger-on-release`): the positional check
  happens when the other key is released, not pressed, which allows holding
  two mods on the same hand and then pressing a key on the other.

## Finger mapping

Ring finger Alt, middle Ctrl, index Shift, on both hands: <kbd>S</kbd>
<kbd>D</kbd> <kbd>F</kbd> and <kbd>L</kbd> <kbd>K</kbd> <kbd>J</kbd>. The
mirror image is easy to remember, and keeps Alt off the inner index
positions (<kbd>G</kbd>, <kbd>H</kbd>), where it was being mistyped.

## The index-finger exception

`hml_idx` and `hmr_idx` (<kbd>F</kbd> and <kbd>J</kbd>, the Shifts) are
tuned apart from the others. The index finger rolls and holds less evenly,
so a long idle requirement made Shift on F and J feel sluggish for CamelCase
and quick rolls. They get a much shorter `require-prior-idle` and the
`hold-preferred` flavor, which commits the Shift on the next key's press
instead of its release. The opposite-hand rule still blocks same-hand rolls,
and the short idle window still blocks most misfires mid-word.

The idea comes from [Sunaku's home-row mods guide](https://sunaku.github.io/home-row-mods.html).

## AltGr on the thumb

`ralt_mt` is the right-thumb `F12` key: F12 when tapped, Right Alt (AltGr)
when held. It has **no** opposite-hand rule, because AltGr accents
(AltGr+E is é, AltGr+O is ó) are typed on both hands, and a thumb is
comfortable to hold while the other fingers reach either side.

## Tuning

| Problem | Try |
| --- | --- |
| Modifiers fire while typing | raise `require-prior-idle-ms` |
| A modifier doesn't fire when you mean it | lower `tapping-term-ms` (for example 250) |
| Key repeat doesn't work | raise `quick-tap-ms` to your fastest double tap |
| Deliberate holds feel slow | try `flavor = "tap-unless-interrupted"` |

The behaviors are in the `behaviors {}` block of
[`config/eyelash_sofle.keymap`](https://github.com/rdlu/zmk-sofle/blob/main/config/eyelash_sofle.keymap).
After a change, run `mise run draw` and commit the keymap with the new
render; this page picks up the new values on its next build.
