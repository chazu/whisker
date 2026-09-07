"""Wrapped input is the real threat: the grid sits N rows above the FIRST input
row, but bind -x returns the cursor to a multi-row input block."""
import fcntl, os, pty, time
import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
pid, fd = pty.fork()
if pid == 0:
    os.environ["COLUMNS"]="40"
    os.execv("/opt/homebrew/bin/bash", ["bash","--noprofile","--rcfile",HERE+"/multirow.bash","-i"])
fcntl.fcntl(fd, fcntl.F_SETFL, os.O_NONBLOCK)
raw=bytearray()
def pump(t=1.2):
    end=time.time()+t
    while time.time()<end:
        try:
            c=os.read(fd,65536)
            if c: raw.extend(c)
            else: time.sleep(0.05)
        except BlockingIOError: time.sleep(0.05)
        except OSError as e:
            if e.errno in (11,35): time.sleep(0.05)
            else: break
pump(3)
# 3 screen-rows of wrapped input at 40 columns.
os.write(fd, b"echo " + b"x"*100); pump(1.5)
s=pyte.Screen(40,14); pyte.ByteStream(s).feed(bytes(raw))
print("=== before Alt-O (input wraps over several rows) ===")
for i,l in enumerate(s.display):
    if l.strip(): print(f"  {i}: {l.rstrip()[:40]}")
os.write(fd, b"\x1bo"); pump(1.5)
s2=pyte.Screen(40,14); pyte.ByteStream(s2).feed(bytes(raw))
print("\n=== after Alt-O with wrapped input ===")
for i,l in enumerate(s2.display):
    if l.strip(): print(f"  {i}: {l.rstrip()[:40]}")
print("\ncursor:", s2.cursor.y, s2.cursor.x)
os.write(fd,b"\r"); pump(1.5)
s3=pyte.Screen(40,14); pyte.ByteStream(s3).feed(bytes(raw))
out="".join(l for l in s3.display)
print("command ran correctly:", "x"*100 in out.replace(" ",""))
os.write(fd,b"exit\r"); pump(1)
