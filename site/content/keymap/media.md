---
title: MEDIA
description: The Eyelash Sofle MEDIA layer (3) - volume, playback, stop and mic mute on the right hand; brightness, touchpad toggle and spare F-keys on the left.
keywords: [MEDIA, volume, playback, mute, mic mute, F20, brightness, touchpad, F21, F13-F24, XF86Launch]
weight: 5
---

Layer 3, held with `mo(MEDIA)` on the right-thumb outer key (position 63).
Release it to go back to BASE.

{{< layer "MEDIA" >}}

## What's on it

- **Right home row**: volume down · volume up · play/pause · stop.
- **Right bottom row**: `F20` (mic mute; bind it on the host) · mute ·
  previous · next.
- **Joystick**: the same media cluster as BASE.
- **Delete** on the left index home key (the <kbd>F</kbd> spot).
- **Screen brightness**, left hand: up and down on <kbd>E</kbd> and
  <kbd>D</kbd> (`XF86MonBrightnessUp/Down`, the laptop panel), and the same
  with Super on <kbd>W</kbd> and <kbd>S</kbd>, which a desktop can bind to
  external monitors (DDC/CI).
- **Touchpad toggle** on <kbd>B</kbd>: `F21`, which the standard Linux
  keyboard tables turn into `XF86TouchpadToggle`. GNOME acts on it by
  itself; other desktops need a bind.
- **Spare F-keys for host binds**: `F14`–`F19` across the left number row
  and `F24` on <kbd>T</kbd>. No keyboard has them and no app expects
  them, so whatever you bind them to on the host is all they do.

BASE already has the media basics on the joystick, so this layer is for the
rest: stop, previous, mic mute, screen brightness, the touchpad and the
spare F-keys. `mo(MEDIA)` stays reachable on the game layers too.

## The upper F-keys on Linux

A keyboard can't send "toggle the touchpad" or "mute the mic"; it sends a
key code, and the standard Linux keyboard tables (xkb, `symbols/inet`) give
the upper F-keys their meanings. Laptop keyboards send these same codes for
their own special keys:

| Key | Arrives as | Used here |
|-----|------------|-----------|
| `F13` | `XF86Tools` | push-to-talk on the [game layers](../gaming/) (games read the raw key) |
| `F14`–`F18` | `XF86Launch5`–`XF86Launch9` | spare, MEDIA number row |
| `F19` | `F19` | spare, MEDIA number row |
| `F20` | `XF86AudioMicMute` | mic mute |
| `F21` | `XF86TouchpadToggle` | touchpad toggle |
| `F22` / `F23` | `XF86TouchpadOn` / `XF86TouchpadOff` (Shift+Super+`F23` = `XF86Assistant`) | not used |
| `F24` | `F24` (Ctrl+Super+`F24` = `XF86TouchpadToggle`) | spare, MEDIA <kbd>T</kbd> |

So `F22`/`F23` aren't free for your own binds, and a desktop-wide bind on
`F13` would take push-to-talk away from games.
