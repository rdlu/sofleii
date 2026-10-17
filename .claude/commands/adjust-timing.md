Adjust hold-tap timing parameters based on the user's description of the problem.

## Steps

1. Ask the user to describe the symptom if not already stated (false triggers, mods not firing, key repeat broken, etc.).
2. If we're on `main`, create a feature branch first — never commit timing changes directly to `main`.
3. Read `config/eyelash_sofle.keymap` — find the `behaviors {}` block with `hml`, `hml_idx`, `hmr`, `hmr_idx` and `ralt_mt`.
4. Apply the appropriate change (see table below).
5. Run `mise run draw` to regenerate the SVG.
6. Commit all three files (`keymap`, `yaml`, `svg`) and push.
7. Create or update a PR with `gh pr create --assignee rdlu --label "keymap"` — never leave the PR unassigned or unlabeled.
8. After merge, wait for the `[Draw Keymap]` workflow to finish on main, then `git checkout main && git pull --ff-only` before starting more work.

## Parameters and what they control

| Parameter | Location in file | Effect of increasing | Effect of decreasing |
|-----------|-----------------|----------------------|----------------------|
| `tapping-term-ms` | all five behaviors | harder to trigger hold accidentally | hold fires sooner / easier to trigger |
| `require-prior-idle-ms` | all five (230 ms; 70 ms on the `_idx` variants) | fewer false mods during fast typing | mods respond faster after adjacent keys |
| `quick-tap-ms` | all five behaviors | longer window for key repeat | shorter repeat window |
| `flavor` | all five (`hold-preferred` on the `_idx` variants) | see flavors below | — |

## Flavors

- `balanced` *(hml, hmr, ralt_mt)*: hold if the term expires, or if another key is **pressed and released** inside it. Good balance for typing.
- `hold-preferred` *(hml_idx, hmr_idx)*: hold if the term expires or another key is **pressed** inside it. Commits sooner; used on the index-finger Shifts so CamelCase isn't sluggish. Safe there only because of the positional restriction and the short idle window.
- `tap-preferred`: hold only when the term expires. Most conservative.
- `tap-unless-interrupted`: tap unless another key is pressed before the term expires (then hold); the term expiring alone gives a tap.

## Symptom → fix

| Symptom | Fix |
|---------|-----|
| Mod fires while typing fast (false trigger) | Increase `require-prior-idle-ms` (+25 ms steps) |
| Mod doesn't fire when I want it | Decrease `tapping-term-ms` (-20 ms steps) or switch to `tap-unless-interrupted` |
| Key repeat doesn't work | Increase `quick-tap-ms` to ≥ your fastest double-tap speed |
| Mod fires too late / laggy | Decrease `tapping-term-ms` |
| Same-hand mod combos (Ctrl+Shift on D+F) fail | The trigger lists must include that hand's own home-row positions (28–30 / 34–36, added in `12fb68e`, #17) |
| AltGr accent not working | Verify `ralt_mt` has **no** `hold-trigger-key-positions` |

## Behaviors and their keys

```
hml      → S(LALT)  D(LCTRL)     [left home row,  balanced,       idle 230]
hml_idx  → F(LSHFT)              [left index,     hold-preferred, idle 70]
hmr      → L(LALT)  K(RCTRL)     [right home row, balanced,       idle 230]
hmr_idx  → J(RSHFT)              [right index,    hold-preferred, idle 70]
ralt_mt  → right-thumb F12 (hold = RALT/AltGr)  [no positional restriction]
```

Shared: `tapping-term-ms=280`, `quick-tap-ms=175`. Keep `hml`/`hmr` and the two
`_idx` variants consistent with each other unless differentiating on purpose.

## Important

The site's hold-tap table is generated from the keymap, so it updates itself. Do update CLAUDE.md's **Hold-tap behaviors** table, this file's block above, and the prose in `site/content/keymap/hold-tap.md` if a change alters what it describes.

## Local iteration (no GH Actions)

Timing tuning is almost always faster to test locally than through the PR/CI cycle — the feel of a mod or tap can only be judged by using the keyboard, and waiting for a full GH Actions build between each attempt is painful. Use this loop:

```bash
# Edit config/eyelash_sofle.keymap — change tapping-term-ms, require-prior-idle-ms, etc.
mise run draw           # regenerate SVG
mise run build-left     # build left half (only the left needs new timing)
mise run flash-left     # prompts for double-tap, waits for NICENANO mount, flashes
# Test on hardware — type, check mod triggers, check repeat behavior
# Repeat
```

Only once the values feel right, commit + push + open a PR (with `--assignee rdlu --label "keymap"`). No need to open a PR for every tweak attempt.

**Studio overlay warning:** if the user has ever used ZMK Studio and the new timing doesn't seem to take effect, the stored overlay is overriding the compiled firmware. Run `mise run flash-reset` on the left half first, then `mise run flash-left`.
