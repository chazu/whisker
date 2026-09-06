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
