---
title: Docs
description: sofleii (Sofle Interactive Interface) - the Eyelash Sofle ZMK firmware docs (flashing, pairing, Studio, gaming mode, underglow, building, the keymap reference) and the planned sofleii CLI/TUI.
---

sofleii, the Sofle Interactive Interface, is everything in
[rdlu/sofleii](https://github.com/rdlu/sofleii): the ZMK firmware for an
Eyelash Sofle split keyboard, and a planned CLI/TUI to manage it. These pages
cover getting firmware onto the halves, pairing them, editing the keymap
live, what every layer does, and what the [sofleii tool](sofleii/) will do.

> [!WARNING]
> This is a personal configuration, built for one keyboard and tuned to one
> person's hands. Layers, keys and timing change without notice. If you have
> the same hardware, use it as a starting point, not as a stable firmware.

{{< cards >}}
  {{< card link="/keymap/" title="The keymap" subtitle="Every layer, generated from the firmware: its own section." icon="view-grid" >}}
  {{< card link="/docs/getting-started/" title="Getting started" subtitle="The board, the halves, and what the firmware does." icon="arrow-right" >}}
  {{< card link="/docs/flashing/" title="Flashing" subtitle="mise run flash-release, or drag-and-drop." icon="download" >}}
  {{< card link="/docs/upgrading-to-v3/" title="Upgrading to v3" subtitle="Settings reset on both halves, then re-pair." icon="arrow-up" >}}
  {{< card link="/docs/bluetooth/" title="Pairing and Bluetooth" subtitle="Five host profiles, and re-pairing the halves." icon="wifi" >}}
  {{< card link="/docs/studio/" title="Studio" subtitle="Live keymap editing over USB and Bluetooth." icon="adjustments" >}}
  {{< card link="/docs/gaming-mode/" title="Gaming mode" subtitle="The picker and the three game layers." icon="puzzle" >}}
  {{< card link="/docs/underglow/" title="Underglow" subtitle="Layer colours, moods and brightness." icon="light-bulb" >}}
  {{< card link="/docs/building/" title="Building locally" subtitle="The west workspace, doctor, and the build tasks." icon="terminal" >}}
  {{< card link="/docs/reference/firmware/" title="Firmware" subtitle="Pins, modules, Kconfig and the memory budget." icon="chip" >}}
  {{< card link="/docs/reference/troubleshooting/" title="Troubleshooting" subtitle="When a flash or a build doesn't do what it should." icon="support" >}}
  {{< card link="/docs/sofleii/" title="sofleii CLI/TUI (planned)" subtitle="Status, profiles, battery and the flash routines, from a terminal." icon="terminal" >}}
{{< /cards >}}
