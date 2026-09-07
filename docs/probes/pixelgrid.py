#!/usr/bin/env python3
"""Render a grid of views as a tiny inline image (design D).

One node per dot, one pixel of gap between dots, colour carrying state. The
image is placed in an exact number of terminal cells so the prompt's width is
known rather than guessed.

    python3 pixelgrid.py                 # 3x3 demo, node (1,1) current, (1,2) alert
    python3 pixelgrid.py --scale 3       # bigger dots
    python3 pixelgrid.py --probe         # what does this terminal support?

The image is emitted with the kitty protocol using c=/r= (exact cell extent)
and C=1 (do not move the cursor), which together are what make the width
honest. See ../grid-design.md, design D.
"""

import argparse
import array
import base64
import fcntl
import os
import struct
import sys
import termios
import zlib

# state -> RGB
COLORS = {
    "idle": (70, 70, 80),
    "ok": (90, 140, 90),
    "current": (240, 240, 240),
    "alert": (230, 70, 60),
}


def cell_size():
    """Pixels per cell, from TIOCGWINSZ. Returns None if the terminal won't say.

    This is the measurement the whole design depends on: without it the image
    cannot be sized to a whole number of cells.
    """
    buf = array.array("H", [0, 0, 0, 0])
    try:
        fcntl.ioctl(sys.stdout, termios.TIOCGWINSZ, buf)
    except OSError:
        return None
    rows, cols, xpix, ypix = buf
    if not rows or not cols or not xpix or not ypix:
        return None
    return xpix // cols, ypix // rows


def png(pixels, w, h):
    """Minimal RGB PNG encoder, so the probe has no dependencies."""

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    raw = b""
    for y in range(h):
        raw += b"\x00"  # filter: none
        for x in range(w):
            raw += bytes(pixels[y][x])
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def build(grid, scale, cell, pad_to_cells, gap=1):
    """Draw the dots. `grid` is a list of rows of state names.

    `scale` is the size of a dot in pixels and `gap` the space between dots,
    kept independent so a one-pixel gap survives a larger dot. On a HiDPI
    display a one-device-pixel dot is not reliably visible, which is why scale
    defaults above 1.
    """
    rows, cols = len(grid), len(grid[0])
    step = scale + gap
    art_w = cols * step - gap
    art_h = rows * step - gap

    cw, ch = cell
    # Round the image out to a whole number of cells, and centre the art in it.
    want_cells = pad_to_cells or max(1, -(-art_w // cw))
    img_w, img_h = want_cells * cw, ch
    if art_w > img_w or art_h > img_h:
        return None, want_cells, (art_w, art_h), (img_w, img_h)

    bg = (0, 0, 0)
    px = [[bg for _ in range(img_w)] for _ in range(img_h)]
    ox, oy = (img_w - art_w) // 2, (img_h - art_h) // 2
    for r, row in enumerate(grid):
        for c, state in enumerate(row):
            color = COLORS.get(state, COLORS["idle"])
            for dy in range(scale):
                for dx in range(scale):
                    px[oy + r * step + dy][ox + c * step + dx] = color
    return png(px, img_w, img_h), want_cells, (art_w, art_h), (img_w, img_h)


def emit(data, cells, rows=1):
    """Kitty placement: exact cell extent, no cursor movement, quiet."""
    b = base64.standard_b64encode(data)
    out = []
    first = True
    while b:
        chunk, b = b[:4096], b[4096:]
        if first:
            meta = f"a=T,f=100,c={cells},r={rows},C=1,q=2,m={1 if b else 0}"
            first = False
        else:
            meta = f"m={1 if b else 0}"
        out.append(b"\033_G" + meta.encode() + b";" + chunk + b"\033\\")
    return b"".join(out)



def selftest():
    """Checks that need no terminal, so this probe can be run in CI."""
    grid = [
        ["idle", "ok", "idle"],
        ["idle", "current", "alert"],
        ["idle", "ok", "idle"],
    ]
    failures = 0

    def check(name, cond):
        nonlocal failures
        print(("  ok   " if cond else "  FAIL ") + name)
        if not cond:
            failures += 1

    data, cells, art, img = build(grid, 3, (16, 34), 0, gap=1)
    check("3x3 grid fits in one cell", cells == 1)
    check("image is exactly one cell", img == (16, 34))
    check("art is 11x11 at scale 3, gap 1", art == (11, 11))
    check("payload stays small", len(data) < 400)

    esc = emit(data, cells)
    check("placement declares an exact cell extent", b"c=1,r=1" in esc)
    check("placement suppresses cursor movement", b"C=1" in esc)
    check("placement is quiet", b"q=2" in esc)
    check("placement is a well formed APC",
          esc.startswith(b"\033_G") and esc.endswith(b"\033\\"))

    # The failure mode that sank the text strip: a grid that does not fit must
    # refuse rather than render a partial, and therefore lying, picture.
    oversized, _, _, _ = build([["idle"] * 40] * 3, 3, (16, 34), 1, gap=1)
    check("oversized grid refuses instead of clipping", oversized is None)

    # The capacity figures quoted in grid-design.md, each checked at the
    # boundary so the numbers stay tight rather than merely true.
    def fits(cols, rows, scale, cells):
        d, _, _, _ = build([["idle"] * cols for _ in range(rows)],
                           scale, (16, 34), cells, gap=1)
        return d is not None

    check("one cell holds 4x8 nodes at 3px, and no more",
          fits(4, 8, 3, 1) and not fits(5, 9, 3, 1))
    check("one cell holds 5x11 nodes at 2px, and no more",
          fits(5, 11, 2, 1) and not fits(6, 11, 2, 1))
    check("two cells hold 8x8 nodes at 3px, and no more",
          fits(8, 8, 3, 2) and not fits(9, 8, 3, 2))

    print("failures:", failures)
    return 1 if failures else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scale", type=int, default=3, help="pixels per dot")
    ap.add_argument("--gap", type=int, default=1, help="pixels between dots")
    ap.add_argument("--cells", type=int, default=0, help="force cell width")
    ap.add_argument("--probe", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    cell = cell_size()
    if args.probe or cell is None:
        print(f"cell size: {cell if cell else 'UNAVAILABLE (terminal reports 0)'}")
        print(f"TERM={os.environ.get('TERM')} "
              f"TERM_PROGRAM={os.environ.get('TERM_PROGRAM')}")
        if cell is None:
            print("Without a cell size the image cannot be sized to whole cells,")
            print("so the prompt cannot know its own width. Design D needs a")
            print("text fallback for this case.")
            return 1
        if args.probe:
            return 0

    grid = [
        ["idle", "ok", "idle"],
        ["idle", "current", "alert"],
        ["idle", "ok", "idle"],
    ]
    print(f"cell={cell[0]}x{cell[1]}px  "
          f"TERM_PROGRAM={os.environ.get('TERM_PROGRAM')}\n")
    print("Does this read at a glance, or is it a smudge? That is the question")
    print("the automated probes cannot answer.\n")

    # Show a range of dot sizes, because the right one depends on the display
    # and the font, and a single sample would beg the question.
    scales = (args.scale,) if args.cells else (2, 3, 4)
    for scale in scales:
        data, cells, art, img = build(grid, scale, cell, args.cells, args.gap)
        if data is None:
            print(f"  dot={scale}px: does not fit "
                  f"({art[0]}x{art[1]} into {img[0]}x{img[1]})")
            continue
        sys.stdout.write(f"  dot={scale}px gap={args.gap}px   ~/dev[")
        sys.stdout.flush()
        sys.stdout.buffer.write(emit(data, cells))
        sys.stdout.buffer.flush()
        sys.stdout.write(" " * cells)
        print(f"]$ echo hi    ({art[0]}x{art[1]}px in {cells} cell(s), "
              f"{len(data)} bytes)")

    print("\nThe bracket must sit tight against the image, with no gap and no")
    print("overlap: that is the cell accounting being exact. If you instead see")
    print("raw escape text, this terminal lacks the protocol and design D must")
    print("fall back to the text strip.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
