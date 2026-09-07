# Whisker

A two-line Bash prompt whose information row switches instantly with Alt-O,
preserving the command being edited and its cursor. A small Rust renderer emits
the row; Bash owns the editing session. Views are configurable.

It began as a proof of concept; see [NOTES.md](NOTES.md) for how it developed.

Build and run from this directory:

```sh
./try-it
```

The launcher builds the Rust binary and opens a fresh Bash 5+ session. It keeps
your current directory and environment, and loads your Readline preferences.
It uses its own prompt/startup file and does not load your usual Bash aliases or
functions. Type `exit` to return to the original shell. No dotfiles are modified
and no command history is saved by this session.

Press **Alt-O** to cycle the configured views; by default **dev → ops →
minimal → dev**. If your macOS terminal uses Option for accented characters,
configure Option as Esc/Meta; pressing **Esc, then O (lowercase)** also sends
the binding.

| Default view | Information row |
| --- | --- |
| minimal | Current directory, with `~` for your home directory |
| dev | Directory and Git branch; `*` means staged, unstaged, or untracked changes |
| ops | Directory and selected Kubernetes context/namespace |

## Configuration

Views are configurable. Without a configuration file the three views above are
used, so nothing needs to be set up to start.

Write your own at `~/.config/whisker/config.toml` (or `$XDG_CONFIG_HOME/whisker/
config.toml`; `$WHISKER_CONFIG` overrides the path). Start from the defaults:

```sh
mkdir -p ~/.config/whisker
target/debug/whisker config example > ~/.config/whisker/config.toml
target/debug/whisker config check
```

`views` is the Alt-O cycle in order, and `start` is the view a new shell opens
in. Each `[view.name]` sets an optional `label`, an optional `separator`
(default two spaces), and the `segments` it shows.

```toml
views = ["cloud", "minimal"]
start = "cloud"

[view.minimal]
segments = ["directory"]

[view.cloud]
label = "» "
separator = " | "
segments = ["directory", "git", "region"]

[segment.region]
command = ["aws", "configure", "get", "region"]
prefix = "aws:"
```

The built-in segments are `directory`, `git`, and `kubernetes`. Any other
segment needs a `[segment.name]` with a `command`, which is an argv list run
directly with no shell involved, so no quoting or expansion applies. Its first
output line becomes the segment. A segment that is empty, fails, or names a
missing program disappears along with its separator, so an absent tool never
leaves a gap. Every segment accepts `prefix`, `suffix`, `shrink` (may be
shortened to fit the width), and `keep_end` (shorten as `…/tail` rather than
`head…`). The directory shrinks from its start by default; custom segments
shrink from their end.

### Segments that must not be truncated

Shortening is safe for a path, where `…/tail` still reads truthfully. It is not
safe for a value whose meaning depends on being complete: a status marker
clipped halfway still looks like a status marker while hiding whatever fell off
the end, and a display that hides a warning is worse than one that is absent,
because the user reads calm and believes it.

Such a segment can set `atomic = true`, which means show it whole or not at
all, and optionally a `fallback` argv to run when it does not fit:

```toml
[segment.status]
command = ["my-status", "--full"]
atomic = true
fallback = ["my-status", "--brief"]
```

A narrow terminal then shows the brief form, or nothing when there is no
fallback, rather than a convincing but incomplete one. `atomic` and `shrink`
contradict each other and cannot both be set, and a `fallback` without `atomic`
is rejected rather than silently ignored.

### Styling

Colour is structured configuration rather than text you embed, and it is applied
after the row is laid out. Widths are therefore always measured on plain text,
so an escape sequence is never counted as a column nor cut in half by
shortening.

```toml
views = ["dev"]

[view.dev]
label = "dev "
label_style = { fg = "green", bold = true }
separator_style = { fg = "bright_black" }
style = { dim = true }            # default for this view's segments
segments = ["directory", "git"]

[segment.git]
style = { fg = "red" }            # a segment's own style wins, field by field
```

`fg` and `bg` take one of the eight colour names (`red`, `green`, `yellow`,
`blue`, `magenta`, `cyan`, `black`, `white`), any of those with a `bright_`
prefix, a `0`-`255` palette index, or `#rrggbb`. Named colours follow your
terminal theme, which usually suits a prompt better than fixed values. The
attributes are `bold`, `dim`, `italic`, and `underline`. A segment's `style`
overrides the view's for the fields it sets; the rest are inherited.

`--color` takes `auto` (default), `always`, or `never`. Whisker's output is
captured into a shell variable rather than written to a terminal, so `auto`
cannot detect a TTY: it honours `NO_COLOR` and `TERM=dumb` and otherwise assumes
colour is wanted. Following the NO_COLOR standard, the variable counts only when
it is present and not empty, whatever its value. Every styled piece resets
afterwards, so nothing leaks into the command you type.

A configuration mistake is reported rather than ignored: an unknown key, a view
listed in `views` without a definition, or a `start` naming no view all fail
with a specific message. `whisker config check` reports the file in use, the
view cycle, and the start view, and `whisker config path` prints where it looks.
If the file is broken, the shell prints the error once at startup and falls back
to a plain prompt rather than failing on every redraw.

The second line is always `` `-=> ``. Git information is omitted outside a repo;
detached HEAD shows a short commit ID. Kubernetes uses `kubectl config view
--minify` with a narrow JSONPath extraction, respects `KUBECONFIG`, and defaults
an omitted namespace to `default`. A missing command or unreadable configuration
shows `⎈ unavailable`. It does not query the cluster.

Rust reads the configuration, collects each segment of the selected view, and
emits a plain text information row, shortening the longest shrinkable segment
first to keep the row within the terminal width. When nothing may shrink any
further it gives up whole `atomic` segments, widest first, before clipping the
row, so a segment that would mislead when truncated is never shown truncated.
Bash prints that row from its
prompt hook; PS1 itself is the stable input marker. This avoids Readline
caching an obsolete information row. Bash stores the selected view in memory
and refreshes before each prompt and on Alt-O. The inputrc macro forwards Alt-O
to a private sequence connected to a Bash function with `bind -x`. Ctrl-L
repaints both rows. The first Alt-O after resizing also performs a full repaint
to recover the row positions.

Try typing a command, moving the cursor into its middle, and switching views.
Also try long wrapped input, `cd` into a Git checkout, Ctrl-L, and terminal
resizing. These are real shell commands, so use commands you intend to run.

To inspect the renderer alone:

```sh
target/debug/whisker render --view dev --columns 80
target/debug/whisker render --view ops --columns 80 --color always
target/debug/whisker render --view ops --columns 80 --color never
target/debug/whisker view next --current dev
target/debug/whisker view move --direction right --current dev
target/debug/whisker view grid
target/debug/whisker view list
target/debug/whisker view start
target/debug/whisker config path
target/debug/whisker config check
```

`view list` and `view start` are what the shell layer uses, so it never hard
codes a view name.

### Arranging views on a grid

Views are a cycle by default: `view next` walks them in order. They can also be
given positions on a 2D grid, which is a way of navigating the views that
already exist rather than a second kind of thing. A node is a view with
coordinates.

```toml
[grid]
rows = ["code", "infra", "data"]
columns = ["ops", "staging", "prod"]

[view.infra_prod]
segments = ["directory", "kubernetes"]
at = ["infra", "prod"]
```

`view move --direction left|right|up|down --current NAME` then reports where
that step lands, and `view grid` prints the shape, one line per row, with `-`
for a cell no view claims.

Two behaviours are deliberate. Movement does not wrap: a grid is a map, and on
a map moving left at the left edge does nothing, where wrapping would teleport
you across the screen for a keypress that felt like a nudge. And undefined
cells are skipped rather than landed on, because sparse grids are normal and a
key that appears to do nothing is worse than one that moves further than
expected.

The grid is optional and additive. Without it, or for a view with no `at`,
`view move` returns the current view unchanged, so an existing configuration
behaves exactly as before.

### Node state

A grid can also show what each node is reporting. Collecting that inline would
not scale, since one Kubernetes check takes around 29 ms and a nine-node grid
would add a quarter of a second to every prompt before touching a network. So
collection is decoupled from display: something else writes a small state file
on its own schedule, and Whisker only reads it.

```toml
[grid]
rows = ["code", "infra"]
columns = ["ops", "prod"]
state = "/run/user/1000/whisker-nodes"
```

The file is one line per view, `NAME STATUS [anything else]`, where status is
`ok`, `alert`, or `unknown`:

```text
infra_prod alert 2026-09-06T22:10:05 3 warning events
infra_ops  ok
```

Trailing words are ignored, so a collector can record a timestamp and a reason
in the same line for a human to read. `view grid` then reports each cell as
`name:status`, with `-` for a cell no view claims.

Anything on the machine can write this file, and a collector may be halfway
through rewriting it when the prompt reads, so nothing about reading it can
fail. A missing file, a malformed line, an unknown view, or an unrecognised
status word all leave that node `unknown` rather than raising an error: a
prompt that refuses to draw is worse than one that admits it does not know.
Only the status word is used, and it is matched against a fixed set, so hostile
content cannot reach the row.

Note the freshness limit this implies. Bash defers signal handlers while
Readline waits for a keystroke, so nothing can repaint the prompt on its own.
The grid is as fresh as your last keystroke, not as fresh as the world.

Known limits. Collectors are synchronous and local, so a slow Git repository or
custom command delays a redraw; there are no timeouts. Styling is static: it
cannot yet depend on state, so "red when the branch is dirty" cannot be
expressed.
Control characters are still stripped from all collected text and from labels,
so metadata can never move the cursor; colour arrives only through the `style`
tables. There is no daemon and no general integration with existing prompt
hooks. The one-row repaint assumes a conventional ANSI terminal.
Full multi-command input (PS2), extreme resizing, and commands taller than the
terminal need further work before daily use.

Beyond `cargo test`, [`checks/`](checks/README.md) drives the prompt through a
real pseudo-terminal and screen emulator to confirm that a styled row occupies
the columns it claims and that editing survives view switches and resizing.

See [NOTES.md](NOTES.md) for the development observations, and
[docs/grid-design.md](docs/grid-design.md) for a proposal, not yet built, to
arrange views on a navigable 2D grid with alert markers.
