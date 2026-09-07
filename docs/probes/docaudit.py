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
print("failures:",len(fails))
sys.exit(1 if fails else 0)
