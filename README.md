# Whisker — proof of concept

Can a small Rust renderer provide a two-line Bash prompt whose information row
changes immediately with Alt-O, preserving the command being edited and its
cursor? This is a disposable experiment to answer that question.

Run from this directory:

```sh
./try-it
```

The launcher builds the Rust binary and opens a fresh Bash 5+ session. It keeps
your current directory and environment, and loads your Readline preferences.
It uses its own prompt/startup file and does not load your usual Bash aliases or
functions. Type `exit` to return to the original shell. No dotfiles are modified
and no command history is saved by this session.

Press **Alt-O** to cycle **dev → ops → minimal → dev**. If your macOS terminal
uses Option for accented characters, configure Option as Esc/Meta; pressing
**Esc, then O (lowercase)** also sends the binding.

| View | Information row |
| --- | --- |
| minimal | Current directory, with `~` for your home directory |
| dev | Directory and Git branch; `*` means staged, unstaged, or untracked changes |
| ops | Directory and selected Kubernetes context/namespace |

The second line is always `` `-=> ``. Git information is omitted outside a repo;
detached HEAD shows a short commit ID. Kubernetes uses `kubectl config view
--minify` with a narrow JSONPath extraction, respects `KUBECONFIG`, and defaults
an omitted namespace to `default`. A missing command or unreadable configuration
shows `⎈ unavailable`. It does not query the cluster.

Rust emits a plain text information row, shortening the directory first to keep
the row within the terminal width. Bash prints that row from its prompt hook;
PS1 itself is the stable input marker. This avoids Readline caching an obsolete
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
```

This prototype has fixed views, plain styling, and synchronous local collectors;
slow Git repositories can delay a redraw. It has no daemon, configuration format,
or general integration with existing prompt hooks. The one-row repaint assumes
a conventional ANSI terminal. Full multi-command input (PS2), extreme resizing,
and commands taller than the terminal need further work before daily use.

See [NOTES.md](NOTES.md) for the experiment's observations.
