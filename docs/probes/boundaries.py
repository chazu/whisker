"""The grid segment where it meets everything else.

The other probes test features one at a time. This one covers the boundaries
between them, which is where the interesting failures live: it found one, where
a narrow row dropped the grid's reserved cells while the placement escape was
still emitted, so the image would have been drawn over text that was not
expecting it.

Run after `cargo build --release`.
"""
import os,pty,fcntl,termios,struct,select,time,subprocess,sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.path.join(ROOT, "target", "release", "whisker")
fails=[]
def check(n,c,d=""):
    print(("  ok   " if c else "  FAIL ")+n+(f"  {d}" if d else "")); 
    if not c: fails.append(n)

def pty_run(cfg, args, cols=80, cell=(16,34), env=None):
    pid,fd=pty.fork()
    if pid==0:
        os.execve(BIN,["whisker"]+args, dict(os.environ, WHISKER_CONFIG=cfg, **(env or {})))
    fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack("HHHH",24,cols,cols*cell[0],24*cell[1]))
    out=b""; end=time.time()+3
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.2)
        if not r: break
        try: c=os.read(fd,65536)
        except OSError: break
        if not c: break
        out+=c
    os.waitpid(pid,0); return out

def write(path, text):
    with open(path,"w") as h: h.write(text)

# 1. grid + colour: styling must not break the placement or the reservation
write("/tmp/b1.toml", '''
views = ["v"]
[grid]
rows = ["r"]
columns = ["c1","c2"]
state = "/tmp/b-state"
[view.v]
segments = ["grid","directory"]
style = { fg = "blue", bold = true }
at = ["r","c1"]
''')
write("/tmp/b-state","v ok\n")
out = pty_run("/tmp/b1.toml", ["render","--columns","80","--color","always"])
check("grid + colour: one placement", out.count(b"\x1b_G")==1)
tail = out.split(b"\x1b\\",1)[1] if b"\x1b\\" in out else out
check("grid + colour: row is styled", b"\x1b[" in tail)
check("grid + colour: no marker leaks", "\u00a0".encode() not in out, repr(out[-40:]))

# 2. NO_COLOR with a grid: the image is not colour, so it should still draw
out = pty_run("/tmp/b1.toml", ["render","--columns","80"], env={"NO_COLOR":"1"})
check("NO_COLOR: placement still emitted", out.count(b"\x1b_G")==1)
tail = out.split(b"\x1b\\",1)[1] if b"\x1b\\" in out else out
check("NO_COLOR: no SGR in the row", b"\x1b[" not in tail, repr(tail[:40]))

# 3. grid + atomic text segment together
write("/tmp/b2.toml", '''
views = ["v"]
[grid]
rows = ["r"]
columns = ["c1","c2"]
state = "/tmp/b-state"
[view.v]
segments = ["grid","strip","directory"]
at = ["r","c1"]
[segment.strip]
command = ["printf","%s","AAAAAAAAAAAAAAAAAAAA"]
atomic = true
fallback = ["printf","%s","<A>"]
''')
# The directory shrinks first, because trimming a path costs less than
# sacrificing a whole segment, so the swap happens later than a naive reading
# of the widths suggests. These thresholds were measured, not assumed.
# The directory shrinks first, because trimming a path costs less than
# sacrificing a whole segment. On a tty the grid also reserves a cell, so the
# strip gives up a little sooner than it would in a pipe. These thresholds
# were measured on the tty path, which is what this probe exercises.
for cols,expect in ((80,"AAAA"),(30,"AAAA"),(24,"<A>"),(16,"<A>")):
    out = pty_run("/tmp/b2.toml",["render","--columns",str(cols),"--color","never"],cols=cols)
    tail=(out.split(b"\x1b\\",1)[1] if b"\x1b\\" in out else out).decode(errors="replace")
    check(f"grid+atomic at {cols}: shows {expect}", expect in tail, repr(tail.strip()))
    # Whatever happens, a partial strip must never appear: that is the whole
    # point of atomic, and it is the failure the design document found.
    partial = "AAAA" in tail and "AAAAAAAAAAAAAAAAAAAA" not in tail
    check(f"grid+atomic at {cols}: never partial", not partial, repr(tail.strip()))

# 4. collect then render: the drawn state matches what collect wrote
write("/tmp/b3.toml", '''
views = ["v1","v2"]
[grid]
rows = ["r"]
columns = ["c1","c2"]
state = "/tmp/b3-state"
[view.v1]
segments = ["grid"]
at = ["r","c1"]
alert = { command = ["false"], when = "exit" }
[view.v2]
segments = ["grid"]
at = ["r","c2"]
alert = { command = ["true"], when = "exit" }
''')
subprocess.run([BIN,"collect"],env=dict(os.environ,WHISKER_CONFIG="/tmp/b3.toml"),capture_output=True)
grid = subprocess.run([BIN,"view","grid"],env=dict(os.environ,WHISKER_CONFIG="/tmp/b3.toml"),
                      capture_output=True,text=True).stdout.strip()
check("collect -> view grid agree", grid=="v1:alert v2:ok", repr(grid))
out = pty_run("/tmp/b3.toml",["render","--view","v1","--columns","80","--color","never"])
check("collect -> render draws it", out.count(b"\x1b_G")==1)

# 5. view move still works with a grid segment configured
mv = subprocess.run([BIN,"view","move","--direction","right","--current","v1"],
                    env=dict(os.environ,WHISKER_CONFIG="/tmp/b3.toml"),capture_output=True,text=True).stdout.strip()
check("view move with a grid segment", mv=="v2", repr(mv))

# 6. a flat config with no grid: the grid segment must be inert, not an error
write("/tmp/b4.toml",'views = ["v"]\n[view.v]\nsegments = ["grid","directory"]\n')
out = pty_run("/tmp/b4.toml",["render","--columns","80","--color","never"])
check("no [grid]: no placement", b"\x1b_G" not in out)
check("no [grid]: row still renders", b"whisker" in out, repr(out[:60]))

for f in ["/tmp/b1.toml","/tmp/b2.toml","/tmp/b3.toml","/tmp/b4.toml","/tmp/b-state","/tmp/b3-state"]:
    try: os.remove(f)
    except OSError: pass

# The regression this probe found: on a row too narrow for the grid, the
# reservation is dropped but the placement escape must go with it. Emitting one
# without the other draws the image over unsuspecting text and leaves the
# prompt's declared width wrong.
write("/tmp/b5.toml", '''
views = ["v"]
[grid]
rows = ["r"]
columns = ["c1","c2"]
state = "/tmp/b-state"
[view.v]
label = "[a-long-label-here] "
segments = ["grid","directory"]
at = ["r","c1"]
''')
write("/tmp/b-state","v ok\n")
for cols in (80, 40, 30, 24, 20, 16, 12, 8):
    out = pty_run("/tmp/b5.toml", ["render","--columns",str(cols),"--color","never"], cols=cols)
    tail = (out.split(b"\x1b\\",1)[1] if b"\x1b\\" in out else out).decode(errors="replace")
    drew = b"\x1b_G" in out
    label = "[a-long-label-here] "
    reserved = tail.startswith(label) and tail[len(label):].startswith(" ")
    check(f"narrow {cols}: escape and cells agree", drew == reserved,
          f"drew={drew} reserved={reserved} {tail.strip()!r}")
for f in ["/tmp/b5.toml","/tmp/b-state"]:
    try: os.remove(f)
    except OSError: pass

print("\nfailures:",len(fails))
sys.exit(1 if fails else 0)
