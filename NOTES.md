# Prototype observations

Question: can Alt-O cycle real prompt information while preserving Bash's
current command and cursor?

The initial shell-only experiment in Bash 5.3 showed that updating PS1 alone
leaves the information row stale. Repainting that one row inside a `bind -x`
callback, then allowing Readline to redraw its input, worked with empty, partial,
wrapped, and Unicode input. An insertion after switching executed at the original
cursor position. A simple terminal narrowing also retained the expected layout.

The complete experiment exposed two additional details:

- Embedding a literal backtick in an expanding PS1 interfered with expansion.
- Even when PS1 changed, Ctrl-L restored Readline's cached information prefix.
  Bash 5.3's `bash_execute_unix_command` restores the current Readline display
  after `bind -x`; it does not reevaluate the whole shell prompt. See the
  [upstream Bash 5.3 source](https://ftp.gnu.org/gnu/bash/bash-5.3.tar.gz).

The working approach prints the information row in PROMPT_COMMAND and keeps
PS1 as the fixed input marker. Alt-O repaints the row above the input. A custom
Ctrl-L handler clears the display, prints the current info row, and lets Readline
restore the input. The first Alt-O after a width change uses that full repaint
too, avoiding guesses about terminal reflow. Metadata is printed as data using
printf; it is never evaluated as shell or prompt syntax.

Verified on 2026-09-05 with the installed Bash 5.3.15, an actual pseudo-terminal,
and a terminal screen emulator:

- `./try-it` builds and launches the complete Rust/Bash/inputrc combination.
- A scratch Git repository shows `main*`; a scratch kubeconfig shows
  `staging:payments` using the installed kubectl and an unreachable cluster URL.
- All three views cycle with empty, partly typed, wrapped, and Unicode input
  while retaining the cursor. Editing in the middle after cycling executes the
  expected command (`abcdXef`).
- Ctrl-L retains the current view.
- Narrowing from 100 to 45 columns, then cycling, restores the two-row prompt
  and retains a wrapped 125-character command; subsequent cycling works.
- `exit` returns successfully. Cargo build, formatting, Clippy with warnings
  denied, and Bash syntax checks pass.

The installed kubeconfig was also read successfully through the Rust renderer.
One local timing sample was about 7 ms for minimal, 17 ms for dev outside a Git
repository, and 199 ms for ops. These are observations, not performance bounds;
collectors are synchronous and have no timeout in this proof of concept.

The user tried the prototype on 2026-09-05 and reported "its awesome", accepting
the initial interaction. This does not establish complete coverage of completion,
history navigation, or terminal behavior. General prompt-hook composition, PS2
editing, and input taller than the screen remain outside this experiment. The
working approach is ready to inform the next iteration of the utility.

## Configurable views (2026-09-06)

The prototype's `View` enum, its fixed cycle, and its hard-coded `layout` were
replaced by a TOML configuration read from `$WHISKER_CONFIG`, else
`$XDG_CONFIG_HOME/whisker/config.toml`, else `~/.config/whisker/config.toml`.
The built-in defaults are the prototype's own three views, so an existing setup
is unchanged and no file is required.

Views became a list of named segments with a label and separator. Segments are
`directory`, `git`, `kubernetes`, or a user `command` given as argv and run
directly, never through a shell, keeping the original guarantee that metadata is
data. An empty or failing segment drops its separator too, so an absent tool
leaves no gap. Shrinking generalised from "shorten the directory" to "shorten
the longest shrinkable segment until the row fits", which preserves the previous
behaviour for the default views.

A file that exists but does not parse is an error rather than a silent fallback,
so a typo is visible. The shell reports it once at startup and continues with a
plain prompt instead of failing on every redraw. `whisker config check` names
the file in use, the cycle, and the start view; `view start` and `view list` let
the shell layer stop hard-coding `dev` and the banner's view names.

Verified: nine unit tests cover the default cycle and layout, empty segments,
shrinking priority, narrow-terminal capping, control-character stripping, a
custom view with a custom command segment, a failing command segment, and ten
configuration mistakes. Cargo build, formatting, Clippy with warnings denied,
and `bash -n` pass. In a real pseudo-terminal, `./try-it` cycled the default
views and a custom two-view config with a custom `hostname` segment while
preserving partly typed input, and a deliberately broken config produced one
error and a working plain shell.

A first real configuration exposed one flaw: a segment's prefix was baked into
the collected text, so shortening ate the decoration and `📁 ~/long/path` became
`…/path` with the icon gone. Prefixes and suffixes now apply during layout,
leaving shortening to consume only the body. A test pins this.

## Static styling (2026-09-06)

Styling is structured configuration, never text the user embeds, and it is
applied after layout rather than during collection. That ordering is the whole
design. Measuring happens on plain text, so an escape sequence can never be
counted as a column, and shortening can never cut one in half. Control
characters are still stripped from every collected value and label, so the
original "metadata is data" guarantee holds: colour arrives only through a
`style` table, which cannot express cursor movement.

`fg`/`bg` accept the eight names with an optional `bright_` prefix, a 0-255
index, or `#rrggbb`. Named colours map to 30-37 and 90-97 so they follow the
terminal's theme. Attributes are bold, dim, italic, underline. A view sets a
default `style` for its segments plus `label_style` and `separator_style`; a
segment's own style overrides per field. Each painted piece opens with `0;` and
closes with a reset, so a style cannot leak in from earlier output nor out into
the typed command.

Two cases needed care. When nothing may shrink the row is capped, which clips
the plain text and then applies one uniform style, since clipping styled text
could sever a sequence. And `--color` cannot autodetect: the row is captured
into a shell variable, so a TTY check would always say no. Auto therefore
honours `NO_COLOR` and `TERM=dumb` and otherwise assumes a terminal.

Verified: 21 tests, including the invariant that stripping SGR from a styled row
reproduces the unstyled row exactly across widths from 80 down to 3, that no row
ends mid-escape, and that an unstyled config emits no escapes even with colour
on. Checked externally across five views and nine widths, plus NO_COLOR,
TERM=dumb, and unset TERM. A real pseudo-terminal showed the colours cycling
with Alt-O while a partly typed command and its cursor survived.

The styling claims were then checked against a terminal rather than against
strings, since only the terminal interprets the escape sequences. `checks/`
renders every view at nine widths onto a screen emulator and asserts that the
visible characters, the column count, and the one-row height are identical with
colour on and off, and that no style is still active where the user types. It
also drives `./try-it` through a pseudo-terminal to cycle views with a partly
typed command, and narrows the terminal mid-session.

Two findings. The emulator, not Whisker, drops the rest of a line after a U+FE0F
variation selector, identically with colour on or off; the checks strip it and
say so. And the checks earn their keep: reordering the code to style before
measuring, so escapes are counted as columns, makes both `checks/screen.py` and
the `style_never_changes_the_laid_out_text` test fail, which was confirmed by
deliberately introducing that regression and then reverting it.

Re-checking the styling work against its own claims found one real defect. The
NO_COLOR standard counts the variable only when present and *not empty*,
whatever its value; the first implementation disabled colour for an empty
`NO_COLOR=` as well. Fixed, with a test covering unset, `1`, `0`, `false`, and
empty, alongside `TERM=dumb`, an empty `TERM`, and an unset one.

The same pass confirmed all 22 documented colour forms by reading back the
colour a terminal actually applied to each cell, rather than by matching the
escape bytes, and confirmed that wide CJK, multi-codepoint emoji, and combining
accents shrink to a single row at widths from 80 down to 2 with colour on and
off alike.
