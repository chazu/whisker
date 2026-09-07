"""Ctrl-L, resize and scrollback with an image-bearing prompt.

Driven through a real Bash in a pty. pyte cannot draw the image, but it CAN
answer the structural questions, which are the ones that matter:
  - does Ctrl-L reissue the prompt (and therefore the placement) intact?
  - after a resize, does the prompt still occupy the columns it declares?
  - does the typed command stay uncorrupted through both?
The placement bytes are opaque to pyte, so we stand in an ESC7/ESC8 pair that
draws N cells and restores the cursor, which is behaviourally what a kitty
placement with C=1 does.
"""
import os, pty, select, time, sys, struct, fcntl, termios, tempfile
import pyte

BASH = "/opt/homebrew/bin/bash"
RC = os.path.join(tempfile.mkdtemp(prefix="whisker-probe-"), "rc.bash")
open(RC, "w").write(
    # draws 2 visible cells, restores cursor, then 2 countable spaces
    "draw=$'\\e7\\e[7m##\\e[0m\\e8'\n"
    'PS1="\\[${draw}\\]  \\$ "\n'
)

pid, fd = pty.fork()
if pid == 0:
    os.execvp(BASH, [BASH, "--rcfile", RC, "-i"])

cols, rows = 40, 8
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, cols*16, rows*34))
screen = pyte.Screen(cols, rows)
stream = pyte.ByteStream(screen)

def pump(sec=0.7):
    end = time.time() + sec
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try: stream.feed(os.read(fd, 65536))
            except OSError: return

def rows_now():
    return [screen.display[i].rstrip() for i in range(screen.lines)
            if screen.display[i].strip()]

fails = 0
def check(name, cond, detail=""):
    global fails
    print(("  ok   " if cond else "  FAIL ") + name + (f"  {detail}" if detail else ""))
    if not cond: fails += 1

pump()
os.write(fd, b"echo hello-world")
pump()
before = rows_now()[-1]
check("command typed after image prompt", "echo hello-world" in before, repr(before))
col_before = screen.cursor.x

# --- Ctrl-L ---
os.write(fd, b"\x0c")
pump()
after = rows_now()
last = after[-1] if after else ""
check("Ctrl-L preserves the typed command", "echo hello-world" in last, repr(last))
check("Ctrl-L redraws prompt at top", len(after) == 1, f"{len(after)} rows")
check("Ctrl-L keeps cursor column", screen.cursor.x == col_before,
      f"{screen.cursor.x} vs {col_before}")

# --- resize ---
cols2 = 60
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols2, cols2*16, rows*34))
screen.resize(rows, cols2)
os.write(fd, b"\x0c")   # force a repaint the way a real user would
pump()
after = rows_now()
last = after[-1] if after else ""
check("resize preserves the command", "echo hello-world" in last, repr(last))
check("prompt still starts the line after resize", last.endswith("echo hello-world"))
# the command must begin at the declared prompt width (2 image cells + "$ ")
check("command begins at declared prompt width",
      last.index("echo") == 4, f"index {last.index('echo')}")

# --- scrollback: run enough output to scroll the image off ---
os.write(fd, b"\x01\x0b")           # clear the line
os.write(fd, b"seq 1 20\n")
pump(1.0)
after = rows_now()
check("prompt survives scrolling output", any("$" in r for r in after),
      repr(after[-1] if after else ""))
os.write(fd, b"echo second-command")
pump()
last = rows_now()[-1]
check("second command types cleanly after scroll",
      "echo second-command" in last, repr(last))

os.write(fd, b"\x03")
os.close(fd); os.waitpid(pid, 0)
print("failures:", fails)
sys.exit(1 if fails else 0)
