# Design: a navigable grid of prompts

**Status:** proposal. Nothing here is built.
**Question:** can the prompt show a small map of a 2D grid of prompt contexts,
mark where you are, and flag nodes with new information, without disturbing the
command you are typing?

Short answer: yes for the map and the position, and cheaper than first thought.
Drawn as a small image it costs one character cell rather than screen rows. The
alert part has a hard constraint that shapes the whole design, described under
[Async is not available](#async-is-not-available).

Everything below marked *measured* was checked against Bash 5.3 in a real
pseudo-terminal with a screen emulator; the probes are described in
[Appendix: what was measured](#appendix-what-was-measured). The rest is design
that follows from those measurements.

## The idea

Today one view is visible and Alt-O cycles a list. The proposal is to arrange
views on a 2D grid and move with directional keys:

```
        ops        staging      prod
       ┌─────────┬─────────┬─────────┐
 code  │ ○       │ ○       │ ○       │
       ├─────────┼─────────┼─────────┤
 infra │ ○       │ ●  you  │ !  new  │
       ├─────────┼─────────┼─────────┤
 data  │ ○       │ ○       │ ○       │
       └─────────┴─────────┴─────────┘
```

A column might be an environment and a row a concern, so left and right change
environment while up and down change what you are looking at. That is a guess at
the shape people want; see [Open questions](#open-questions).

## What the prompt can actually do

The current prompt owns exactly one row above the input, repainted from a
`bind -x` callback with `ESC[1A`. Four things were measured before designing on
top of them.

**Multi-row repaint works.** A three-row block above the input repaints
correctly, and Readline restores the typed command and cursor afterwards. The
one-row trick generalises to N rows: move up N, rewrite each, return.

**It survives the bottom of the screen.** With the prompt at the last row of a
scrolled terminal, cursor-up arithmetic still lands correctly, because the
terminal has already scrolled by the time the callback runs.

**It survives wrapped input.** With input wrapping over three screen rows, the
grid still repaints correctly and the command executes intact. This works
because `bind -x` returns the cursor to the *first* input row before invoking
the callback, so "up N" is measured from a stable place.

**Async repaint does not work.** See below. This is the load-bearing finding.

## Async is not available

The tempting design is: a background collector notices something, signals the
shell, and the grid lights up while you type. Measured behaviour says no.

Bash defers a trap until the current foreground command finishes. While Readline
waits for a keystroke, a `SIGUSR1` handler does not run. It was still pending
minutes later. Neither a keystroke, `SIGCONT`, nor `SIGWINCH` reliably flushed
it; one early probe appeared to, but it did not reproduce, so it is treated as
unavailable rather than as a trick worth using.

This is not a limitation to engineer around. It is the constraint that makes the
design honest:

> The grid can only change at moments the user creates: a new prompt, a
> navigation key, or an explicit refresh key.

Everything below follows from that. It also rules out a background daemon that
paints the screen on its own, which is a good thing, because a process writing
to a terminal that Readline believes it owns is how prompts corrupt themselves.

The consequence to state plainly in the UI: **an alert is as fresh as your last
keystroke, not as fresh as the world.** A design that implies otherwise would be
lying to the user.

## Four designs

### A. Inline strip, one row (now the fallback for design D)

Keep the current single information row and add a compact grid to it.

```
[infra·staging] ~/dev/whisker  git:main*   ○○○ │ ○●! │ ○○○
```

Each character is a node, `│` separates grid rows, `●` is you, `!` is an alert.
Measured, that 3x3 strip is 15 columns and the whole example row is 58, so it
fits beside existing content at 80.

This design was prototyped through Whisker's real configuration, with the strip
as an ordinary `command` segment, and it renders correctly on one row at every
width from 100 down to 12. But that prototype exposed a flaw that has to be
fixed before design A is safe.

**A truncated grid lies.** Whisker shortens the longest shrinkable segment and,
when nothing more can give, clips the whole row. Marking the strip
`shrink = false` does not exempt it from that final clip. Measured with the
alert in the last cell:

```
28 cols: [env] …ker  ○○○ │ ○●○ │ ○○!
24 cols: [env] …  ○○○ │ ○●○ │ ○…      <- alert gone, still looks like a grid
```

At 24 columns the row still resembles a healthy grid while the alert has
silently vanished. A status display that hides an alert is worse than one that
is absent, because the user reads calm and believes it.

Design A therefore needs one new capability that does not exist today: a
segment that is rendered **whole or not at all**, with an alternative for when
it does not fit. Something like

```toml
[segment.grid]
command = ["whisker-grid", "--strip"]
atomic = true                      # never clip; drop or swap instead
fallback = ["whisker-grid", "--summary"]   # e.g. "⟨infra,staging⟩ 2!"
```

so a narrow terminal shows an honest 18-column summary rather than a
convincing, wrong picture. `atomic` is useful well beyond the grid: any segment
whose meaning depends on being complete wants it.

- **Cost:** zero extra rows.
- **Risk:** low. It is the existing mechanism with a longer string, so
  everything already verified about width, shrinking, and styling applies
  unchanged.
- **Limit:** no room for node names, so it only works once you know the layout.

### B. Block map, N rows

Render the grid as its own block above the information row, shown in the sketch
at the top.

- **Cost:** 3 rows for a 3-row grid, which is 12% of a 24-row terminal and 8%
  of a 40-row one. Drawn with the borders shown in the sketch above it is 7
  rows, or 29% of a 24-row terminal, permanently. Borders are expensive.
- **Risk:** medium. Multi-row repaint is verified, but every existing invariant
  (Ctrl-L, resize, the first-Alt-O-after-resize repaint) must be re-established
  for N rows rather than one.
- **Mitigation:** show the block only while navigating. Hold a modifier and the
  map appears; release and it collapses to the strip from A. This is the "peek"
  idea and it is what makes B affordable.

### C. Alternate screen overlay

On a navigation key, switch to the alternate screen buffer (`ESC[?1049h`), draw
a full map, take a keystroke, restore. A terminal that implements the mode
restores the previous screen itself, so the prompt cannot be corrupted.

Unverified, unlike the rest of this document. The screen emulator used for the
other probes does not implement `1049`, so the probe showed the map leaking onto
the main screen rather than being restored. That is the emulator's gap, not a
result about real terminals, but it means this option needs checking in an
actual terminal before being chosen. It also hints at the real risk: any
terminal or multiplexer that does not support the mode degrades badly.

- **Cost:** zero rows in steady state.
- **Risk:** the interaction is modal, which is a different feel from a prompt,
  and it competes with the pager the user may already be in. Behaviour on
  terminals lacking `1049` is unknown and must be checked.
- **Best for:** a "show me everything" key rather than routine movement.

### D. A pixel grid, drawn as an image (recommended)

Since the grid is fundamentally a picture, draw it as one: a dot per node, a
one-pixel gap between dots, and colour carrying state, sized so the whole thing
fits inside one or two ordinary character cells.

```
   . . .        .  idle       @  you
   . @ !        o  ok         !  needs attention
   . o .
```

An earlier revision of this document rejected graphics outright. That
rejection was wrong, and the reasoning is worth keeping because two of the
three objections dissolve once the image is placed properly.

**Width is knowable, not guessed.** The objection was that Readline needs the
printing width of `PS1` and an image has none. The kitty protocol answers this
directly: `c=` and `r=` place an image in an *exact* number of cells, scaling
it to fit, and `C=1` tells the terminal not to move the cursor at all. So the
prompt emits the escape inside `\[ \]`, where its zero-width claim is now
true, and then emits exactly `c` real spaces for the image to sit on. Readline
counts those spaces, and the count is correct by construction.

This was measured with an analogue pyte can execute: a prompt that draws two
visible cells, restores the cursor, and then spends two countable spaces.
With a 16-character command, Ctrl-A landed on column 4, exactly where the
command begins, and Ctrl-E on column 20. Exact, where the naive `\[ \]`
prompt was off by 12.

**It needs no relaxation of `clean`.** The other objection was that graphics
escapes must pass through the function that strips control characters, which is
the defence protecting the untrusted alert state file. That confused two paths.
`clean` is applied to *segment output*, which is untrusted because anything can
write it. A pixel grid is not segment output: Whisker generates the bytes
itself from its own config and its own view states, the way it already emits
its own SGR colour codes without laundering them. The state file keeps flowing
through `clean` and keeps being neutralised. Verified that it still is.

**Size.** Measured with a real PNG encoder. At a 16x34 pixel cell, a 3x3 grid
with three-pixel dots and one-pixel gaps is 11x11 pixels and fits in **one
cell**, with a payload around 110 bytes. One cell holds up to 4x8 nodes at that
scale, or 5x11 with two-pixel dots; two cells hold 8x8. The grid is therefore
free in the only currency design B was expensive in, which is screen space.

One caveat found while building it: a single *device* pixel is not reliably
visible on a HiDPI display, so the dot wants to be three or four pixels with
the gap held at one. The probe keeps dot size and gap independent for exactly
this reason.

**What it still costs.**

- **Portability.** Terminals without the protocol must get the text strip from
  design A as a fallback, so A is not replaced, it is demoted to the fallback
  path. Support is queryable at startup, so this is detectable rather than
  hoped for.
- **A cell size is required.** Sizing to whole cells needs pixels-per-cell from
  `TIOCGWINSZ`, and some terminals report zero. That is also detectable, and it
  selects the same text fallback.
- **Multiplexers.** tmux and Zellij do not implement the kitty protocol.
- **Colour alone carries the alert.** That is bad for colour-blind users and
  invisible under `NO_COLOR`, which Whisker already honours strictly. The
  alert dot needs a second channel, such as brightness or a fifth blank cell,
  or the fallback must engage.

`pixelgrid.py` implements all of this and self-tests without a terminal:
`python3 docs/probes/pixelgrid.py --selftest`. Run it without arguments in a
real terminal to see the grid inline.

**Recommendation:** build D, with A's text strip as the fallback for terminals
that cannot draw it, and C as an on-demand full map. B's always-visible text
block is no longer worth building, since D delivers the same map for a fraction
of the space.

The `atomic` and `fallback` work from step 0 is *more* necessary now, not less:
"draw the picture, or cleanly swap to the strip, but never show half of
either" is precisely what `atomic` expresses.

## Alerts

### What raises one

An alert must come from the same kind of thing a segment already is: a local
command that exits non-zero, prints something, or reports a count. Reusing the
existing `command` mechanism means no new trust boundary and no new evaluator.

```toml
[node.infra.prod]
segments = ["directory", "kubernetes"]
alert.command = ["kubectl", "get", "events", "--field-selector", "type=Warning"]
alert.when = "output"
```

`when` could be `output` (non-empty output), `exit` (non-zero), or `changed`
(output differs from last seen). `changed` is the one that matches "new
information the user should be aware of", and it needs somewhere to remember the
previous value.

### When it is collected

This is where the async constraint bites. Collecting every node's alert on every
prompt does not scale. Measured on this machine, one `kubernetes` segment takes
a median of 29 ms against a local kubeconfig, against 2 ms for a directory and
5 ms for Git. Nine such nodes is roughly a quarter of a second added to every
prompt, before anything touches a network; a node whose check is a real remote
call would be far worse, and the collectors have no timeout.

The only workable shape is to decouple collection from display:

- A background process, started once per shell or shared across shells, polls
  each node's alert command on its own schedule and writes a small state file.
- The prompt hook and the navigation keys only *read* that file. Reading is a
  few microseconds and cannot block on the network.

This was measured end to end: a background writer changed the state file while a
command was half-typed, and a `bind -x` key picked up the change and repainted
without disturbing the input. The freshness limit stands, since the user still
has to press something, but the cost of *checking* becomes free.

State file, one line per node, easy to write from a shell script:

```
infra staging ok   2026-09-06T22:10:03
infra prod    alert 2026-09-06T22:10:05 3 warning events
```

This file is untrusted input, since anything on the machine can write it.
Verified that the existing protection already covers it: a state file
containing `ESC[2J ESC[H PWNED ESC[31m` and a second line, read through an
ordinary segment, rendered as one row with the escapes stripped to spaces and
the extra line discarded. The existing `clean` and first-line rules are exactly
the right defence, so this needs no new machinery, only the discipline of
reading the file through a normal segment rather than around it.

### Clearing one

An alert that never clears becomes wallpaper. Options, in increasing order of
effort: clear on visiting the node, clear when the underlying command stops
reporting, or clear on an explicit dismiss key. Visiting is the most predictable
and needs no extra state, so it is the one to start with.

## Keybinding

Alt-O currently forwards to a private sequence bound with `bind -x`. The same
mechanism extends directly. The awkward part is that arrow keys and Alt-arrows
are heavily overloaded by terminals and multiplexers, and Alt-arrow in
particular is commonly intercepted before the shell sees it.

A safer default is a small set of unmodified letter keys behind a prefix, in the
spirit of tmux: a leader key, then `h`/`j`/`k`/`l` or the arrows. That costs one
extra keystroke and buys predictability. It also gives somewhere obvious to hang
"show the full map" (C) and "dismiss alerts".

## What the current code already gives, and what it lacks

Design A was prototyped through Whisker's real configuration and binary rather
than sketched, which is how the truncation flaw above was found. What that
prototype showed:

**Already works, no new code.** A grid strip as an ordinary `command` segment
renders on one row at every width from 100 to 12, with styling, and stays
colour-safe. An alert count read from a state file renders and correctly
disappears, separator included, when the count is zero. Hostile content in that
state file is already neutralised.

**Missing, and needed.**

- `atomic` and `fallback` on a segment, so a grid is never shown truncated.
  This is the correctness gap; nothing else on this list can cause a wrong
  reading.
- 2D movement. `view next --current NAME` walks a single ring, verified: four
  views cycle `a1 → a2 → b1 → b2 → a1`. A grid needs something like
  `view move --direction left|right|up|down --current NAME`, plus grid
  coordinates in the config for it to move through.
- A place to store per-node alert state and the last-seen value that `changed`
  would compare against.

The order matters: `atomic` is worth adding on its own merits, independent of
whether the grid is ever built.

## Configuration sketch

The existing format extends without a new concept: a node is a view with
coordinates.

```toml
grid.rows = ["code", "infra", "data"]
grid.columns = ["ops", "staging", "prod"]
start = { row = "infra", column = "staging" }

[node.infra.staging]
segments = ["directory", "kubernetes"]
alert = { command = ["check-staging"], when = "exit" }

[grid.display]
mode = "strip"          # strip | block | overlay
position_style = { fg = "green", bold = true }
alert_style = { fg = "red", bold = true }
```

Sparse grids need a decision: if `[node.data.prod]` is undefined, is it an empty
cell you can move onto, or is it skipped? Skipping is friendlier; showing a hole
is more honest about the shape.

## What could go wrong

- **The grid becomes noise.** Nine idle nodes carry almost no information most
  of the time. Mitigation: show the grid only when something is non-default,
  otherwise show position alone.
- **Alerts train people to ignore them.** A flapping check is worse than no
  check. Any `changed` alert needs debouncing, and probably a minimum interval.
- **Two display paths must agree.** With D drawing an image and A drawing text,
  a node that reads as alerting in one must alert in the other. Divergence
  between them is the new version of the truncation bug.
- **Colour is the only channel in D.** Under `NO_COLOR`, or for a colour-blind
  user, a red dot and a green dot are the same dot. This needs a second channel
  or it must fall back to text.
- **A grid may be the wrong model.** If most people have five contexts and no
  natural second axis, a list with alert markers delivers most of the value for
  a fraction of the cost. Worth checking before building.

## Suggested order

0. **`atomic` and `fallback` on segments.** Without this a narrow terminal
   shows a truncated grid that hides alerts, which is the one failure mode that
   makes the feature actively harmful. Worth doing regardless of the grid.
1. **Strip rendering only.** Static grid from config, position marker, no
   alerts. Proves the layout inside the existing one-row mechanism, and it is
   the fallback every terminal gets, so it is worth building first even though
   D is the recommended display. Already prototyped through the real
   configuration, so this is mostly a matter of generating the strip rather
   than hand-writing it.
2. **The pixel grid (D)** behind capability detection, falling back to step 1.
   `pixelgrid.py` already renders it; this is a matter of porting that to Rust
   and querying support and cell size at startup.
3. **Navigation.** Leader key plus directional keys, with the existing
   input-preservation checks extended to cover it.
4. **Alert state file.** Reading and display only, with a documented format, so
   anything can write it.
5. **A collector** that populates the file on a schedule.
6. **Overlay (C)** for the full map.

Steps 1 to 3 are useful alone: a grid you can move through is worth having even
if nothing ever alerts.

## Open questions

- Is a 2D grid the right model, or is this a list with alerts?
- What is the second axis in practice? Environment × concern is a guess.
- Should the grid be shared between shells, or is each shell's position its own?
  Shared position is surprising; shared *alerts* are clearly right.
- Does an alert need a severity, or is one level enough?

## Appendix: what was measured

Bash 5.3.15 on macOS, in a real pseudo-terminal, rendered with a screen
emulator. Each probe drove an actual interactive shell.

| Probe | Result |
| --- | --- |
| 3-row block repainted from `bind -x` | Works; typed input and cursor preserved |
| Same, prompt at the bottom of a scrolled screen | Works; cursor-up lands correctly |
| Same, input wrapped over 3 screen rows | Works; command executed intact |
| `SIGUSR1` trap while Readline waits | **Does not run**; deferred indefinitely |
| Keystroke, `SIGCONT`, `SIGWINCH` as a flush | Did not reliably deliver the trap |
| Background state file + `bind -x` refresh | Works; picked up mid-typing without disturbing input |
| Vertical cost of a 3x3 map | 3 rows plain = 12% of 24 rows; 7 rows bordered = 29% |
| Cost of one collector | 2 ms directory, 5 ms Git, 29 ms Kubernetes (median of 5) |
| Alternate-screen overlay (design C) | **Not established**; the emulator lacks `1049` |
| Width of the design A strip | 15 columns for 3x3; 58 for the whole example row |
| Design A through the real binary | One row at every width 100..12; colour-safe |
| **Truncated grid hides an alert** | **At 24 columns the alert vanishes while the row still looks like a grid** |
| Alert count from a state file | Works; segment and separator vanish at zero |
| Hostile state file (`ESC[2J`, extra line) | Already neutralised by the existing `clean` and first-line rules |
| `view next` as 2D movement | Single ring only; 2D needs a new subcommand |
| Sixel through the real binary | Blanked by `clean`; a picture must be generated internally, not via a segment |
| Prompt that under-reports its width | Ctrl-A put the cursor at column 0 against text at column 12 |
| Image sized to whole cells, cursor pinned | Exact: Ctrl-A at column 4, Ctrl-E at 20 for a 16-char command |
| 3x3 pixel grid, 3px dots, 1px gaps | 11x11 px, fits one 16x34 cell, ~110 byte payload |
| Node capacity of one cell | 4x8 at 3px dots, 5x11 at 2px; two cells give 8x8 |

The async result is the one that matters, because it converts "live dashboard"
into "map that updates when you touch it". Better to know that before building
than after.
