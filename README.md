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

Press **Alt-O** to cycle the configured views; by default **dev → ops → minimal
→ dev**. If your macOS terminal uses Option for accented characters, configure
Option as Esc/Meta; pressing **Esc, then O (lowercase)** also sends the binding.

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
first to keep the row within the terminal width. Bash prints that row from its
prompt hook; PS1 itself is the stable input marker. This avoids Readline caching an obsolete
information row. Bash stores the selected view in memory and refreshes before
each prompt and on Alt-O. The inputrc macro forwards Alt-O to a private sequence
connected to a Bash function with `bind -x`. Ctrl-L repaints both rows. The first
Alt-O after resizing also performs a full repaint to recover the row positions.

Try typing a command, moving the cursor into its middle, and switching views.
Also try long wrapped input, `cd` into a Git checkout, Ctrl-L, and terminal
resizing. These are real shell commands, so use commands you intend to run.

To inspect the renderer alone:

```sh
target/debug/whisker render --view dev --columns 80
target/debug/whisker render --view ops --columns 80
target/debug/whisker view next --current dev
target/debug/whisker view list
target/debug/whisker config check
```

Known limits. Collectors are synchronous and local, so a slow Git repository or
custom command delays a redraw; there are no timeouts. Text is emitted without
colour: any icon or symbol works in a `label`, `prefix`, or `separator`, but
ANSI colour does not, since control characters are stripped so metadata can
never move the cursor. There is no daemon and no general integration with
existing prompt hooks. The one-row repaint assumes a conventional ANSI terminal.
Full multi-command input (PS2), extreme resizing, and commands taller than the
terminal need further work before daily use.

See [NOTES.md](NOTES.md) for the development observations.
