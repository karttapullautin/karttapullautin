# Optimisations: rendering, knolls and vector tile production

This document describes the performance changes of the rendering, the knoll detection and the contours, and of the vector tile builder (`pullauta vectortiles`), and their measurements. The aim was that processing one
3 × 3 km sheet takes less time and memory, and that vector data (MVT tiles) is produced without unnecessary intermediate steps. All numbers were measured
on the same workstation (8 cores) with MML (Finnish National Land Survey) laser scanning data from the Funet mirror, unless stated otherwise.
**Estimates are marked as estimates.**

## Summary

| Measure | Before | Now |
|---|---|---|
| Large sheet (`M4211E3`, 8.1 M points), karttapullautin | 24.2 s | 8.0 s (`vector_output`) / 9.9 s (normal run) |
| Smaller sheet (`K3414B1`, 4.3 M points), karttapullautin | 22.8 s, 1.40 GB | 11.1 s, 0.45 GB |
| Peak memory, large sheet | 466 MB | 315 MB (`vector_output`) |
| Water cells for vector production | 9.2 s | 0.16 s |
| Vegetation vectorisation and MVT build, nine sheets | - | 5.3 s, 313 MB |
| Whole chain for nine sheets (karttapullautin → MVT) | - | **74 s** |
| Large sheet (`M4122F4`, 8.2 M points), karttapullautin, `vector_output` | 7.26 s | 6.0 s (section 7) |
| `vectortiles`, nine sheets | 5.4 s (1 thread) | 4.3 s (2 threads), 3.8 s (all 8 cores) (section 8) |

The outputs of karttapullautin are **byte-identical** to the original binary: both PNG maps and all temporary files.

## Measuring basis

Karttapullautin already has `Timing` sections (`RUST_LOG=debug`). With the original binary, 13.8 s of the 22.8 s of the small sheet (4.3 M points) went to
rendering (7.4 s + 6.4 s: the map with and the map without depressions). Of the 24.2 s of the large sheet (8.1 M points), the knoll detection
(`knoll detection part 2`) took 14.6 s.

Every fix was preceded by a measurement of where the time really goes. Two initial assumptions turned out to be wrong:

- **PNG decoding was not slow** (50-70 ms per file). What was slow was `resize` of the `image` crate (3.4 s for a 3003 → 7093 px scaling).
- **Drawing the curves was not slow** (under 0.1 s). It was first assumed that stamping every point 25-36 times would be the bottleneck, but the measurement did
  not support it.

## Changes

### 1. Rendering is shared by the two maps

Karttapullautin writes two PNG maps (with and without depressions). Each call decoded and scaled the same layers and computed the slope of the height
field from scratch. Now:

- `render_both` builds the base (vegetation, undergrowth, low layer, north lines) and reads the height field and the slope once (`load_curve_inputs`). The
  map with depressions is made first because it writes `formlines.dxf.bin`.
- Decoded PNGs are released right after use (`into_rgba8()`, no copies).
- New ini setting `render_nodepressions` (default `1`). With `0` only the map with depressions is made.
- `Timing` sections in `render_both`.

Result (`K3414B1`): 22.8 s → 19.7 s, 1.40 GB → 1.07 GB. With `render_nodepressions=0`: 16.4 s and 875 MB.

### 2. Nearest-neighbour scaling without the f32 intermediate

`image::imageops::resize` uses an f32 intermediate image (`Rgba32F`) whatever the filter. For 3003² → 7093² it allocated about 340 MB extra and took
3.4 s. With the nearest filter every output pixel is one source pixel with weight 1, so the value stays an integer. The replacement computes the source
pixel with the same f32 formula (`floor((x + 0.5) * ratio)`) and copies straight into a u8 buffer. Unit tests compare the result with the crate's for
several sizes and for a cropped source area.

Result: 19.7 s → 11.0 s, 1.07 GB → 0.70 GB. Base layers 3.8 s → 0.96 s and overlaid layers 3.0 s → 0.3 s per map.

### 3. Layers straight onto the canvas

- `NearestView` (implements `GenericImageView`) reads the source pixels only when they are drawn, so no scaled copies are made.
  `image::imageops::overlay` uses it as it is, so the blending is exactly the same.
- `undergrowth.png` is blended row by row with the `png` crate when its size matches the canvas (otherwise the old path is used). The whole image is never
  held decoded (about 200 MB saved).
- The base of the map without depressions is rebuilt (about 0.4 s) instead of keeping a copy (about 200 MB).

Result: 703 MB → 452 MB, with the total time staying at 11 s. Other sheets (old → new): `L3313A4` 17.3 → 5.3 s and 1.37 → 0.45 GB, `R4233C4` 15.9 → 4.8 s
and 1.34 → 0.44 GB, `V4133E4` 28.7 → 18.1 s and 1.42 → 0.51 GB.

### 4. Knoll detection: no more all-pairs loops

On the large sheet the knoll detection took 14.6 s. The added `Timing` sections (`knolldetector`) located two loops (`bounding boxes and tops` 6.5 s and
`candidates` 8.1 s) in which every line was compared with every point and the height was looked up twice from a hash map. The other parts take under 0.3 s
together.

- `elevation` is now a `Vec<f64>` indexed by the line (instead of a hash map).
- `PointGrid` (a regular 100 m grid) finds the points inside a bounding box. The result is sorted in the original list order, so the first hit (`topid`)
  and the "any hit" (`skip`) are the same as before.

Result: both loops take about 20 ms. Large sheet 24.2 s → 9.9 s. All temp files and PNGs byte-identical.

### 5. The `vector_output` mode

Vector data needs neither the PNG maps nor all the text DXF files. The ini setting `vector_output = 1` (use it with `output_dxf=0`):

- **skips the PNG rendering.** The form lines (`formlines.dxf.bin`) are still made by `draw_curves`, without an image.
- **computes the water cells in karttapullautin** (`vegetation::watercells` → `temp/water.txt`): 3 m cells that contain water-class points, one line
  `x y water total` per cell. The earlier way converted the points to text (`internal2xyz`) and counted with `awk`: 9.2 s → 0.16 s, with the same 2994 cells.
- **writes text DXF of only five layers** (`out2`, `formlines`, `c2g`, `c3g`, `dotknolls`). The DXFs of the other steps are left out: `contours03.dxf` alone was
  144.6 MB of text (the others: `out.dxf` 15.9 MB, `out2.dxf` 19.7 MB, `formlines.dxf` 6.9 MB).

Result (large sheet): 9.9 s → 8.0 s, 466 MB → 315 MB. The five DXF files and `vegetation.png` are byte-identical to the normal run.

### 6. The vector tile builder: `pullauta vectortiles`

The command `pullauta vectortiles <run folder> <output> [--no-tile-files] [--threads N] [--crs <name>]` reads the `temp` folders of karttapullautin runs (made with
`vector_output=1`): the `.dxf.bin` files of the layers, `vegetation.png` (an indexed PNG: the palette index is the class directly), `vegetation.pgw` and `water.txt`.
The result is `map.pmtiles`, `metadata.json` and optionally `tiles/z/x/y.pbf` (see [vectortiles.md](vectortiles.md)).

Modules (`src/vectortiles/`): `project.rs` (any projected CRS to Web Mercator through `proj4rs`), `features.rs` and
`simplify.rs` (reading the layers, Douglas–Peucker for lines, JTS-style ring simplification), `raster.rs` (mosaic, block majority, morphology,
connected-area detection, contour tracing), `vegetation.rs`, `tiler.rs` (tile split, line clipping with Liang–Barsky, polygon clipping with the `geo`
crate), `mvt.rs` (hand-written protobuf) and `pmtiles.rs` (PMTiles v3: Hilbert ids, leaf directories, tile data streamed to a temporary file). New dependencies:
`geo` (only boolean operations and area), `flate2` (gzip), `proj4rs` and `rayon`. The module is excluded from wasm builds.

Nine 3 x 3 km sheets (81 km2) are tiled in **5.3 s** (vegetation 3.6 s) with a peak memory of 313 MB.

Verification: the projection against pyproj (under 1 mm); the tile ids against a reference; the MVT round trip; a PMTiles reader (the Python `pmtiles` package) reads the header and
all 72 tiles of the test area, and OpenLayers' MVT reader parses all features; PMTiles writing with leaf directories; closing and smoothing against scipy; the nesting rule of the vegetation.
Vegetation ring simplification follows the rule of GEOS `preserve_topology=True` (the first parts of a ring are split to depth 3 so that it keeps at least four points)
but without the intersection checks of JTS, so a polygon can differ slightly from a GEOS result.

### 7. Contours: visit only the cells a level crosses

Profiling (`Timing` sections, 8.2 M point sheet, 6.0 s) showed that `knoll detection part 1` (2.43 s) is in practice one call: `heightmap2contours` with a
0.3 m interval (2.6 s; `xyz2heightmap` takes only 0.18 s). `grid2contours` scanned the whole grid of 2.25 M cells for every contour level (208 of them),
although most cells do not cross a given level.

The cells are now bucketed by level before the loop: a cell belongs to the levels between the smallest and largest of its corner values (the same
comparisons as the old skip), and the cells are visited in the same i, j order. The levels are computed with the same `level += v` summation as before.
The result is **byte-identical** (all 11 compared files: `.dxf.bin` and `.hmap`).

| | Before | Now |
|---|---|---|
| `knoll detection part 1` | 2.43 s | 1.23 s |
| `contour generation part 2` (2.5 m contours) | 0.30 s | 0.17 s |
| whole sheet | 7.26 s | 6.0 s |

What remains is about 1 s: the segment search (0.58 s, 8.1 M end points) and chaining the polylines through a HashMap (0.31 s). They were not changed (decision:
not implemented). Possible follow-ups are running the levels in parallel and replacing the HashMap with a grid index.

#### Time spent after this (large sheet, three runs, 6.0 s)

| Step | Time |
|---|---|
| vegetation generation | 2.0 s |
| knoll detection part 1 (`contours03`) | 1.23 s |
| reading the LAZ (`preparing input file`) | 0.94 s |
| cliff generation | 0.73 s |
| contour generation 1-4 | 0.55 s |
| knoll detection part 2 | 0.22 s |
| water cells | 0.16 s |
| DXF/bin outputs, form lines | 0.16 s |

There is no parallelism except in two settings: `processes` (sheets in parallel, needs `batch=1`) and `parallel_laz_decompression`. Inside one sheet the steps are
single-threaded (`vector_output` skips the PNG rendering, where the `image` crate's rayon was the only parallelism), so one sheet uses about 1.5 of 8 cores.

### 8. Vegetation polygons on several threads: `--threads`

The vegetation of the `vectortiles` step (3.6 s for nine sheets) split as follows: grid 0.54 s, naming the areas 0.13 s, tracing 0.44 s, smoothing
0.58 s, union 0.52 s, clipping to the parent 0.78 s, simplification and projection 0.33 s.

`vegetation::polygons` now runs three phases in a `rayon` pool: (1) tracing, smoothing and union per area, (2) clipping to the parent by nesting depth (a
parent always comes before its child, so areas of the same depth only read the finished areas of the previous depth), (3) simplification and projection.
Results are collected in the original order, so **`map.pmtiles` is byte-identical with every thread count** (1, 2, 4, 0).

Parameter: `--threads N` (`BuildOptions::threads`); 0 = all cores, 1 = one thread, **default 2**.

| Threads | Wall time | Memory |
|---|---|---|
| 1 | 5.50 s | 369 MB |
| 2 | 4.26 s | 386 MB |
| 4 | 3.89 s | 411 MB |
| 0 (8 cores) | 3.82 s | 472 MB |

The speed-up stays small (1.44×) because building the grid, naming and reading are single-threaded and one large area dominates the parallel part. The default
is 2 so that `vectortiles` does not take all cores from other work.

### 9. Any coordinate system: proj4rs

The first version had the Krüger series of a GRS80 transverse Mercator zone (TM35FIN 27°, SWEREF 99 TM 15°). To support any CRS, the projection is now done
by [proj4rs](https://github.com/3liz/proj4rs) (pure Rust, with the `crs-definitions` EPSG database): `--crs` takes an EPSG code, a UTM zone name, `tm35fin`, `sweref99tm` or
a PROJ string, and refuses a geographic CRS. The output for TM35FIN is **byte-identical** to the hand-written series (9 sheets, 72 tiles, 2 691 237 bytes), and a little
faster (4.97 s against 5.47 s). Tests: pyproj reference points for EPSG:3067, 3006, 25832, 32633, 32735, 3035 and a PROJ string to under 1 mm, the national grids on an old
datum (EPSG:28992, 27700, 2056) to a few metres, and the name parsing and the errors.

### 10. Overview zoom z13

z14 alone was too heavy at small zooms: the same land area needs four times the number of z14 tiles, and zoom 13 would draw tens of thousands of lines at
once. `pullauta vectortiles` now makes **z13 tiles** in addition to z14 (`--overview-zoom N`, default 13, `none` = z14 only), with fewer and simpler features
(`Features::overview`, in Web Mercator metres): only `normal` and `index` contours (3 m tolerance, at least 60 m long), cliffs `cliff2`/`cliff3` (at least
25 m), vegetation polygons and holes of at least 1600 m² simplified by 6 m; no form lines and no knolls. The PMTiles archive has both levels (the lower level's
tiles first, because the ids order that way).

| Test area | Without z13 | z13 + z14 |
|---|---|---|
| Tampere, 9 sheets (`map.pmtiles`) | 2.38 MB, 72 tiles | 2.83 MB, 94 tiles (z13: 22 tiles, 0.57 MB raw, 5285 contours, 3310 polygons) |
| Sweden, 14 blocks | 13.6 MB | 16.7 MB |
| Build time | 5.2 s | 5.0 s (Tampere); Swedish blocks 237 s (2 in parallel) |

One z13 tile covers four times the area but is about the same size as a z14 tile (Tampere 31.6 kB vs 36.2 kB, Skellefteå 36.9 kB vs 26.3 kB), i.e. about four
times lighter per area. The tile server supports several zooms in its archives (it checks the header's minZ..maxZ), so no change to the server was needed.

### 11. A slow block profiled: clipping to the parent

The Skellefteå centre block in Sweden (10 sheets, 62 km², 26 200 polygons) took 125 s, Tampere's 81 km² took 5 s. Profiling (one thread): reading 7.8 s,
grid 0.5 s, contours/smoothing/union 2.6 s, **clipping to the parent 122.8 s**, simplification and projection 0.6 s. Clipping a nested area to its parent with
the `geo` crate's boolean operation is slow when the parent is large (a forest with thousands of clearings: tens of thousands of points) and there are
thousands of children.

Fix: a child is not clipped if the parent has more than 4000 points (in the raster the child is a subset of the parent, so it can protrude beyond the
smoothed parent's edge by at most the distance the smoothing moves the outline), nor when the child is more than 3σ from the parent's edge in the raster.
The block went from 146 s → **28 s** on one thread, and the vegetation area changed by 0.0002 %. Tampere's result is byte-identical. All 14 Swedish blocks:
237 s → **84 s** (2 in parallel, 2 threads; CPU 470 s → 63 s).

### 12. Joining the cliff pieces

Karttapullautin draws cliffs as short (two-point) pieces. Pieces of the same kind (`cliff2`/`cliff3`/`cliff4`) whose end points are under 2 m apart are
joined into chains (`join_lines`: greedy, the nearest end point first from the tail and then from the head) and the chains are simplified by 0.5 m.
Tampere: 39 589 features / 79 178 points → **6 510 / 52 054**; the tiles 4.33 → 3.81 MB raw. In the z13 overview the cliffs are now longer chains, so the 25 m
lower limit no longer removes almost everything (1 223 cliffs).

### 13. Scan years in the tiles: 8 bytes per tile

Every tile carries the first and last laser scanning year of the sheets it covers (for the attribution box: "Laser scanning 2022-2026"). The years are in
**one empty layer whose name is two characters**: `'0' + (first − 2000)` and `'0' + (last − first)` (2000-2063, a span of at most 63 years), and the layer has only the
version field: the tile's field tag and a length of 2 bytes, the name 4, the version 2 = **8 bytes** (`mvt::scan_years_layer`, a test checks the length is under 10).
No real layer has a two-character name, OpenLayers' MVT reader (its `layers` list) does not read it, and the server's merge keeps layers with different names, so an
edge tile of two archives has both (the reader takes the union). The year of a sheet: `scan_year.txt` in the sheet's folder, or the beginning of a
Lantmäteriet sheet name `yYY` (`y25g017-…` = 2025). Measured: Tampere z14 tile 60 079 → 60 087 bytes, z13 +8; the size of the Swedish blocks does not change (<0.01 %).

### 14. Cliffs: clusters as short straight lines (the `size` class)

The previous chaining (section 12) turned dense cliff clusters into back-and-forth zigzag lines that rendered as black blobs. Now (`cluster_cliffs`) pieces of the
same kind whose points are under 3 m apart form a cluster, and the cluster is drawn as **one short, nearly straight line**: the points are divided into 6 m bins along
the main axis, the bin centres are simplified by 1.5 m, and a cluster longer than 40 m is cut in two at the middle of the pieces until every line is at most 40 m.
A single piece stays as it is. The line has an attribute **`size`**: 1 = one piece, 2 = a cluster (2-5 pieces), 3 = a large cluster (6+ pieces, a higher cliff), on which
the style draws the line thicker (×1.3 and ×1.6).

Tampere: cliffs 52 054 points (section 12) → **10 636** (−80 %), 5 252 lines (classes 1: 2 547, 2: 1 696, 3: 1 009); the whole pmtiles 2.78 → 2.69 MB. The z13 overview
keeps the kinds `cliff2` and `cliff3` that have `size` ≥ 2 or a length of at least 25 m.

## Data: point density and other countries (measurements and observations)

The results are observations of single areas; **conclusions are estimates**, and tuning the thresholds was not tried.

**MML data (Funet):** 0.70-0.91 points/m² (nine sheets, all returns; from the LAZ headers).

**5p data (MML `Avoin_laseraineisto5p`):** only five limited production areas (Forssa, Nurmijärvi, Nuuksio, Pieksämäki, Suomutunturi), not the whole country.
The Nuuksio sheet L4132F3 (9 × 1 km, 9.6-14.1 points/m², elevation up to 213 m) at full density and thinned (`thinfactor 0.08`, about 0.8/m²):

| | 5p full | Thinned |
|---|---|---|
| contours | 1518 (1 065 137 m) | 1553 (1 058 979 m, −0.6 %) |
| cliffs | **350 385** | 10 237 |
| vegetation area | 124 M | 91 M (−27 %) |
| tiles total | 6.69 MB | 0.57 MB |
| karttapullautin / sheet (1 km²) | 2.8-4.4 s, 194-248 MB | 1.4-2.1 s, 83-90 MB |

For contours, 0.8 points/m² is enough. Undergrowth (green) suffers from sparse data, and with dense data there are many times more cliffs (the default
thresholds are tuned for sparse data; an estimate, not tested).

**Lantmäteriet Laserdata Skog (Sweden):** STAC API `https://api.lantmateriet.se/stac-hojd/v1`, collection `dsm-skoglig-copc`, COPC-LAZ (LAS 1.4, point format 6),
2.5 km tiles, EPSG:5845 (SWEREF 99 TM), CC BY 4.0. Downloading the files needs Basic authentication. Karttapullautin reads the files as they are.

| Tile | Area | Density | Karttapullautin | Cliffs |
|---|---|---|---|---|
| `y20d027-67725_5050_25` | Dalarna | 1.1/m² | 7.4 s, 482 MB | 23 |
| `y21d009-66625_4125_25` | Värmland | 2.1/m² | 5.8 s, 350 MB | 866 |
| `y22a035-63500_4900_25` | Småland | 0.86/m² | 10.3 s, 566 MB | 103 842 |
| `y26h019-73725_8900_25` | Norrbotten | 1.3/m² | 6.4 s, 455 MB | 4 468 |

## The whole chain (nine-sheet test area)

Karttapullautin on the nine sheets one after another takes 69 s, and `pullauta vectortiles` **5.3 s** for the same area, so the whole chain is **74 s (8.2 s per sheet)**.

## Choices about the data size (tiles)

These do not change karttapullautin but the size of the MVT tiles:

- **Vegetation is vector from a 3 m grid.** Small polygons and holes (under 36 m²) are dropped, water is cut out, yellow is closed across 15 m gaps, and the rings are
  smoothed (σ = 6 m) and simplified (2 m). On the test sheet `M4211E3` the MVT-type estimate went 72 kB → 59 kB (smoothing) → 45 kB (closing yellow); on the
  forested `V4133E4`, 108 kB → 59 kB.
- **Contours are simplified by 0.3 m** (Douglas–Peucker) before projection; karttapullautin has already smoothed and thinned them.
- **Only z14 was stored at first.** OpenLayers requests the same tiles at zooms under 14 too, so lower levels were not needed. The test area is 2.38 MB in total (72 tiles,
  median 37 kB). Section 10 later added the z13 overview.

## Checks and how to repeat them

- `cargo fmt --check`, `cargo clippy --release` (no warnings) and `cargo test --release` pass.
- New unit tests: `nearest_view_matches_image_crate` and `nearest_view_uses_only_the_cropped_part` (`src/render.rs`).
- Comparison: run the original and the new binary on the same LAZ file in different folders and `cmp` `pullautus.png`, `pullautus_depr.png` and all files of
  `temp/`. In the vector mode compare `out2.dxf`, `formlines.dxf`, `c2g.dxf`, `c3g.dxf`, `dotknolls.dxf` and `vegetation.png`.
- Read the times with `RUST_LOG=debug pullauta <file>.laz` and the `Leaving section` lines.

## What was not done

- GPU computing was not tried: the heavy steps are branching geometry, and the measurements put the bottlenecks elsewhere (loops, copies).
- For large areas (a whole country) the build should be done block by block: now all features of a run folder are in memory at once.
- The `thinfactor` setting (thinning of the points) was not used in production, because it would change the result; it was used only to measure the effect of density.
- Speeding up the contour segment search and chaining (running the levels in parallel, a grid index instead of the HashMap) was not implemented.
- The undergrowth and cliff thresholds were not tuned for the 5p or Swedish data.
- The stamping of the curves was not changed, because the measurement did not show it to be a bottleneck.
