# Probes for the grid design

Experiments backing [../grid-design.md](../grid-design.md).

`integration.py` is the important one: it exercises Whisker's real binary and
configuration, and it is what found the truncation flaw in design A. The others
drive a bare Bash session in a pseudo-terminal to learn what the shell and
terminal allow, and test nothing about Whisker itself.

```sh
python3 -m venv .venv && .venv/bin/pip install pyte

.venv/bin/python docs/probes/integration.py     # design A via the real binary
.venv/bin/python docs/probes/multirow.py        # N-row repaint above the input
.venv/bin/python docs/probes/wrapped_input.py   # the same with wrapped input
.venv/bin/python docs/probes/async.py           # can a signal repaint mid-typing?
.venv/bin/python docs/probes/statefile.py       # background state + refresh key
.venv/bin/python docs/probes/altscreen.py       # design C; see the caveat below
.venv/bin/python docs/probes/graphics.py        # \[ \] width accounting
.venv/bin/python docs/probes/pixelgrid.py --selftest   # design D, the pixel grid
python3 docs/probes/pixelgrid.py               # ...and see it, in a real terminal
.venv/bin/python docs/probes/invariants.py     # Ctrl-L, resize, scrollback
```

The Bash probes need Bash 5+ (`/opt/homebrew/bin/bash` on this machine; edit the
path otherwise). `integration.py` needs `cargo build` first.

Two worth rerunning if you doubt the document:

- `async.py` backs its central claim. The alert should fail to appear until the
  shell is given a chance to run the trap.
- `altscreen.py` proves nothing. pyte does not implement the alternate screen
  buffer (mode 1049), so the overlay appears to leak onto the main screen.
  Source `altscreen.bash` in a real terminal and press Alt-O to test design C
  properly.
- `graphics.py` shows why a *naive* graphics prompt breaks: the real binary
  blanks a sixel via `clean`, and a prompt that under-reports its width
  misplaces the cursor by exactly the amount it hid.
- `pixelgrid.py` is design D and supersedes that objection. It renders the grid
  as a real PNG placed in an exact number of cells with the cursor pinned.
  `--selftest` needs no terminal and is the one to run in CI; running it plain
  in a graphics-capable terminal shows the grid inline.
- `invariants.py` checks that a prompt whose width comes from a drawn-then-
  restored region survives Ctrl-L, a resize, and scrolling. It uses an
  ESC7/ESC8 stand-in rather than a real placement, since pyte cannot draw one,
  but the structural question of whether Readline's arithmetic holds is exactly
  what it can answer.

`configs/` holds the Whisker configurations `integration.py` renders.
