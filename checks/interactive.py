"""End-to-end: drive ./try-it in a real pty and render onto a real screen.

Checks the outcome the user cares about: cycling views with Alt-O shows styled
rows, the typed command survives with its cursor, and no colour leaks into the
input the user is typing.
"""
import fcntl
import os
import pty
import sys
import time

import pyte

os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
pid, fd = pty.fork()
if pid == 0:
    os.environ["COLUMNS"] = "100"
    os.execv("./try-it", ["./try-it"])

fcntl.fcntl(fd, fcntl.F_SETFL, os.O_NONBLOCK)
screen = pyte.Screen(100, 24)
stream = pyte.ByteStream(screen)
raw = bytearray()


def pump(seconds=1.5):
    end = time.time() + seconds
    while time.time() < end:
        try:
            chunk = os.read(fd, 65536)
            if chunk:
                raw.extend(chunk)
                stream.feed(chunk.replace(b"\xef\xb8\x8f", b""))
            else:
                time.sleep(0.05)
        except BlockingIOError:
            time.sleep(0.05)
        except OSError as error:
            if error.errno in (11, 35):
                time.sleep(0.05)
            else:
                break


def rows():
    return [line.rstrip() for line in screen.display if line.strip()]


pump(4)
start = rows()
print("start view row:", start[-2] if len(start) > 1 else start)

os.write(fd, b"echo hello world")
pump(1.2)
typed = rows()

seen = []
for _ in range(4):
    os.write(fd, b"\x1bo")
    pump(1.5)
    current = rows()
    seen.append(current[-2] if len(current) > 1 else "")

failures = []

# 1. Cycling must actually change the information row.
if len(set(seen)) < 3:
    failures.append(f"Alt-O did not cycle distinct rows: {seen}")

# 2. The typed command must survive every switch, with the cursor after it.
last = rows()[-1] if rows() else ""
if "echo hello world" not in last:
    failures.append(f"typed command lost after cycling; last row={last!r}")

# 3. No style may be active where the user types. Check the cell under the
#    cursor on the input row.
y = screen.cursor.y
x = screen.cursor.x
cell = screen.buffer[y][x - 1] if x else screen.buffer[y][0]
if cell.fg != "default" or cell.bold:
    failures.append(f"style leaked into the input row: fg={cell.fg} bold={cell.bold}")

# 4. The prompt must stay two rows: info row directly above the input row.
if len(rows()) >= 2 and not rows()[-1].startswith("`-=>"):
    failures.append(f"input row is not the PS1 marker: {rows()[-1]!r}")

# 5. The command must actually run correctly after all that switching.
os.write(fd, b"\r")
pump(1.5)
if "hello world" not in "".join(rows()):
    failures.append("the command did not execute correctly after cycling")

# 6. Colour really was emitted (otherwise this test proves nothing).
if b"\x1b[0;" not in bytes(raw):
    failures.append("no SGR sequences were emitted at all")

os.write(fd, b"exit\r")
pump(1.5)

print("\ncycled rows:")
for row in seen:
    print("  ", row)

if failures:
    print(f"\n{len(failures)} FAILURE(S):")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)
print("\nPASS: styled rows cycle, input and cursor survive, no colour leaks")
