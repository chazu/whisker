# Behavioural checks

`cargo test` covers the renderer's logic. These check what a terminal actually
does with the output, which string comparison cannot settle: whether a styled
row occupies the columns and rows it claims, and whether the prompt survives
real editing.

They need a pseudo-terminal and a screen emulator:

```sh
python3 -m venv .venv && .venv/bin/pip install pyte
.venv/bin/python checks/screen.py        # every view x width on a real screen
.venv/bin/python checks/interactive.py   # Alt-O cycling with input preserved
.venv/bin/python checks/resize.py        # narrowing the terminal mid-session
```

`screen.py` uses your own configuration by default, so it exercises whatever
views you actually run; set `WHISKER_CONFIG` to check another.

Note: pyte drops the rest of a line after a U+FE0F variation selector, with
colour on or off alike, so these strip it before feeding. That is a defect in
the emulator, not in Whisker, whose bytes are identical either way.
