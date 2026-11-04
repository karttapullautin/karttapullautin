# Vector tiles from karttapullautin: `pullauta vectortiles`

This fork can turn the output of Karttapullautin straight into **vector tiles** (MVT in a PMTiles archive) for web maps, with no intermediate
GeoJSON or Python step. Contours, form lines, cliffs, knolls and the vegetation (as polygons) of many sheets become one `map.pmtiles`.
Measured on an 8-core workstation: nine 3 x 3 km sheets (81 km2) take **5 s and 313 MB** to tile, after the sheets themselves
(about 7 s and 300 MB each, 8 M points). Details and measurements of every optimisation: [optimizations.md](optimizations.md) (in Finnish).

## Quick start

```bash
cargo build --release
examples/laz-to-pmtiles.sh my-laz-sheets/ out/ --crs tm35fin --scan-year 2020     # sheets -> out/out/map.pmtiles
```

[examples/laz-to-pmtiles.sh](../examples/laz-to-pmtiles.sh) runs the whole chain below: `pullauta` on every sheet in parallel (with `vector_output = 1`),
then `pullauta vectortiles`. Four 1 km sheets (10 points per m2) take 7 s and 4 s. [examples/sweden](../examples/sweden) has the scripts for fetching
Lantmäteriet's open laser data and building a large area in blocks.

## Pipeline

```text
LAZ/LAS sheets --pullauta (vector_output=1)--> temp/*.dxf.bin, vegetation.png, water.txt   (one run folder per sheet)
run folders    --pullauta vectortiles-------> map.pmtiles (+ metadata.json, tiles/z/x/y.pbf)
```

1. In `pullauta.ini` set `vector_output = 1` (and `output_dxf = 0`). The run then skips the PNG maps and writes, in `temp/`, the layers as
   `out2.dxf.bin` (contours), `formlines.dxf.bin`, `c2g.dxf.bin` and `c3g.dxf.bin` (cliffs), `dotknolls.dxf.bin`, the vegetation raster
   `vegetation.png` + `vegetation.pgw` (1 m pixels, indexed PNG) and `water.txt` (the 3 m cells that contain water-class points).
   Run it once per sheet, each in its own folder.
2. `pullauta vectortiles <folder of runs> <output folder> [options]`: every sub-folder with a `temp` folder is a sheet.

| Option | Meaning |
|---|---|
| `--no-tile-files` | write only `map.pmtiles` and `metadata.json`, not `tiles/z/x/y.pbf` |
| `--threads N` | threads of the vegetation polygons (default 2, 0 = all cores, 1 = one); the output is byte-identical for every N |
| `--crs tm35fin\|sweref99tm` | the coordinate system of the runs: ETRS-TM35FIN (EPSG:3067, default) or SWEREF 99 TM (EPSG:3006) |
| `--overview-zoom N\|none` | a lighter, simplified zoom N below z14 (default 13); `none` = only z14 |

A folder of runs can be any size, but the vegetation of all sheets is joined into one mosaic (one byte per metre), so build large areas in blocks of
about 12 km (5 x 5 sheets) and let the tile server merge the archives.

## Tiles

Web Mercator, **z14** with all the detail and **z13**, a lighter overview (main contours, large cliffs, bigger and simpler vegetation polygons, no form
lines or knolls). An application overzooms z14.

| Layer | Geometry | Properties |
|---|---|---|
| `contour` | line | `kind`: `normal`, `index`, `intermed`, `depression`, `depression_intermed`, `small_depression`, `slope`; `ele` |
| `formline` | line | - |
| `cliff` | line | `kind`: `cliff2`, `cliff3`, `cliff4`; `size`: 1 a single piece, 2 a cluster of 2-5 pieces, 3 a larger cluster (a higher cliff) |
| `knoll` | point | `kind`: `dotknoll`, `uglydotknoll`, `udepression`, `uglyudepression` |
| `vegetation` | polygon | `kind`: `open` or `green`; `level` 1-7 for green (density), 0 for open |

- **Vegetation** is vectorised from `vegetation.png`: 3 m grid, water removed (water-class points, closed over gaps), open land closed over gaps up to 15 m,
  nested greens (a darker level is drawn only where it is at least two levels denser than the colour already drawn, and always inside the lighter one),
  polygons and holes under 36 m2 dropped, smoothed (sigma 6 m) and simplified (2 m). White (ordinary forest) is not drawn.
- **Cliffs** come from Karttapullautin as short pieces. Pieces of the same kind with vertices within 3 m of each other form a cluster, and a cluster becomes
  **one short, nearly straight line** along its principal axis (bins of 6 m, simplified by 1.5 m, cut at 40 m), with the `size` class above for the renderer
  to draw it thicker. (Joining the pieces into chains, as an earlier version did, made back-and-forth zigzags.)
- **Laser scanning years.** Every tile carries the first and last scanning year of the sheets it covers as **one empty layer of 8 bytes**: the layer's *name*
  is two characters, `'0' + (first - 2000)` and `'0' + (last - first)` (years 2000-2063), plus the version field. No real layer has a two-character
  name, MVT readers that list the layers they want never read it, and a tile server that merges two archives' tiles keeps both layers, so a reader takes the
  union. The year of a sheet is the number in `scan_year.txt` in the sheet's folder, or the two digits after the `y` that starts the name of a
  Lantmäteriet sheet (`y25g017-...` = 2025). An application can show "Laser scanning 2022-2026" from it.

## PMTiles

PMTiles v3 written directly (Hilbert tile ids, root and leaf directories, gzip); the tile data streams to a temporary file, so memory does not grow with the
number of tiles. A tile server that serves every `*.pmtiles` of a directory and merges the tiles that two archives share can serve an area built in
blocks. Tested with `osm-mapsforge serve` of the Karttahimmeli proxy-server and with the Python `pmtiles` reader and OpenLayers' MVT format.

## Sweden

`--crs sweref99tm` reads Lantmäteriet's open laser data (Laserdata Skog, COPC-LAZ, 2.5 km sheets) as it is. Measured: 467 sheets processed with 4 parallel
`pullauta` runs in 2462 s (5.3 s per sheet, 0.85 s per km2), the tiles of 40 blocks (528 sheets) in 186 s.

## Tests

`cargo test --release` (104 tests): the projection against pyproj (1 mm), the MVT and PMTiles encoders (round trips, large archives), vegetation rules, cliff
clusters, scan years. The Python scripts these replace, and a check of the Rust output against them, are described in `optimizations.md`.
