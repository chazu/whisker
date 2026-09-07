# Probes for the grid design

Experiments backing [../grid-design.md](../grid-design.md).

`integration.py` is the important one: it exercises Whisker's real binary and
configuration, and it is what found the truncation flaw in design A. It also
covers design D's safety claims: that Whisker's own escapes reach the row while
a segment's identical escape is blanked, and that a graphics payload sent
through a segment comes out inert. The others drive a bare Bash session in a
pseudo-terminal to learn what the shell and terminal allow, and test nothing
about Whisker itself.

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
python3 docs/probes/docaudit.py                # do the doc's numbers still hold?
rustc -O docs/probes/pixelgrid.rs -o /tmp/pg && /tmp/pg   # design D's cost in Rust
rustc -O --test docs/probes/pixelgrid.rs -o /tmp/pgt && /tmp/pgt
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
- `docaudit.py` re-derives the measurements quoted in `grid-design.md` from the
  code and fails if they have drifted. It needs neither a terminal nor any
  dependency, though it will build and run `pixelgrid.rs` if `rustc` is present.
  It has already earned its place twice: switching the encoder to RGBA moved the
  payload from 110 to 124 bytes without the document noticing, and it later
  caught a figure updated in the cost table but not the appendix.
- `pixelgrid.rs` answers whether design D is affordable in Rust. It is a
  complete renderer, dots through PNG through base64 through escape, with no
  dependencies: PNG needs a zlib stream, so it carries a small fixed-Huffman
  deflate encoder. Run it plain to benchmark, `--test` for its own checks, or
  `--dump N` to write one PNG for an independent decoder to verify.

`configs/` holds the Whisker configurations `integration.py` renders.
