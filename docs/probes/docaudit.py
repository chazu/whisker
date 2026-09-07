"""Cross-check the numbers quoted in grid-design.md against the code.

A design document full of measurements rots the moment the code changes. This
caught exactly that: switching the encoder to RGBA moved the payload from 110
to 124 bytes while the document still said 110. Run it whenever pixelgrid.py
changes.
"""
import re,sys,io,os
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
import pixelgrid as p
import os
HERE=os.path.dirname(os.path.abspath(__file__))
ROOT=os.path.dirname(os.path.dirname(HERE))
doc=open(os.path.join(ROOT,"docs","grid-design.md")).read()
fails=[]
def check(name,cond,detail=""):
    print(("  ok   " if cond else "  FAIL ")+name+(f"  {detail}" if detail else ""))
    if not cond: fails.append(name)

grid=[["idle","ok","idle"],["idle","current","alert"],["idle","ok","idle"]]
d,c,a,i=p.build(grid,3,(16,34),0,gap=1)
check("doc says 11x11 pixels", "11x11 pixels" in doc and a==(11,11), str(a))
check("doc says fits one cell", "**one\ncell**" in doc.replace("\r","") and c==1, f"cells={c}")
check("doc quotes the real payload size", f"payload of {len(d)} bytes" in doc, f"{len(d)} bytes")
# capacity claims
def fits(cols,rows,scale,cells):
    dd,_,_,_=p.build([["idle"]*cols for _ in range(rows)],scale,(16,34),cells,gap=1)
    return dd is not None
check("doc's 4x8 at 3px", "4x8" in doc and fits(4,8,3,1) and not fits(5,9,3,1))
check("doc's 5x11 at 2px", "5x11" in doc and fits(5,11,2,1) and not fits(6,11,2,1))
check("doc's 8x8 in two cells", "8x8" in doc and fits(8,8,3,2) and not fits(9,8,3,2))
# dpr claim: 22x22 in one cell, 146 bytes
d2,c2,a2,_=p.build(grid,6,(32,68),0,gap=2)
check("doc's dpr=2 gives 22x22 in one cell", a2==(22,22) and c2==1, f"{a2} cells={c2}")
check("doc quotes the real dpr=2 size", f"{len(d2)} bytes" in doc, f"{len(d2)} bytes")
# the doc must not still claim graphics are rejected
check("doc no longer rejects graphics outright",
      "(rejected)" not in doc)
check("doc marks D as recommended", "### D. A pixel grid, drawn as an image (recommended)" in doc)

# --- the Rust benchmark's numbers -------------------------------------------
# These are quoted in "Will it be fast enough?" and in the appendix. Re-derive
# them by running the benchmark, so a change in the encoder cannot leave the
# document asserting a speed or size that is no longer true.
import subprocess, shutil, tempfile
rs = os.path.join(HERE, "pixelgrid.rs")
if shutil.which("rustc") is None:
    print("  skip  rustc not available; Rust numbers unchecked")
else:
    exe = os.path.join(tempfile.mkdtemp(prefix="whisker-bench-"), "pixelgrid")
    subprocess.run(["rustc", "-O", rs, "-o", exe], check=True,
                   capture_output=True)
    out = subprocess.run([exe], capture_output=True, text=True).stdout
    sizes = dict(re.findall(r"^  (.+?)\s{2,}[\d.]+ us\s+(\d+) bytes$", out, re.M))
    def size_of(prefix):
        for label, n in sizes.items():
            if label.startswith(prefix):
                return int(n)
        return None
    four = size_of("3x3 grid, 4px dots")
    five = size_of("3x3 grid, 5px dots")
    panels = size_of("4 grids side by side")
    tall = size_of("7x3 grid, 4px dots")
    # Each figure appears in both the cost table and the appendix, so require
    # the expected number of occurrences rather than merely one: otherwise
    # updating a number in one place and not the other still passes.
    check("doc quotes the real 4px payload twice",
          doc.count(f"{four} bytes") == 2, f"{four}, seen {doc.count(f'{four} bytes')}x")
    check("doc quotes the real 5px payload", f"{five} bytes" in doc, str(five))
    # The four-panel figure is quoted in three places: the prose under
    # "Several grids at once", the cost table, and the appendix.
    check("doc quotes the real four-panel payload in all three places",
          doc.count(str(panels)) == 3, f"{panels}, seen {doc.count(str(panels))}x")
    check("doc quotes the real 7-row payload", f"{tall} bytes" in doc, str(tall))
    check("doc's 7-rows-per-cell claim matches the benchmark",
          re.search(r"^  7x3 grid, 4px dots, 1 cell\s+[\d.]+ us", out, re.M) is not None
          and re.search(r"^  8x3 grid, 4px dots, 1 cell\s+does not fit", out, re.M) is not None
          and "holds seven rows, not eight" in doc)
    check("doc records the stored-block payload it rejected", "11841 bytes" in doc)

print("failures:",len(fails))
sys.exit(1 if fails else 0)
