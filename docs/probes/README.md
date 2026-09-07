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
.venv/bin/python docs/probes/graphics.py        # design D; sixel and \[ \] accounting
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
- `graphics.py` decides design D on two grounds an emulator can judge: the real
  binary blanks a sixel via `clean`, and a prompt that under-reports its width
  misplaces the cursor. Whether a terminal *draws* the image is left to
  `graphics.bash`, which must be sourced in a real terminal, and which is not
  needed to reach the conclusion.

`configs/` holds the Whisker configurations `integration.py` renders.
