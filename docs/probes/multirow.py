import fcntl, os, pty, sys, time
import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
pid, fd = pty.fork()
if pid == 0:
    os.execv("/opt/homebrew/bin/bash", ["bash","--noprofile","--rcfile",HERE+"/multirow.bash","-i"])
fcntl.fcntl(fd, fcntl.F_SETFL, os.O_NONBLOCK)
raw=bytearray()
def pump(t=1.5):
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
def screen():
    s=pyte.Screen(60,20); pyte.ByteStream(s).feed(bytes(raw))
    return [l.rstrip() for l in s.display if l.strip()], s
pump(3)
os.write(fd, b"echo preserved"); pump(1)
before,_=screen()
print("BEFORE Alt-O:"); [print("   ",l) for l in before[-5:]]
for i in range(3):
    os.write(fd, b"\x1bo"); pump(1.2)
after,s=screen()
print("\nAFTER 3x Alt-O:"); [print("   ",l) for l in after[-5:]]
print("\ncursor:", s.cursor.y, s.cursor.x)
os.write(fd, b"\r"); pump(1); os.write(fd,b"exit\r"); pump(1)
fin,_=screen()
print("\nFINAL:"); [print("   ",l) for l in fin[-4:]]
