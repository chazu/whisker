"""Can the grid be drawn as a picture (sixel or the kitty protocol)?

Two questions, only one of which a screen emulator can answer:

1. Does Whisker pass a graphics escape through at all? Answerable here, and the
   answer is no: `clean` replaces every control character with a space, which is
   the defence that makes an untrusted state file safe. A picture segment cannot
   exist without weakening it.

2. Does a picture inside PS1 keep Readline's column arithmetic honest? Not
   answerable here. pyte does not implement sixel, and a glyph's pixel size is
   exactly the thing an emulator with no pixels cannot model. Run
   `graphics.bash` in a real terminal for that.

What this probe *can* show is the shape of the accounting problem, by standing a
zero-width-but-visible thing in for the image.
"""

import os
import pty
import select
import subprocess
import time
from pathlib import Path

import pyte

ROOT = Path(__file__).resolve().parents[2]
WHISKER = ROOT / "target" / "release" / "whisker"
BASH = "/opt/homebrew/bin/bash"

# A minimal one-pixel-ish sixel: DCS q ... ST
SIXEL = "\033Pq#0;2;100;0;0#0~~@@vv@@~~$-#0??}}GG}}??-\033\\"


def whisker_passes_graphics_through() -> str:
    """Question 1, against the real binary."""
    config = ROOT / "docs" / "probes" / "configs" / "graphics.toml"
    out = subprocess.run(
        [str(WHISKER), "render", "--columns", "80", "--color", "never"],
        env={**os.environ, "WHISKER_CONFIG": str(config)},
        capture_output=True,
        text=True,
    ).stdout
    return out


def readline_accounting() -> str:
    """Question 2's shape: something visible that Readline is told is invisible.

    A graphics escape has to sit inside \\[ \\] because Readline must not count
    its bytes as columns. But \\[ \\] means "this takes no space at all", which
    is a lie for a picture. This drives a real Bash with a prompt that makes the
    same claim and shows what the lie costs.
    """
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(BASH, [BASH, "--norc", "--noprofile", "-i"])

    screen = pyte.Screen(40, 6)
    stream = pyte.ByteStream(screen)

    def pump(seconds=0.6):
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try:
                    stream.feed(os.read(fd, 65536))
                except OSError:
                    return

    pump()
    # PS1 claims the ten X's are zero-width, the way a sixel would have to.
    os.write(fd, b"PS1='\\[XXXXXXXXXX\\]$ '\n")
    pump()
    os.write(fd, b"echo hello-there-this-is-a-long-command")
    pump()
    typed = [screen.display[i].rstrip() for i in range(screen.lines)]
    # Ctrl-A: Readline recomputes the cursor from its own idea of the prompt
    # width. That is where a wrong width stops being cosmetic.
    os.write(fd, b"\001")
    pump()
    home = [screen.display[i].rstrip() for i in range(screen.lines)]
    cursor = (screen.cursor.x, screen.cursor.y)

    lines = (
        ["   after typing:"]
        + ["   " + l for l in typed if l]
        + ["   after Ctrl-A (cursor to start of line):"]
        + ["   " + l for l in home if l]
        + [f"   cursor landed at column {cursor[0]}, row {cursor[1]}"]
        + [
            "   The command's first character is at column 12. Readline put the"
            f" cursor at {cursor[0]}, off by {12 - cursor[0]}: the width it was"
            " told to ignore, plus the two-column '$ '."
        ]
    )
    os.write(fd, b"\003")
    os.close(fd)
    os.waitpid(pid, 0)
    return "\n".join(l for l in lines if l)


def main():
    print("1. Graphics escape through the real binary")
    print("   rendered:", repr(whisker_passes_graphics_through()))
    print("   The DCS introducer and ST are gone, replaced by spaces, so the")
    print("   terminal sees payload text rather than an image. `clean` did it.")
    print()
    print("2. Readline accounting when a prompt lies about its width")
    if not Path(BASH).exists():
        print("   skipped: no Bash 5 at", BASH)
        return
    print(readline_accounting())
    print()
    print("   A picture in the prompt has this problem by construction: it is")
    print("   visible but must be declared invisible. See graphics.bash for the")
    print("   part only a real terminal can answer.")


if __name__ == "__main__":
    main()
