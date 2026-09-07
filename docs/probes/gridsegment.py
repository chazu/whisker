"""The grid segment, through the real binary, on a pty that reports a cell size.

This is the one probe that exercises design D as a feature rather than as a
prototype: it runs `whisker render` with a `grid` segment, decodes the PNG the
renderer actually emitted, and checks that what was drawn matches the
configured state.

A pty is used because the cell size comes from TIOCGWINSZ on the controlling
terminal, and because Whisker's stdout is a pipe in normal use. Setting the
window size *with* pixel dimensions is what makes the ioctl useful; a terminal
that reports zeroes there is the fallback case, covered separately below.
"""
import os, pty, fcntl, termios, struct, select, time, base64, io, re, sys, subprocess
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.path.join(ROOT, "target", "release", "whisker")
CONFIG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "configs", "grid-segment.toml")
STATE = "/tmp/whisker-probe-grid-state"



def run(extra, cell=(16,34)):
    pid, fd = pty.fork()
    if pid == 0:
        env = dict(os.environ, WHISKER_CONFIG=CONFIG, **extra)
        os.execve(BIN, ["whisker","render","--columns","80","--color","never"], env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH",24,80,80*cell[0],24*cell[1]))
    out=b""; end=time.time()+3
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.2)
        if not r: break
        try: c=os.read(fd,65536)
        except OSError: break
        if not c: break
        out+=c
    os.waitpid(pid,0); return out

# The states the drawing is checked against.
with open(STATE, "w") as handle:
    handle.write("a1 ok\na2 alert\na3 ok\nb3 alert\n")

fails=[]
def check(name,cond,detail=""):
    print(("  ok   " if cond else "  FAIL ")+name+(f"  {detail}" if detail else ""))
    if not cond: fails.append(name)

for label, extra, expect_art in [("scale=1",{},(14,9)), ("scale=2",{"WHISKER_SCALE":"2"},(28,18))]:
    out = run(extra)
    m = re.search(rb"\x1b_G([^;]*);(.*?)\x1b\\", out, re.S)
    check(f"{label}: emits one placement", m is not None)
    if not m: continue
    meta, payload = m.group(1).decode(), m.group(2)
    check(f"{label}: declares exact cells", "c=1,r=1" in meta, meta)
    check(f"{label}: pins the cursor", "C=1" in meta)
    check(f"{label}: is quiet", "q=2" in meta)
    png = base64.b64decode(payload)
    im = Image.open(io.BytesIO(png)); im.load()
    check(f"{label}: image is one cell", im.size==(16*(2 if extra else 1), 34*(2 if extra else 1)), str(im.size))
    # Count coloured dots: 5 views placed, one of which (b1) is current/white
    colours = {}
    for px in im.getdata():
        if px[3]: colours[px[:3]] = colours.get(px[:3],0)+1
    check(f"{label}: current node is white", (240,240,240) in colours)
    check(f"{label}: two alerts are red", (230,70,60) in colours)
    check(f"{label}: two ok nodes are green", (90,140,90) in colours)
    # b2 is an undefined cell and must not be drawn: 5 nodes, not 6
    dot_px = sum(colours.values())
    per = (4*(2 if extra else 1))**2
    check(f"{label}: exactly 5 nodes drawn", dot_px==5*per, f"{dot_px}px / {per}px per dot")
    # the row: countable spaces equal to the declared cells
    tail = out.split(b"\x1b\\")[-1].decode(errors="replace")
    check(f"{label}: row reserves 1 space for the image", tail.startswith(" ") and not tail.startswith("  ~"), repr(tail[:6]))

# Without a cell size there is nothing to size the image to, so the segment
# must disappear cleanly rather than guess. This is what every terminal
# lacking the protocol gets, and it is the more common path in practice.
plain = subprocess.run(
    [BIN, "render", "--columns", "80", "--color", "never"],
    capture_output=True, text=True, cwd=ROOT,
    env=dict(os.environ, WHISKER_CONFIG=CONFIG),
).stdout
check("no cell size: no placement is emitted", "\x1b_G" not in plain, repr(plain))
check("no cell size: the row is plain text", plain.strip() != "" and "\x1b" not in plain,
      repr(plain))
# A terminal that reports zero pixel dimensions is the iTerm2 case: it is a
# terminal, it may even support the protocol, but it will not say how big a
# cell is, so the image cannot be sized to whole cells. That must fall back
# too rather than divide by zero or guess.
zeroed = run({}, cell=(0, 0))
check("zero-pixel terminal: no placement", b"\x1b_G" not in zeroed, repr(zeroed[:40]))
check("zero-pixel terminal: the row still renders", b"whisker" in zeroed, repr(zeroed[:60]))

os.remove(STATE)
print("\nfailures:", len(fails))
sys.exit(1 if fails else 0)
