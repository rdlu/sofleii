#!/usr/bin/env python3
"""Screenshots of the sofleii TUI as SVG, from its demo fixture.

Runs `sofleii` with SOFLEII_TUI_DEMO=1 (a fixture snapshot: no keyboard, no
real addresses) in a detached 80x24 tmux session on a private socket,
captures the pane with its ANSI colours (`capture-pane -p -e`), opens the
help overlay with `?` and Flash mode's setup with `F`, capturing each, then
turns each capture into an SVG. Standard library only; needs tmux and a built binary.

    scripts/tui-screenshot.py [--bin PATH] [--out DIR] [--ansi FILE NAME]

`mise run tui-screenshots` builds the release binary and runs this. With
`--ansi FILE NAME` it converts an existing capture instead of running tmux.

The SVG draws a dark terminal (the docs site's dark tokens) so it reads the
same on the light and the dark theme, and embeds the site's 0xProto font
(OFL, site/static/fonts/0xproto/) so it also renders right as a plain
<img> or on GitHub. Each run of same-styled cells is pinned to the cell
grid with textLength, so a fallback glyph can't shift the box drawing.
"""

from __future__ import annotations

import argparse
import base64
import html
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COLS, ROWS = 80, 24
CELL_W, CELL_H, FONT_SIZE, PAD = 8.4, 17.0, 14.0, 12.0

# The site's dark theme (site/assets/css/custom.css) for the base colours;
# the 16 ANSI colours tuned to sit on it.
BG = "#131313"
FG = "#e2e2e2"
FRAME = "#474747"
ANSI16 = [
    "#2a2a2a", "#ff6b6b", "#b9ca4a", "#e7c547", "#7aa6da", "#ffade1", "#70c0b1", "#c6c6c6",
    "#666666", "#ffb4ab", "#d4e96b", "#ffe082", "#a8c8f0", "#ffc8ec", "#9ee0d4", "#ffffff",
]


def xterm256(n: int) -> str:
    if n < 16:
        return ANSI16[n]
    if n < 232:
        n -= 16
        steps = [0, 95, 135, 175, 215, 255]
        r, g, b = steps[n // 36], steps[(n // 6) % 6], steps[n % 6]
        return f"#{r:02x}{g:02x}{b:02x}"
    v = 8 + (n - 232) * 10
    return f"#{v:02x}{v:02x}{v:02x}"


class Style:
    __slots__ = ("fg", "bg", "bold", "dim", "italic", "underline", "reverse")

    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.fg = None
        self.bg = None
        self.bold = self.dim = self.italic = self.underline = self.reverse = False

    def key(self) -> tuple:
        return (self.fg, self.bg, self.bold, self.dim, self.italic, self.underline, self.reverse)

    def apply(self, params: list[int]) -> None:
        i = 0
        while i < len(params):
            p = params[i]
            if p == 0:
                self.reset()
            elif p == 1:
                self.bold = True
            elif p == 2:
                self.dim = True
            elif p == 3:
                self.italic = True
            elif p == 4:
                self.underline = True
            elif p == 7:
                self.reverse = True
            elif p == 22:
                self.bold = self.dim = False
            elif p == 23:
                self.italic = False
            elif p == 24:
                self.underline = False
            elif p == 27:
                self.reverse = False
            elif 30 <= p <= 37:
                self.fg = ANSI16[p - 30]
            elif 90 <= p <= 97:
                self.fg = ANSI16[p - 90 + 8]
            elif 40 <= p <= 47:
                self.bg = ANSI16[p - 40]
            elif 100 <= p <= 107:
                self.bg = ANSI16[p - 100 + 8]
            elif p == 39:
                self.fg = None
            elif p == 49:
                self.bg = None
            elif p in (38, 48) and i + 1 < len(params):
                if params[i + 1] == 5 and i + 2 < len(params):
                    colour = xterm256(params[i + 2])
                    i += 2
                elif params[i + 1] == 2 and i + 4 < len(params):
                    r, g, b = params[i + 2 : i + 5]
                    colour = f"#{r:02x}{g:02x}{b:02x}"
                    i += 4
                else:
                    colour = None
                if p == 38:
                    self.fg = colour
                else:
                    self.bg = colour
                i += 1
            i += 1


SGR = re.compile(r"\x1b\[([0-9;:]*)m")
OTHER_ESC = re.compile(r"\x1b(\[[0-9;?]*[A-Za-z]|\][^\x07]*\x07|.)")


def parse(text: str) -> list[list[tuple[str, tuple]]]:
    """Each row as a list of (char, style key) cells."""
    style = Style()
    rows: list[list[tuple[str, tuple]]] = []
    for line in text.split("\n")[:ROWS]:
        cells: list[tuple[str, tuple]] = []
        pos = 0
        while pos < len(line):
            m = SGR.match(line, pos)
            if m:
                raw = m.group(1).replace(":", ";")
                style.apply([int(x) if x else 0 for x in raw.split(";")] if raw else [0])
                pos = m.end()
                continue
            m = OTHER_ESC.match(line, pos)
            if m:
                pos = m.end()
                continue
            cells.append((line[pos], style.key()))
            pos += 1
        rows.append(cells[:COLS])
    while len(rows) < ROWS:
        rows.append([])
    return rows


# Box-drawing characters are drawn as lines (to the cell edges, so they join
# with no gaps whatever the font), and the full block as a rect.
BOX = {
    "─": "lr", "│": "ud", "┌": "rd", "┐": "ld", "└": "ur", "┘": "ul",
    "├": "udr", "┤": "udl", "┬": "lrd", "┴": "lru", "┼": "udlr",
    "╭": "rd", "╮": "ld", "╰": "ur", "╯": "ul",
}


def font_face() -> str:
    faces = []
    for weight, name in (("normal", "0xProto-Regular.woff2"), ("bold", "0xProto-Bold.woff2")):
        path = ROOT / "site/static/fonts/0xproto" / name
        if not path.exists():
            continue
        data = base64.b64encode(path.read_bytes()).decode()
        faces.append(
            "@font-face{font-family:'0xProto';font-weight:%s;"
            "src:url(data:font/woff2;base64,%s) format('woff2');}" % (weight, data)
        )
    return "".join(faces)


def to_svg(rows: list[list[tuple[str, tuple]]], title: str) -> str:
    width = COLS * CELL_W + 2 * PAD
    height = ROWS * CELL_H + 2 * PAD
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:g} {height:g}" '
        f'width="{width:g}" height="{height:g}" role="img" aria-label="{html.escape(title)}">',
        f"<title>{html.escape(title)}</title>",
        "<style>",
        font_face(),
        "text{font-family:'0xProto',ui-monospace,'DejaVu Sans Mono','Noto Sans Mono',monospace;"
        f"font-size:{FONT_SIZE}px;white-space:pre;font-variant-ligatures:none;"
        "font-feature-settings:'liga' 0,'calt' 0;}",
        "</style>",
        f'<rect width="100%" height="100%" fill="{BG}" stroke="{FRAME}" stroke-width="2"/>',
    ]
    backgrounds = []
    texts = []
    lines: dict[str, list[str]] = {}
    for r, cells in enumerate(rows):
        y_top = PAD + r * CELL_H
        baseline = y_top + CELL_H * 0.76
        for c, (ch, key) in enumerate(cells):
            if ch not in BOX and ch != "█":
                continue
            fg = key[0] or FG
            x0, xm, x1 = PAD + c * CELL_W, PAD + (c + 0.5) * CELL_W, PAD + (c + 1) * CELL_W
            y0, ym, y1 = y_top, y_top + CELL_H / 2, y_top + CELL_H
            if ch == "█":
                backgrounds.append(
                    f'<rect x="{x0:g}" y="{y0 + 2:g}" width="{CELL_W:g}" height="{CELL_H - 4:g}" fill="{fg}"/>'
                )
            else:
                ends = {"l": (x0, ym), "r": (x1, ym), "u": (xm, y0), "d": (xm, y1)}
                d = lines.setdefault(fg, [])
                for e in BOX[ch]:
                    ex, ey = ends[e]
                    d.append(f"M{xm:g} {ym:g}L{ex:g} {ey:g}")
            cells[c] = (" ", key)
        c = 0
        while c < len(cells):
            key = cells[c][1]
            start = c
            while c < len(cells) and cells[c][1] == key:
                c += 1
            run = "".join(ch for ch, _ in cells[start:c])
            fg, bg, bold, dim, italic, underline, reverse = key
            fg = fg or FG
            if reverse:
                fg, bg = (bg or BG), fg
            x = PAD + start * CELL_W
            w = (c - start) * CELL_W
            if bg and bg != BG:
                backgrounds.append(
                    f'<rect x="{x:g}" y="{y_top:g}" width="{w:g}" height="{CELL_H:g}" fill="{bg}"/>'
                )
            if not run.strip():
                continue
            attrs = [f'x="{x:g}"', f'y="{baseline:g}"', f'fill="{fg}"']
            attrs.append(f'textLength="{w:g}" lengthAdjust="spacingAndGlyphs"')
            if bold:
                attrs.append('font-weight="bold"')
            if dim:
                attrs.append('opacity="0.55"')
            if italic:
                attrs.append('font-style="italic"')
            if underline:
                attrs.append('text-decoration="underline"')
            texts.append(f"<text {' '.join(attrs)}>{html.escape(run, quote=False)}</text>")
    paths = [
        f'<path d="{"".join(d)}" stroke="{fg}" stroke-width="1.2" stroke-linecap="square" fill="none"/>'
        for fg, d in lines.items()
    ]
    out += backgrounds + paths + texts + ["</svg>", ""]
    return "\n".join(out)


def tmux(*args: str, check: bool = True) -> str:
    return subprocess.run(
        ["tmux", "-L", "sofleii-shot", *args], check=check, capture_output=True, text=True
    ).stdout


def capture(binary: Path) -> dict[str, str]:
    env_cmd = f"env SOFLEII_TUI_DEMO=1 TERM=xterm-256color COLORTERM=truecolor {binary}"
    tmux("kill-server", check=False)
    tmux("new-session", "-d", "-s", "shot", "-x", str(COLS), "-y", str(ROWS), env_cmd)
    try:
        tmux("set-option", "-t", "shot", "status", "off")
        shots = {}
        for _ in range(50):
            if "sofleii" in tmux("capture-pane", "-p", "-t", "shot"):
                break
            time.sleep(0.1)
        else:
            sys.exit("tui-screenshot: the TUI never drew (is the binary built?)")
        time.sleep(0.5)
        shots["dashboard"] = tmux("capture-pane", "-p", "-e", "-t", "shot")
        tmux("send-keys", "-t", "shot", "?")
        time.sleep(0.5)
        shots["help"] = tmux("capture-pane", "-p", "-e", "-t", "shot")
        tmux("send-keys", "-t", "shot", "Escape")
        tmux("send-keys", "-t", "shot", "F")
        time.sleep(0.5)
        shots["flash"] = tmux("capture-pane", "-p", "-e", "-t", "shot")
        tmux("send-keys", "-t", "shot", "Escape")
        tmux("send-keys", "-t", "shot", "q")
        return shots
    finally:
        tmux("kill-server", check=False)


TITLES = {
    "dashboard": "The sofleii TUI dashboard (demo fixture, 80x24)",
    "help": "The sofleii TUI help overlay (demo fixture, 80x24)",
    "flash": "The sofleii TUI Flash mode setup (demo fixture, 80x24)",
}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bin", type=Path, default=ROOT / "sofleii/target/release/sofleii")
    ap.add_argument("--out", type=Path, default=ROOT / "site/static/img/sofleii")
    ap.add_argument("--ansi", nargs=2, metavar=("FILE", "NAME"))
    args = ap.parse_args()

    if args.ansi:
        shots = {args.ansi[1]: Path(args.ansi[0]).read_text()}
    else:
        if not os.access(args.bin, os.X_OK):
            sys.exit(f"tui-screenshot: no binary at {args.bin}; build it first")
        shots = capture(args.bin)

    args.out.mkdir(parents=True, exist_ok=True)
    for name, text in shots.items():
        path = args.out / f"tui-{name}.svg"
        path.write_text(to_svg(parse(text), TITLES.get(name, f"sofleii TUI: {name}")))
        print(f"wrote {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}")


if __name__ == "__main__":
    main()
