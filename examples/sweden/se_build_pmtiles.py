"""Builds one PMTiles archive per block of processed sheets (5 x 5 sheets = 12.5 km) with `pullauta vectortiles --crs sweref99tm`.

Blocks keep the vegetation mosaic small (a 40 x 40 km area would need several GB); tile2 merges the archives that share a tile.
usage: PROCESSED=... PMTILES_OUT=... PULLAUTA=... [WORK=...] se_build_pmtiles.py [parallel builds, default 2]   (rebuilds only blocks whose sheets changed)
"""
import hashlib, os, re, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor

PROC = os.environ["PROCESSED"]                    # where se_proc_tile.sh keeps the sheets
BLOCKS = os.path.join(os.environ.get("WORK", "."), "se_blocks")   # symlink folders and builds (local disk)
OUT = os.environ["PMTILES_OUT"]
PULLAUTA = os.environ["PULLAUTA"]
BLOCK_M = 12500
os.makedirs(BLOCKS, exist_ok=True); os.makedirs(OUT, exist_ok=True)

blocks = {}
for area in sorted(os.listdir(PROC)):
    for sheet in sorted(os.listdir(os.path.join(PROC, area))):
        d = os.path.join(PROC, area, sheet)
        if not os.path.exists(os.path.join(d, ".done")): continue
        m = re.match(r"y\d\d\w+-(\d+)_(\d+)_\d+$", sheet)
        n, e = int(m.group(1)) * 100, int(m.group(2)) * 100
        blocks.setdefault(f"{area}_{e // BLOCK_M}_{n // BLOCK_M}", []).append((sheet, d))

def build(item):
    name, sheets = item
    key = hashlib.sha1("\n".join(sorted(s for s, _ in sheets)).encode()).hexdigest()
    stamp = os.path.join(OUT, name + ".sheets")
    out_file = os.path.join(OUT, name + ".pmtiles")
    if os.path.exists(out_file) and os.path.exists(stamp) and open(stamp).read() == key:
        return name, len(sheets), 0.0, "unchanged"
    folder = os.path.join(BLOCKS, name)
    subprocess.run(["rm", "-rf", folder]); os.makedirs(folder)
    for sheet, d in sheets: os.symlink(d, os.path.join(folder, sheet))
    work = os.path.join(BLOCKS, name + "_out")
    subprocess.run(["rm", "-rf", work])
    t = time.time()
    r = subprocess.run([PULLAUTA, "vectortiles", folder, work, "--crs", "sweref99tm", "--no-tile-files", "--threads", "2"], capture_output=True, text=True)
    dt = time.time() - t
    if r.returncode != 0: return name, len(sheets), dt, "FAILED " + r.stderr[-300:]
    subprocess.run(["cp", os.path.join(work, "map.pmtiles"), out_file + ".new"], check=True)
    os.replace(out_file + ".new", out_file); open(stamp, "w").write(key)
    subprocess.run(["rm", "-rf", work, folder])
    return name, len(sheets), dt, f"{os.path.getsize(out_file)/1e6:.1f} MB"

jobs = int(sys.argv[1]) if len(sys.argv) > 1 else 2
t0 = time.time()
with ThreadPoolExecutor(jobs) as ex:
    for name, n, dt, msg in ex.map(build, sorted(blocks.items())):
        print(f"{name}: {n} sheets, {dt:.1f} s, {msg}", flush=True)
print(f"BUILDDONE {len(blocks)} blocks wall={time.time()-t0:.1f}s")
