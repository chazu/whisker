"""Resize the pty mid-session, then cycle. Styled rows must survive the reflow."""
import fcntl, os, pty, struct, sys, termios, time
import pyte
os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
pid, fd = pty.fork()
if pid == 0:
    os.execv("./try-it", ["./try-it"])
fcntl.fcntl(fd, fcntl.F_SETFL, os.O_NONBLOCK)
raw = bytearray()
def resize(cols, rows_n=24):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows_n, cols, 0, 0))
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
def render(cols):
    s=pyte.Screen(cols,24); st=pyte.ByteStream(s)
    st.feed(bytes(raw).replace(b"\xef\xb8\x8f", b""))
    return [l.rstrip() for l in s.display if l.strip()]

resize(100); pump(4)
os.write(fd, b"echo abcdefghij"); pump(1)
resize(45); pump(1)                     # narrow the terminal
os.write(fd, b"\x1bo"); pump(2)         # first Alt-O after resize = full repaint
after = render(45)
os.write(fd, b"\x1bo"); pump(2)
after2 = render(45)
failures=[]
info = [r for r in after2 if not r.startswith("`-=>")]
too_wide = [r for r in info if len(r) > 44]
if too_wide: failures.append(f"row exceeds 44 cols after resize: {too_wide}")
if not any("abcdefghij" in r for r in after2): failures.append(f"input lost after resize: {after2}")
if b"\x1b[0;" not in bytes(raw): failures.append("no colour emitted")
os.write(fd, b"\r"); pump(1.5); os.write(fd, b"exit\r"); pump(1.5)
print("rows after narrowing to 45:")
for r in after2: print(f"  ({len(r):>2}) {r}")
if failures:
    print("\nFAILURES:"); [print(" -",f) for f in failures]; sys.exit(1)
print("\nPASS: styled rows fit and input survives after a resize")
