---
title: sofleii
description: sofleii, the Sofle Interactive Interface - ZMK firmware for the Eyelash Sofle split keyboard (home-row mods, nine layers, gaming mode, per-layer underglow, Studio over USB and Bluetooth) and the sofleii CLI/TUI to manage it.
layout: hextra-home
# The headline, typed round and round (layouts/_shortcodes/hero-typed.html):
# each line held for `hold` ms. `tagline` is what screen readers, reduced
# motion and no-JS get, and where a return visit picks up.
hero:
  - { text: "sofleii = Sofle Interactive Interface", hold: 4000 }
  - { text: "Firmware and tools for the Eyelash Sofle", hold: 5000, tagline: true }
  - { text: "↓↘→ + P? Nah. Q + S + Z.", hold: 3000 }
  - { text: "Select your stage: Minecraft, No Man's Sky, or the arena.", hold: 3500 }
---

{{< hextra/hero-badge link="https://github.com/rdlu/sofleii/releases/latest" >}}
  <div class="hx:w-2 hx:h-2 hx:bg-primary-400"></div>
  <span>{{< param "firmware.version" >}} · {{< param "firmware.stack" >}}</span>
  {{< icon name="arrow-circle-right" attributes="height=14" >}}
{{< /hextra/hero-badge >}}

<div class="zs-hero">
<div class="hx:mt-6 hx:mb-6">
{{< hero-typed >}}
</div>

<div class="hx:mb-10">
{{< hextra/hero-subtitle >}}
  sofleii, the Sofle Interactive Interface, is the ZMK firmware for one
  Eyelash Sofle split keyboard (nice!nano v2, nice!view), with home-row mods,
  nine layers, a gaming mode, per-layer underglow and live keymap editing in
  Studio, plus a CLI/TUI on the way to manage it from a terminal.
{{< /hextra/hero-subtitle >}}
</div>
</div>

<div class="zs-actions">
{{< hextra/hero-button text="The keymap" link="/keymap/" >}}
{{< hextra/hero-button text="Keymap PDF" link="/sofleii-keymap.pdf" >}}
{{< hextra/hero-button text="Get started" link="/docs/getting-started/" >}}
{{< hextra/hero-button text="GitHub" link="https://github.com/rdlu/sofleii" >}}
</div>

<div class="hx:mt-12"></div>

{{< hextra/hero-section >}}Move list{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Home-row mods"
    icon="hand"
    subtitle="Alt, Ctrl and Shift under S D F and J K L. A mod fires only when the other hand joins in, and only after a pause in typing, so fast rolls stay letters."
    link="keymap/hold-tap/"
  >}}
  {{< hextra/feature-card
    title="Nine layers"
    icon="collection"
    subtitle="BASE, NAV, CODE, MEDIA and SYS|NUM, plus a game picker, three game layers and a custom game slot. CODE types programming digraphs (`|>`, `->`, `::<>`) in one key."
    link="keymap/"
  >}}
  {{< hextra/feature-card
    title="Bonus stage: gaming mode"
    icon="puzzle"
    subtitle="Locked layers with no hold-tap at all: plain WASD, held modifiers that just hold, push-to-talk on the thumb. Minecraft, generic FPS/MOBA and No Man's Sky."
    link="docs/gaming-mode/"
  >}}
  {{< hextra/feature-card
    title="Per-layer underglow"
    icon="light-bulb"
    subtitle="An in-repo module paints the underglow in the active layer's colour on both halves. The resting look and brightness are yours, and survive a reboot."
    link="docs/underglow/"
  >}}
  {{< hextra/feature-card
    title="Studio over USB and BLE"
    icon="adjustments"
    subtitle="Edit the keymap live from the browser, wired or over Bluetooth, no rebuild. The encoder binding is editable there too. No unlock key."
    link="docs/studio/"
  >}}
  {{< hextra/feature-card
    title="Soft off: Q + S + Z"
    icon="moon"
    subtitle="No ↓↘→ motion required: hold Q, S and Z together for two seconds and the board powers down. One press of reset wakes it."
    link="keymap/combos/"
  >}}
{{< /hextra/feature-grid >}}

<div class="hx:mt-16"></div>

{{< hextra/hero-section >}}Secret character: the sofleii CLI/TUI{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

A terminal dashboard plus a **scriptable CLI**: every reading is a subcommand with `--json`, stable exit codes and a waybar module. Build it from the repo today; the first release is [coming (#83)](https://github.com/rdlu/sofleii/issues/83), and the roadmap lives in [#44](https://github.com/rdlu/sofleii/issues/44).

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Status at a glance"
    icon="status-online"
    subtitle="A live TUI and sofleii status: the split link, the host connection, both batteries and the firmware version, over the same Studio RPC the web app uses."
    link="docs/sofleii/"
  >}}
  {{< hextra/feature-card
    title="BLE profiles"
    icon="wifi"
    subtitle="List, switch, rename and unpair host profiles, and choose USB or BLE first, from the command line, without a browser."
    link="docs/sofleii/"
  >}}
  {{< hextra/feature-card
    title="Scripts and status bars"
    icon="terminal"
    subtitle="JSON with a versioned schema, exit codes for checks, a waybar module and an update check against the releases. Coming later: flashing and the keymap from the tool."
    link="docs/sofleii/"
  >}}
{{< /hextra/feature-grid >}}

<div class="hx:mt-16"></div>

{{< hextra/hero-section >}}Press Start{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Flashing"
    icon="download"
    link="docs/flashing/"
    subtitle="One mise task, or drag-and-drop .uf2 files onto each half."
  >}}
  {{< hextra/feature-card
    title="Upgrading to v3"
    icon="arrow-up"
    link="docs/upgrading-to-v3/"
    subtitle="v3.0.0 needs a settings reset on both halves and a fresh pairing."
  >}}
  {{< hextra/feature-card
    title="Troubleshooting"
    icon="support"
    link="docs/reference/troubleshooting/"
    subtitle="Old keymap after a flash, halves that won't pair, Invalid BOARD."
  >}}
{{< /hextra/feature-grid >}}
