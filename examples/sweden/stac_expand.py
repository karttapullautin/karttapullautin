#!/usr/bin/env python3
"""Lists the Laserdata Skog sheets to download next, to merge two downloaded areas and to grow the area systematically, over land only.

usage: stac_expand.py <data dir with <area>/<sheet>.copc.laz> <area,area,...> <rings> [min size MB, default 15] > next.tsv   (area TAB url TAB size)

Positions are the 2.5 km sheets of Lantmäteriet's grid (the newest scan of each). The sheets already downloaded in the given areas define a bounding
box (in sheets). The list is, in this order:
  0. "merge": every sheet inside that box that is not downloaded yet (the gap between two areas fills up),
  1..<rings>. ring r: the sheets whose Chebyshev distance to the box is r sheets (r x 2.5 km),
so a download that follows the list in order grows the area one ring at a time. Land only: the STAC has sheets only where there was scanning, none
over the open sea; of the coastal sheets those under the minimum size (default 15 MB, a normal forest sheet is 90-140 MB) are mostly sea and are left out.
A summary (sheets and GB per step) goes to stderr.
"""
import collections, json, os, re, sys, urllib.request
from pyproj import Transformer

data_dir, areas, rings = sys.argv[1], sys.argv[2].split(","), int(sys.argv[3])
min_bytes = float(sys.argv[4] if len(sys.argv) > 4 else 15) * 2**20
CELL = 2500

have = set()
for area in areas:
    for f in os.listdir(os.path.join(data_dir, area)):
        m = re.match(r"y\d\d\w+-(\d+)_(\d+)_25\.copc\.laz$", f)
        if m:
            have.add((int(m.group(2)) * 100 // CELL, int(m.group(1)) * 100 // CELL))
if not have:
    sys.exit("no sheets downloaded in " + data_dir)
x0, x1 = min(c[0] for c in have), max(c[0] for c in have)
y0, y1 = min(c[1] for c in have), max(c[1] for c in have)

back = Transformer.from_crs(3006, 4326, always_xy=True)
pad = rings + 1
corners = [back.transform(x * CELL, y * CELL) for x in (x0 - pad, x1 + pad + 1) for y in (y0 - pad, y1 + pad + 1)]
bbox = (min(c[0] for c in corners), min(c[1] for c in corners), max(c[0] for c in corners), max(c[1] for c in corners))
url = f"https://api.lantmateriet.se/stac-hojd/v1/search?collections=dsm-skoglig-copc&bbox={','.join(map(str, bbox))}&limit=100"
items = []
while url:
    page = json.load(urllib.request.urlopen(url))
    items += page["features"]
    url = next((l["href"] for l in page["links"] if l["rel"] == "next"), None)

newest = {}
for item in items:  # the newest scan of every position
    b = item["assets"]["data"]["proj:bbox"]
    key = (int(b[0] // CELL), int(b[1] // CELL))
    when = item["properties"].get("datetime", "")
    if key not in newest or when > newest[key][0]:
        newest[key] = (when, item)


def step(cell):
    """0 = inside the box (merge), r = r sheets outside it"""
    dx = max(x0 - cell[0], 0, cell[0] - x1)
    dy = max(y0 - cell[1], 0, cell[1] - y1)
    return max(dx, dy)


rows = []
for cell, (_, item) in newest.items():
    data = item["assets"]["data"]
    if cell in have or data["file:size"] < min_bytes or step(cell) > rings:
        continue
    rows.append((step(cell), cell[1], cell[0], data["href"], data["file:size"]))
rows.sort()
summary = collections.defaultdict(lambda: [0, 0])
for s, _, _, _, size in rows:
    summary[s][0] += 1
    summary[s][1] += size
print(f"downloaded {len(have)} sheets in {x1 - x0 + 1} x {y1 - y0 + 1} sheets box; to download:", file=sys.stderr)
for s in sorted(summary):
    n, size = summary[s]
    print(f"  {'merge (inside the box)' if s == 0 else f'ring {s} ({s * 2.5:g} km)'}: {n} sheets, {size / 2**30:.1f} GB", file=sys.stderr)
print(f"  total {len(rows)} sheets, {sum(r[4] for r in rows) / 2**30:.0f} GB", file=sys.stderr)
for s, _, _, href, size in rows:
    print(f"expansion\t{href}\t{size}")
