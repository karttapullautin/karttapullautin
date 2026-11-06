# Optimisations: rendering, knolls, contours and vector tiles

The aim was that processing one 3 × 3 km sheet takes less time and memory, and that vector data (MVT tiles) is produced without unnecessary intermediate steps.
Every change was preceded by a measurement of where the time really goes (`RUST_LOG=debug` prints the `Timing` sections). All numbers are from an 8-core
workstation with MML laser scanning sheets, unless stated otherwise. **Estimates are marked as estimates.**

## Summary

| Measure | Before | Now |
|---|---|---|
| Large sheet (`M4211E3`, 8.1 M points), whole run | 24.2 s | 9.9 s (8.0 s with `vector_output`) |
| Smaller sheet (`K3414B1`, 4.3 M points), whole run | 22.8 s, 1.40 GB | 11.1 s, 0.45 GB |
| Peak memory, large sheet | 466 MB | 315 MB (`vector_output`) |
| Large sheet (`M4122F4`, 8.2 M points), `vector_output` | 7.26 s | 6.0 s |
| `pullauta vectortiles`, nine sheets (81 km²) | - | 5.3 s, 313 MB (3.8 s with all cores) |

The outputs of karttapullautin are **byte-identical** to the original binary: both PNG maps and all files of `temp/`.
Two first guesses were wrong: PNG decoding was not slow (50-70 ms per file), and drawing the curves was not slow (under 0.1 s). The time went to `resize`
of the `image` crate (3.4 s for one scaling), to rendering the same layers twice, and to all-pairs loops in the knoll detection.

## Karttapullautin

1. **The two maps share their work.** The base layers, the height field and its slope are computed once (`render_both`) instead of once per map. New ini setting
   `render_nodepressions` (default `1`); with `0` only the map with depressions is made (22.8 s → 16.4 s, 875 MB on the small sheet).
2. **Nearest-neighbour scaling without the f32 intermediate.** `image::imageops::resize` allocates an `Rgba32F` image even for the nearest filter (340 MB and 3.4 s for
   3003² → 7093²). The replacement uses the same source-pixel formula and writes u8 directly; unit tests compare it with the crate for several sizes and for a cropped source.
3. **Layers straight onto the canvas.** `NearestView` reads source pixels lazily (no scaled copies), `undergrowth.png` is blended row by row, and the base of the second map is
   rebuilt (0.4 s) instead of kept (200 MB). Steps 1-3 together: small sheet 22.8 s → 11.0 s and 1.40 GB → 0.45 GB; other sheets 17.3 → 5.3 s, 15.9 → 4.8 s and 28.7 → 18.1 s.
4. **Knoll detection without all-pairs loops.** On the large sheet it took 14.6 s: every line was compared with every point and heights were looked up twice from a hash map.
   Elevations are now a `Vec<f64>` and a 100 m `PointGrid` finds the points in a bounding box (in the original list order, so the first hit and "any hit" are unchanged):
   both loops take about 20 ms, the sheet 24.2 s → 9.9 s.
5. **Contours visit only the cells a level crosses.** `grid2contours` scanned the whole grid (2.25 M cells) for each of ~200 levels. The cells are now bucketed by the levels between
   their lowest and highest corner and visited in the same order: `knoll detection part 1` 2.43 s → 1.23 s, the sheet 7.26 s → 6.0 s. What remains of `heightmap2contours`
   is the segment search (0.58 s) and chaining through a HashMap (0.31 s); running the levels in parallel or a grid index instead of the HashMap would be the next steps.
6. **`vector_output = 1`** (use with `output_dxf = 0`) skips the PNG rendering (the form lines are still made), computes the 3 m water cells in-process (`temp/water.txt`, 9.2 s → 0.16 s;
   they used to be counted with `internal2xyz` and `awk`) and writes text DXF of five layers only (`contours03.dxf` alone was 144.6 MB). Large sheet 9.9 s → 8.0 s and 466 → 315 MB.

Where the 6.0 s of the large sheet go now: vegetation 2.0 s, knoll detection part 1 1.23 s, reading the LAZ 0.94 s, cliffs 0.73 s, contours 0.55 s, knoll detection part 2 0.22 s, water cells 0.16 s,
DXF/bin outputs 0.16 s. There is no parallelism inside a sheet except the optional `parallel_laz_decompression`; `processes` runs several sheets at once, so one sheet uses about 1.5 of 8 cores.

## Vector tiles: `pullauta vectortiles`

The command reads the `temp` folders of runs made with `vector_output = 1` and writes `map.pmtiles` ([vectortiles.md](vectortiles.md)). Modules in `src/vectortiles/`: `project.rs`
(any projected CRS to Web Mercator through `proj4rs`), `features.rs` and `simplify.rs` (the layers, Douglas–Peucker, JTS-style ring simplification), `raster.rs` (mosaic, block majority,
morphology, connected areas, contour tracing), `vegetation.rs`, `tiler.rs` (tile split, Liang–Barsky line clipping, polygon clipping with `geo`), `mvt.rs` (hand-written protobuf) and `pmtiles.rs`
(PMTiles v3, tile data streamed to a temporary file). The module is excluded from wasm builds. Nine sheets take 5.3 s and 313 MB; the whole chain (pullauta on nine sheets 69 s, then the tiles) 74 s.

- **Threads.** The vegetation polygons run in a `rayon` pool in three phases (tracing/smoothing/union per area, clipping to the parent by nesting depth, simplification and projection), collected in
  the original order, so **the output is byte-identical for every `--threads`** (default 2, 0 = all cores): 1 thread 5.50 s / 369 MB, 2 threads 4.26 s, 4 threads 3.89 s, 8 cores 3.82 s / 472 MB.
- **Clipping to the parent** with the `geo` crate is slow for a large parent (a forest with thousands of clearings) with thousands of children: a block of 10 sheets took 125 s. A child is not clipped when the
  parent has more than 4000 points or the child is more than 3σ from the parent's edge in the raster (it can protrude at most by the distance the smoothing moves the outline): 146 s → 28 s on one thread,
  the vegetation area changed by 0.0002 %.
- **Any coordinate system.** `proj4rs` (pure Rust, `crs-definitions` EPSG database) accepts an EPSG code, a UTM zone, `tm35fin`, `sweref99tm` or a PROJ string and refuses a geographic CRS. The output for
  TM35FIN is byte-identical to the Krüger series it replaced, and a little faster (4.97 s against 5.47 s). Tests: pyproj reference points for several CRSs to under 1 mm, national grids on an old datum to
  a few metres (the definition's 3/7 parameters, no grid files), the name parsing and the errors.
- **Overview zoom z13** (`--overview-zoom`, default 13): z14 alone is heavy at small zooms (four times the tiles, tens of thousands of lines at once). z13 has only `normal`/`index` contours (3 m
  tolerance, ≥ 60 m long), cliffs `cliff2`/`cliff3` (≥ 25 m) and vegetation polygons and holes ≥ 1600 m² simplified by 6 m. A z13 tile is about as big as a z14 tile but covers four times the area.
- **Cliffs.** Karttapullautin draws cliffs as short two-point pieces. Pieces of a kind whose vertices are under 3 m apart form a cluster, drawn as **one short, nearly straight line** along its principal axis
  (6 m bins, simplified by 1.5 m, cut at 40 m); the `size` attribute (1 a piece, 2 a cluster, 3 a large cluster) lets a style draw it thicker. Chaining the pieces instead made zigzags; clustering
  cut the cliff points of a test area by 80 % (52 054 → 10 636).
- **Scan years in 8 bytes.** Every tile has the first and last scan year of its sheets in one empty layer whose two-character *name* encodes them (`'0' + (first - 2000)`, `'0' + (last - first)`), so a web
  map can show "Laser scanning 2022-2026". No real layer has such a name, MVT readers that list the layers they want never read it, and a server that merges two archives' tiles keeps both layers.
- **Data size.** Vegetation is vector from a 3 m grid (polygons and holes under 36 m² dropped, water cut out, open land closed across 15 m gaps, rings smoothed with σ = 6 m and simplified by 2 m); contours are
  simplified by 0.3 m (pullauta has already smoothed and thinned them). A nine-sheet area with both zooms is 2.8 MB (94 tiles).

## Point density (observations of single areas; conclusions are estimates)

MML's open data has 0.7-0.9 points/m². Lantmäteriet's Laserdata Skog (COPC-LAZ, SWEREF 99 TM, CC BY 4.0) has 0.9-2.1 points/m² and is read as it is with `--crs sweref99tm`. MML's 5p data (a few limited production
areas, 10-14 points/m²) gives 350 385 cliff pieces on a 3 × 3 km sheet against 10 237 when thinned to 0.8 points/m² (`thinfactor 0.08`), while the contours differ by 0.6 %: 0.8 points/m² is enough for contours,
sparse data makes the undergrowth (green) weak, and the default thresholds are tuned for sparse data (not tested on dense data).

## Checks and how to repeat them

- `cargo fmt --check`, `cargo clippy --all-features --all-targets -- -D warnings` and `cargo test` pass.
- Compare the original and the new binary on the same LAZ file in different folders: `cmp` `pullautus.png`, `pullautus_depr.png` and all files of `temp/` (in `vector_output` mode `out2.dxf`, `formlines.dxf`,
  `c2g.dxf`, `c3g.dxf`, `dotknolls.dxf` and `vegetation.png`).
- Read the times with `RUST_LOG=debug pullauta <file>.laz` and the `Leaving section` lines.

## Not done

- GPU computing: the heavy steps are branching geometry, and the measurements put the bottlenecks elsewhere.
- The contour segment search and chaining (levels in parallel, a grid index), and the stamping of the curves (not a bottleneck).
- `pullauta vectortiles` holds all features of a run folder in memory: build a large area in blocks of about 12 km and let a tile server merge the archives.
- The undergrowth and cliff thresholds are not tuned for dense (5p) or Swedish data.
