import fcntl, os, pty, signal, struct, termios, time
import pyte

HERE = os.path.dirname(os.path.abspath(__file__))
def run(label, action, settle=3.0):
    pid, fd = pty.fork()
    if pid == 0:
        os.execv("/opt/homebrew/bin/bash", ["bash","--noprofile","--rcfile",HERE+"/async.bash","-i"])
    fcntl.fcntl(fd, fcntl.F_SETFL, os.O_NONBLOCK)
    raw=bytearray()
    def pump(t):
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
    pump(settle)                      # let the shell fully settle
    os.write(fd, b"echo ab"); pump(1.5)
    os.kill(pid, signal.SIGUSR1); pump(1.5)
    mid = bytes(raw)
    action(pid, fd); pump(2.0)
    s=pyte.Screen(60,10); pyte.ByteStream(s).feed(bytes(raw))
    row=[l.rstrip() for l in s.display if l.strip()][:1]
    sm=pyte.Screen(60,10); pyte.ByteStream(sm).feed(mid)
    rowm=[l.rstrip() for l in sm.display if l.strip()][:1]
    print(f"  {label:<24} before={rowm} after={row}")
    try: os.write(fd,b"\r"); pump(0.6); os.write(fd,b"exit\r"); pump(0.6)
    except Exception: pass
run("nothing",           lambda p,f: None)
run("SIGWINCH",          lambda p,f: fcntl.ioctl(f, termios.TIOCSWINSZ, struct.pack("HHHH",11,58,0,0)))
run("keystroke",         lambda p,f: os.write(f,b"c"))
