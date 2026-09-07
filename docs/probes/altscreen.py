"""Design C probe: alternate-screen overlay.

pyte does NOT implement mode 1049, so this shows the overlay leaking onto the
main screen. That is the emulator's gap, not a result about real terminals.
Run the shell fragment by hand in a real terminal to learn anything useful.
"""
import fcntl, os, pty, time
import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
pid, fd = pty.fork()
if pid == 0:
    os.execv("/opt/homebrew/bin/bash", ["bash","--noprofile","--rcfile",HERE+"/altscreen.bash","-i"])
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
def show(tag):
    s=pyte.Screen(50,10); pyte.ByteStream(s).feed(bytes(raw))
    print(f"=== {tag} ===")
    for l in s.display:
        if l.strip(): print("  ", l.rstrip())
pump(3)
os.write(fd, b"echo preserved"); pump(1)
show("before overlay")
os.write(fd, b"\x1bo"); pump(2.0)
show("after overlay returns")
os.write(fd, b"\r"); pump(1)
s=pyte.Screen(50,10); pyte.ByteStream(s).feed(bytes(raw))
print("\ncommand executed:", "preserved" in "".join(s.display))
os.write(fd,b"exit\r"); pump(1)
