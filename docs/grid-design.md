# Design: a navigable grid of prompts

**Status:** proposal. Nothing here is built.
**Question:** can the prompt show a small map of a 2D grid of prompt contexts,
mark where you are, and flag nodes with new information, without disturbing the
command you are typing?

Short answer: yes for the map and the position, with a measured cost of one to
three screen rows. The alert part has a hard constraint that shapes the whole
design, described under [Async is not available](#async-is-not-available).

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

## Three designs

### A. Inline strip, one row (recommended first step)

Keep the current single information row and add a compact grid to it.

```
[infra·staging] ~/dev/whisker  git:main*   ○○○ │ ○●! │ ○○○
```

Each character is a node, `│` separates grid rows, `●` is you, `!` is an alert.
A 3x3 grid costs 11 columns. It fits beside existing content at 80 columns and
degrades by dropping to a summary when narrow:

```
⟨infra,staging⟩ 2!
```

- **Cost:** zero extra rows.
- **Risk:** low. It is the existing mechanism with a longer string, so
  everything already verified about width, shrinking, and styling applies
  unchanged.
- **Limit:** no room for node names, so it only works once you know the layout.

### B. Block map, N rows

Render the grid as its own block above the information row, shown in the sketch
at the top.

- **Cost:** measured at 3 rows for a 3-row grid, plus borders if drawn. On a
  24-row terminal a bordered 3x3 map costs about 12% of the screen, permanently.
- **Risk:** medium. Multi-row repaint is verified, but every existing invariant
  (Ctrl-L, resize, the first-Alt-O-after-resize repaint) must be re-established
  for N rows rather than one.
- **Mitigation:** show the block only while navigating. Hold a modifier and the
  map appears; release and it collapses to the strip from A. This is the "peek"
  idea and it is what makes B affordable.

### C. Alternate screen overlay

On a navigation key, switch to the alternate screen buffer (`ESC[?1049h`), draw
a full map, take a keystroke, restore. The terminal restores the previous screen
byte for byte, so the prompt cannot be corrupted.

- **Cost:** zero rows in steady state.
- **Risk:** the interaction is modal, which is a different feel from a prompt,
  and it competes with the pager the user may already be in.
- **Best for:** a "show me everything" key rather than routine movement.

**Recommendation:** build A, then add C as an on-demand overlay. Treat B's
always-visible block as opt-in, since its permanent row cost is the largest
thing being asked of the user.

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
prompt is unacceptable: the existing `ops` view alone took about 199 ms, and a
3x3 grid of such nodes would add seconds to every prompt.

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

Whisker should treat this file as untrusted input and clean it exactly as it
cleans segment output today, since anything that writes it can otherwise inject
terminal control sequences into a prompt.

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

- **The grid becomes noise.** Nine nodes of `○` carry almost no information most
  of the time. Mitigation: show the strip only when something is non-default,
  otherwise show position alone.
- **Alerts train people to ignore them.** A flapping check is worse than no
  check. Any `changed` alert needs debouncing, and probably a minimum interval.
- **The row cost is permanent but the benefit is occasional.** This is the
  strongest argument for A over B.
- **A grid may be the wrong model.** If most people have five contexts and no
  natural second axis, a list with alert markers delivers most of the value for
  a fraction of the cost. Worth checking before building.

## Suggested order

1. **Strip rendering only.** Static grid from config, position marker, no
   alerts. Proves the layout inside the existing one-row mechanism.
2. **Navigation.** Leader key plus directional keys, with the existing
   input-preservation checks extended to cover it.
3. **Alert state file.** Reading and display only, with a documented format, so
   anything can write it.
4. **A collector** that populates the file on a schedule.
5. **Overlay (C)** for the full map.

Steps 1 and 2 are useful alone: a grid you can move through is worth having even
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
| Vertical cost of a 3-row map | 12% of a 24-row terminal, 8% of a 40-row one |

The async result is the one that matters, because it converts "live dashboard"
into "map that updates when you touch it". Better to know that before building
than after.
