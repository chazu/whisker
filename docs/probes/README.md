# Probes for the grid design

Throwaway experiments backing [../grid-design.md](../grid-design.md). They are
not part of Whisker and test nothing about it; each drives a bare Bash session
in a pseudo-terminal to find out what the shell and terminal will allow.

```sh
.venv/bin/python docs/probes/multirow.py        # N-row repaint above the input
.venv/bin/python docs/probes/wrapped_input.py   # the same with wrapped input
.venv/bin/python docs/probes/async.py           # can a signal repaint mid-typing?
.venv/bin/python docs/probes/statefile.py       # background state + refresh key
.venv/bin/python docs/probes/altscreen.py      # design C; see the caveat below
```

`altscreen.py` proves nothing: pyte does not implement the alternate screen
buffer (mode 1049), so the overlay appears to leak onto the main screen. Source
`altscreen.bash` in a real terminal and press Alt-O to actually test design C.

They need `pyte` and Bash 5+ (`/opt/homebrew/bin/bash` on this machine; edit the
path otherwise). `async.py` is the one worth rerunning if you doubt the design's
central claim: it should show the alert failing to appear until the shell is
given a chance to run the trap.
