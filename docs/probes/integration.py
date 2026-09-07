"""Design A through Whisker's real binary and configuration.

Unlike the other probes, which drive bare Bash to learn what the terminal
allows, this one exercises the actual renderer. It is what found the truncation
flaw described in ../grid-design.md: a narrow terminal clips the grid strip
even when it is marked shrink = false, hiding an alert behind a row that still
looks like a healthy grid.
"""
import os
import subprocess
import sys

import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BIN = os.path.join(ROOT, "target", "debug", "whisker")
CONFIGS = os.path.join(HERE, "configs")


def render(config, columns, color="never"):
    return subprocess.run(
        [BIN, "render", "--columns", str(columns), "--color", color],
        capture_output=True, text=True, cwd=ROOT,
        env=dict(os.environ, WHISKER_CONFIG=os.path.join(CONFIGS, config)),
    ).stdout.rstrip("\n")


failures = []

print("1. Design A renders on one row at every width, colour-safe")
for columns in (100, 80, 60, 45, 30, 20, 12):
    plain = render("design-a-strip.toml", columns)
    color = render("design-a-strip.toml", columns, "always")
    screen = pyte.Screen(max(columns, 2), 4)
    pyte.Stream(screen).feed(color)
    visible = [line.rstrip() for line in screen.display if line.strip()]
    if len(visible) > 1:
        failures.append(f"width {columns} wrapped to {len(visible)} rows")
    if visible and visible[0] != plain:
        failures.append(f"width {columns}: colour changed the text")
    print(f"   {columns:>3}: {plain}")

print("\n2. A truncated grid hides its alert (the flaw this probe found)")
alert_lost_at = None
for columns in range(40, 14, -2):
    row = render("alert-truncation.toml", columns)
    if "!" not in row and alert_lost_at is None:
        alert_lost_at = columns
        print(f"   {columns:>3}: {row}   <- alert gone, still looks like a grid")
if alert_lost_at is None:
    failures.append("expected the alert to be lost when narrow; it never was")
else:
    print(f"   alert first disappears at {alert_lost_at} columns")

print("\n3. A hostile state file is already neutralised")
state = "/tmp/whisker-probe-state"
with open(state, "w") as handle:
    handle.write("evil \x1b[2J\x1b[H PWNED \x1b[31m\nSECOND LINE\n")
config = os.path.join(CONFIGS, "hostile-state.toml")
with open(config, "w") as handle:
    handle.write(
        'views = ["g"]\n[view.g]\nsegments = ["raw"]\n'
        f'[segment.raw]\ncommand = ["cat", "{state}"]\n'
    )
row = subprocess.run(
    [BIN, "render", "--columns", "100", "--color", "never"],
    capture_output=True, text=True, cwd=ROOT,
    env=dict(os.environ, WHISKER_CONFIG=config),
).stdout
os.remove(config)
if "\x1b" in row:
    failures.append("escape sequences survived into the row")
if row.count("\n") != 1:
    failures.append("more than one line reached the row")
print(f"   escapes stripped: {'\x1b' not in row}; single line: {row.count(chr(10)) == 1}")

if failures:
    print("\nFAILURES:")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)
print("\nPASS: findings in grid-design.md reproduce against the real binary")
