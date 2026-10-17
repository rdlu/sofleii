Edit the keymap based on the user's request, then regenerate the SVG and commit.

## Steps

1. Read `config/eyelash_sofle.keymap` to understand the current state before making any changes.
2. If we're on `main` (not a feature branch), create a new feature branch first — never commit keymap edits directly to `main`.
3. Make the requested edits. Refer to the layer/behavior context below.
4. Run `mise run draw` to regenerate the SVG (always do this — never commit a stale SVG).
5. Stage and commit `config/eyelash_sofle.keymap`, `keymap-drawer/eyelash_sofle.yaml`, and `keymap-drawer/eyelash_sofle.svg` together.
6. Push to the current branch.
7. Create or update the PR:
   - **If no PR exists**, create one with `gh pr create --assignee rdlu --label "keymap"` (add `thumb-cluster` too if thumb cluster is involved). Never open an unassigned / unlabeled PR.
   - **If a PR already exists**, update its description — the Summary section must reflect the actual current state of all layers and behaviors (not just what changed this session).
8. Post a PR comment documenting this interaction:
   - What the user asked for
   - What was changed (layer, key, behavior)
   - Any non-obvious rationale
   ```bash
   gh pr comment <number> --body "..."
   ```

## After merge

After the PR is merged, the `[Draw Keymap]` workflow on main regenerates the SVG/YAML and commits them. Always wait for it to finish, then `git checkout main && git pull --ff-only` before starting the next piece of work. Skipping the pull causes merge conflicts in `keymap-drawer/*` on the next branch.

## Local iteration loop (no GH Actions)

If the user wants to iterate fast — try something, flash, test, tweak, flash again — without waiting for CI or opening a PR for every attempt, use the fully local loop:

```bash
# 1. Edit config/eyelash_sofle.keymap
# 2. Regenerate SVG locally
mise run draw

# 3. Build firmware locally (requires `mise run setup` done once)
mise run build-left      # left half (only the left has the keymap logic)
# mise run build-right   # only needed if right-half keys changed (rare)

# 4. Flash — tasks wait up to 60s for NICENANO mount
mise run flash-left
# mise run flash-right

# 5. Test on hardware, then go back to step 1
```

**When to use the local loop vs. the PR loop:**
- **Local loop**: rapid experimentation, timing tweaks, trying out a layout before committing to it. Nothing is pushed; nothing is tracked.
- **PR loop**: once the change is settled and you want it in the repo, commit + push + PR (with assignee + labels) as described above. GH Actions produces the release artifacts.

**⚠️ ZMK Studio overlay gotcha:** if the user has ever used ZMK Studio to edit the keymap live, the stored overlay will override any firmware you flash locally. Symptom: you flash new firmware and the behavior is still the old one. Fix — two-step on the left half: `mise run flash-reset` (or `mise run flash-release-reset` for the GH Actions variant) to wipe the settings partition, then immediately `mise run flash-left` to install the new firmware on top.

## Layer index, key positions, hold-tap behaviors

CLAUDE.md (always loaded) is the reference for these: **Keymap structure** (all
9 layers and how to reach them), **Key position numbering**, and **Hold-tap
behaviors** (all five, with their keys and trigger lists). Read the keymap
itself before trusting any table; the site renders its layer grids straight
from it (https://sofleii.dlu.one/keymap/).

BASE at a glance (col 6 is the centre column: joystick directions + press):
```
Row 0: `    1     2   3    4    5   [VolUp]  6   7     8   9  0  -
Row 1: Tab  Q     W   E    R    T   [VolDn]  Y   U     I   O  P  =
Row 2: Esc  A     S*  D*   F*   G   [Play]   H   J*    K*  L* ;  '
Row 3: \    Z     X   C    V    B   [Next]   N   M     ,   .  /  Ret
Row 4: Mute mo4  F11 GUI  mo1  Bspc [Mute]  Spc mo2  CW  F12/AltGr mo3
* = home-row mod (S D F = Alt Ctrl Shift, J K L = Shift Ctrl Alt)
```

When adding keys:
- `&mt MOD KEY` is a stock mod-tap. Prefer `hml`/`hmr` on the home row; CODE's
  bracket mod-taps use plain `&mt` on purpose.
- `&lt LAYER KEY` (layer-tap) was removed from CODE in `ebc0602` for hold-tap
  lag. Prefer `&mo` on thumbs.

## Common ZMK key names

Modifiers: `LCTRL LSHFT LALT LGUI RCTRL RSHFT RALT`
Special: `BSPC DEL ESC TAB RET SPACE CAPS`
Symbols: `LBKT RBKT LBRC RBRC LPAR RPAR MINUS EQUAL PLUS GRAVE SEMI APOS FSLH BSLH`
Media: `C_VOL_UP C_VOL_DN C_MUTE C_PP C_PREV C_NEXT C_STOP`
Mouse: `&mkp LCLK` `&mkp RCLK` `&mkp MCLK` `&mmv MOVE_UP/DOWN/LEFT/RIGHT`
RGB: `&rgb_ug RGB_TOG/BRI/BRD/SAI/SAD/HUI/HUD/EFF/EFR`
BT: `&bt BT_SEL 0` … `&bt BT_CLR`

## After editing

If the user asks about timing, read the `behaviors {}` block and CLAUDE.md's **Hold-tap behaviors**; use the `adjust-timing` command for changes.
