#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Generate config/eyelash_sofle.json from the shield's physical layout.

The `zmk,physical-layout` node in
config/boards/shields/eyelash_sofle/eyelash_sofle-layouts.dtsi is the single
source of the key geometry. This writes it out as a QMK-style info.json, the
format keymap-drawer's `draw -j`, the Draw Keymap workflow, the docs site's
layer grids and keymap-editor read. `row`/`col` come from the matrix
transform in eyelash_sofle.dtsi, `sensors` from its keymap sensors.

    scripts/layout-to-json.py          # rewrite config/eyelash_sofle.json
    scripts/layout-to-json.py --check  # exit 1 if the JSON is out of date

Standard library only.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHIELD = ROOT / "config/boards/shields/eyelash_sofle"
LAYOUT_DTSI = SHIELD / "eyelash_sofle-layouts.dtsi"
SHIELD_DTSI = SHIELD / "eyelash_sofle.dtsi"
OUT = ROOT / "config/eyelash_sofle.json"


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def units(v):
    """Centi-units (or centi-degrees) to a JSON number, 1.0 -> 1."""
    f = v / 100
    return int(f) if f == int(f) else f


def keys():
    text = strip_comments(LAYOUT_DTSI.read_text())
    num = r"\(?\s*(-?\d+)\s*\)?"
    rows = re.findall(r"<&key_physical_attrs" + (r"\s+" + num) * 7 + r"\s*>", text)
    if not rows:
        sys.exit(f"{LAYOUT_DTSI}: no key_physical_attrs entries found")
    return [tuple(int(v) for v in r) for r in rows]


def matrix():
    text = strip_comments(SHIELD_DTSI.read_text())
    m = re.search(r"default_transform:.*?map\s*=\s*<(.*?)>", text, re.S)
    if not m:
        sys.exit(f"{SHIELD_DTSI}: default_transform map not found")
    return [(int(r), int(c)) for r, c in re.findall(r"RC\((\d+),\s*(\d+)\)", m.group(1))]


def sensors():
    text = strip_comments(SHIELD_DTSI.read_text())
    m = re.search(r"compatible\s*=\s*\"zmk,keymap-sensors\".*?sensors\s*=\s*<([^>]*)>", text, re.S)
    out = []
    for ref in re.findall(r"&(\w+)", m.group(1) if m else ""):
        node = re.search(rf"\b{ref}:\s*(\w+)\s*\{{(.*?)\}};", text, re.S)
        name = node.group(1) if node else ref
        compat = re.search(r"compatible\s*=\s*\"([^\"]+)\"", node.group(2)) if node else None
        out.append({"ref": ref, "name": name, "identifier": name,
                    "compatible": compat.group(1) if compat else ""})
    return out


def render():
    ks, rc = keys(), matrix()
    if len(ks) != len(rc):
        sys.exit(f"{len(ks)} keys in {LAYOUT_DTSI.name} but {len(rc)} "
                 f"positions in default_transform: the key order must match")
    lines = []
    for (w, h, x, y, rot, rx, ry), (row, col) in zip(ks, rc):
        k = {"row": row, "col": col, "x": units(x), "y": units(y)}
        if w != 100:
            k["w"] = units(w)
        if h != 100:
            k["h"] = units(h)
        if rot:
            k.update(r=units(rot), rx=units(rx), ry=units(ry))
        lines.append("        " + json.dumps(k))
    head = {
        "id": "eyelash_sofle",
        "name": "Eyelash Sofle",
        "_generated": "from config/boards/shields/eyelash_sofle/eyelash_sofle-layouts.dtsi "
                      "by scripts/layout-to-json.py; edit the .dtsi and run `mise run layout`",
    }
    out = "{\n"
    out += "".join(f"  {json.dumps(k)}: {json.dumps(v)},\n" for k, v in head.items())
    out += '  "layouts": {\n    "default_layout": {\n      "layout": [\n'
    out += ",\n".join(lines)
    out += "\n      ]\n    }\n  },\n"
    out += '  "sensors": ' + json.dumps(sensors()) + "\n}\n"
    json.loads(out)  # sanity: valid JSON
    return out


def main():
    new = render()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text() if OUT.exists() else ""
        if old != new:
            sys.exit(f"{OUT.relative_to(ROOT)} is out of date with "
                     f"{LAYOUT_DTSI.relative_to(ROOT)}: run `mise run layout` and commit it")
        print(f"{OUT.relative_to(ROOT)} is up to date")
        return
    OUT.write_text(new)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
