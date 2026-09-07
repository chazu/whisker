"""A background writer changes state; a keybind picks it up without Enter."""
import fcntl, os, pty, time
import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
open("/tmp/whisker_grid_state","w").write("[ o ][ o ][ o ]  quiet")
pid, fd = pty.fork()
if pid == 0:
    os.execv("/opt/homebrew/bin/bash", ["bash","--noprofile","--rcfile",HERE+"/statefile.bash","-i"])
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
def top():
    s=pyte.Screen(60,10); pyte.ByteStream(s).feed(bytes(raw))
    return [l.rstrip() for l in s.display if l.strip()][:2]
pump(3)
os.write(fd, b"echo typing"); pump(1)
print("baseline:                  ", top())
# A background agent raises an alert while the user is mid-typing.
open("/tmp/whisker_grid_state","w").write("[ o ][!!!][ o ]  ALERT node 2")
pump(0.5)
print("after background write:    ", top())
os.write(fd, b"\x1bo"); pump(1.5)     # user presses the refresh keybind
print("after refresh keybind:     ", top())
os.write(fd, b"\r"); pump(1); os.write(fd,b"exit\r"); pump(1)
