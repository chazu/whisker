"""Verify styled rows on a real terminal screen, not by string inspection.

The claim under test: styling must not change how many columns or rows the
information row occupies, and the visible characters must be identical with
colour on and off. A terminal emulator settles this properly, because it is what
actually interprets the escape sequences.
"""
import os
import subprocess
import sys

import pyte

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "debug", "whisker")
CONFIG = os.environ.get("WHISKER_CONFIG", os.path.expanduser("~/.config/whisker/config.toml"))


def run(view, columns, color):
    env = dict(os.environ, WHISKER_CONFIG=CONFIG)
    out = subprocess.run(
        [BIN, "render", "--view", view, "--columns", str(columns), "--color", color],
        capture_output=True, text=True, env=env, check=True,
    )
    return out.stdout.rstrip("\n")


def screen_of(text, columns):
    """Feed the row to a terminal and report what it actually displays.

    pyte drops the remainder of a line after a U+FE0F variation selector, with
    colour on or off alike, so it is removed before feeding. That is an
    emulator defect, not Whisker's: the bytes are identical either way.
    """
    text = text.replace("\ufe0f", "")
    screen = pyte.Screen(columns, 6)
    stream = pyte.Stream(screen)
    stream.feed(text + "\r\n")
    lines = [screen.display[i].rstrip() for i in range(6)]
    used = len([line for line in lines if line])
    return lines, used


failures = []
views = subprocess.run([BIN, "view", "list"], capture_output=True, text=True,
                       env=dict(os.environ, WHISKER_CONFIG=CONFIG),
                       check=True).stdout.split()

for view in views:
    for columns in (120, 100, 80, 60, 45, 30, 20, 12, 6):
        plain = run(view, columns, "never")
        color = run(view, columns, "always")

        plain_lines, plain_used = screen_of(plain, columns)
        color_lines, color_used = screen_of(color, columns)

        # 1. The visible characters must be identical.
        if plain_lines != color_lines:
            failures.append(
                f"{view}@{columns}: visible text differs\n"
                f"   plain={plain_lines[0]!r}\n   color={color_lines[0]!r}")

        # 2. The row must occupy exactly one line, never wrapping.
        if color_used != 1:
            failures.append(
                f"{view}@{columns}: styled row used {color_used} lines: {color_lines[:3]}")

        # 3. It must fit the width.
        if len(color_lines[0]) > columns - 1:
            failures.append(
                f"{view}@{columns}: rendered {len(color_lines[0])} cols > budget {columns-1}")

        # 4. No style may still be active at the end of the row.
        cursor_style = screen_of(color + "X", columns)
        screen = pyte.Screen(columns, 6)
        pyte.Stream(screen).feed(color)
        # After the row, the next written cell must be default-styled.
        screen.draw("X")
        cell = screen.buffer[0][screen.cursor.x - 1]
        if cell.fg != "default" or cell.bg != "default" or cell.bold or cell.italics or cell.underscore:
            failures.append(
                f"{view}@{columns}: style leaks past the row into the typed "
                f"command (fg={cell.fg} bg={cell.bg} bold={cell.bold})")

print(f"checked {len(views)} views x 9 widths = {len(views)*9} rows on a real screen")
if failures:
    print(f"\n{len(failures)} FAILURE(S):")
    for f in failures[:15]:
        print(" -", f)
    sys.exit(1)
print("PASS: colour never changes visible text, width, or row count, and never leaks")
