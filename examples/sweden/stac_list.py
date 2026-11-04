"""Lists the newest Laserdata Skog sheet of every 2.5 km position inside a box around a point, as TSV: area, url, size (bytes).

usage: stac_list.py <area> <lon> <lat> <half width in metres>  >> sheets.tsv
Lantmäteriet's STAC (https://api.lantmateriet.se/stac-hojd/v1, collection dsm-skoglig-copc) needs no login; the downloads do (se_download.sh).
"""
import json, sys, urllib.request
from pyproj import Transformer

area, lon, lat, half = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
to3006 = Transformer.from_crs(4326, 3006, always_xy=True)
back = Transformer.from_crs(3006, 4326, always_xy=True)
e, n = to3006.transform(lon, lat)
x0, y0 = back.transform(e - half, n - half)
x1, y1 = back.transform(e + half, n + half)
url = f"https://api.lantmateriet.se/stac-hojd/v1/search?collections=dsm-skoglig-copc&bbox={x0},{y0},{x1},{y1}&limit=100"
items = []
while url:
    page = json.load(urllib.request.urlopen(url))
    items += page["features"]
    url = next((l["href"] for l in page["links"] if l["rel"] == "next"), None)
newest = {}
for item in items:  # a position is scanned more than once over the years: keep the newest scan
    key = item["id"].split("-", 1)[1]
    when = item["properties"].get("datetime", "")
    if key not in newest or when > newest[key][0]:
        newest[key] = (when, item)
for _, item in sorted(newest.values(), key=lambda v: v[1]["id"]):
    data = item["assets"]["data"]
    print(f"{area}\t{data['href']}\t{data['file:size']}")
