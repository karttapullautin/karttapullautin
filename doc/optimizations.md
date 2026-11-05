# Optimisations: rendering, knolls and contours

This document describes the changes of the `render-optimization` branch and their measurements. The aim was that processing one 3 × 3 km sheet takes less
time and memory, without changing any output. All numbers were measured on the same workstation (8 cores) with MML (Finnish National Land Survey) laser
scanning sheets from the Funet mirror, unless stated otherwise. **Estimates are marked as estimates.**

## Summary

| Measure | Before | Now |
|---|---|---|
| Large sheet (`M4211E3`, 8.1 M points), whole run | 24.2 s | 8.0 s (`vector_output`) / 9.9 s (normal run) |
| Smaller sheet (`K3414B1`, 4.3 M points), whole run | 22.8 s, 1.40 GB | 11.1 s, 0.45 GB |
| Peak memory, large sheet | 466 MB | 315 MB (`vector_output`) |
| Water cells for a vector pipeline | 9.2 s | 0.16 s |
| Large sheet (`M4122F4`, 8.2 M points), `vector_output` | 7.26 s | 6.0 s (the contour change, section 6) |

The outputs are **byte-identical** to the original binary: both PNG maps and all files of `temp/` (checked with `cmp`).

## Where the time went

Karttapullautin already has `Timing` sections (`RUST_LOG=debug`). With the original binary, 13.8 s of the 22.8 s of the small sheet (4.3 M points) went to
rendering (7.4 s + 6.4 s: the map with and the map without depressions). Of the 24.2 s of the large sheet (8.1 M points), the knoll detection
(`knoll detection part 2`) took 14.6 s.

Every fix was preceded by a measurement of where the time really goes. Two initial assumptions turned out to be wrong:

- **PNG decoding was not slow** (50-70 ms per file). What was slow was `resize` of the `image` crate (3.4 s for a 3003 → 7093 px scaling).
- **Drawing the curves was not slow** (under 0.1 s). I had assumed that stamping every point 25-36 times would be the bottleneck, but the measurement did
  not support it.

## Changes

### 1. Rendering is shared by the two maps (`60d5f5a`)

Karttapullautin writes two PNG maps (with and without depressions). Each call decoded and scaled the same layers and computed the slope of the height
field from scratch. Now:

- `render_both` builds the base (vegetation, undergrowth, low layer, north lines) and reads the height field and the slope once (`load_curve_inputs`). The
  map with depressions is made first because it writes `formlines.dxf.bin`.
- Decoded PNGs are released right after use (`into_rgba8()`, no copies).
- New ini setting `render_nodepressions` (default `1`, as before). With `0` only the map with depressions is made.
- `Timing` sections in `render_both`.

Result (`K3414B1`): 22.8 s → 19.7 s, 1.40 GB → 1.07 GB. With `render_nodepressions=0`: 16.4 s and 875 MB.

### 2. Nearest-neighbour scaling without the f32 intermediate (`60d5f5a`)

`image::imageops::resize` uses an f32 intermediate image (`Rgba32F`) whatever the filter. For 3003² → 7093² it allocated about 340 MB extra and took
3.4 s. With the nearest filter every output pixel is one source pixel with weight 1, so the value stays an integer. The replacement computes the source
pixel with the same f32 formula (`floor((x + 0.5) * ratio)`) and copies straight into a u8 buffer. Unit tests compare the result with the crate's for
several sizes and for a cropped source area.

Result: 19.7 s → 11.0 s, 1.07 GB → 0.70 GB. Base layers 3.8 s → 0.96 s, overlaid layers 3.0 s → 0.3 s per map.

### 3. Layers straight onto the canvas (`e5311fe`)

- `NearestView` (implements `GenericImageView`) reads the source pixels only when they are drawn, so no scaled copies are made.
  `image::imageops::overlay` uses it as it is, so the blending is exactly the same.
- `undergrowth.png` is blended row by row with the `png` crate when its size matches the canvas (otherwise the old path is used). The whole image is never
  held decoded (about 200 MB saved).
- The base of the map without depressions is rebuilt (about 0.4 s) instead of keeping a copy (about 200 MB).

Result: 703 MB → 452 MB, with the total time staying at 11 s. Other sheets (old → new): `L3313A4` 17.3 → 5.3 s and 1.37 → 0.45 GB, `R4233C4` 15.9 → 4.8 s
and 1.34 → 0.44 GB, `V4133E4` 28.7 → 18.1 s and 1.42 → 0.51 GB.

### 4. Knot detection: no more all-pairs loops (`71617b9`)

On the large sheet the knoll detection took 14.6 s. The added `Timing` sections (`knolldetector`) located two loops (`bounding boxes and tops` 6.5 s and
`candidates` 8.1 s) in which every line was compared with every point and the height was looked up twice from a hash map. The other parts take under 0.3 s
together.

- `elevation` is now a `Vec<f64>` indexed by the line (instead of a hash map).
- `PointGrid` (a regular 100 m grid) finds the points inside a bounding box. The result is sorted in the original list order, so the first hit (`topid`)
  and the "any hit" (`skip`) are the same as before.

Result: both loops take about 20 ms. Large sheet 24.2 s → 9.9 s. All temp files and PNGs byte-identical.

### 5. The `vector_output` mode (`1ad340b`)

A vector pipeline needs neither the PNG maps nor all the text DXF files. The ini setting `vector_output = 1` (use it with `output_dxf=0`):

- **skips the PNG rendering.** The form lines (`formlines.dxf.bin`) are still made by `draw_curves`, without an image.
- **computes the water cells in karttapullautin** (`vegetation::watercells` → `temp/water.txt`): 3 m cells that contain water-class points, one line
  `x y water total` per cell. The earlier way converted the points to text (`internal2xyz`) and counted with `awk`: 9.2 s → 0.16 s, with the same 2994 cells.
- **writes text DXF of only five layers** (`out2`, `formlines`, `c2g`, `c3g`, `dotknolls`). The DXFs of the other steps are left out: `contours03.dxf` alone was
  144.6 MB of text (the others: `out.dxf` 15.9 MB, `out2.dxf` 19.7 MB, `formlines.dxf` 6.9 MB).

Result (large sheet): 9.9 s → 8.0 s, 466 MB → 315 MB. The five DXF files and `vegetation.png` are byte-identical to the normal run.

### 6. Contours: visit only the cells a level crosses (`3b1af02`)

Profiling a large sheet (8.2 M points, 6.0 s in all) showed that `knoll detection part 1` (2.43 s) is in practice one call: `heightmap2contours` with a 0.3 m
interval (2.6 s; reading the points takes only 0.18 s). `grid2contours` scanned the whole grid of 2.25 M cells for every contour level (208 of them), although
most cells do not cross a given level.

The cells are now bucketed by the levels between their lowest and highest corner before the loop, and visited in the same i, j order as a full scan would.
The levels are computed with the same `level += v` summation as before. The result is **byte-identical** (11 files compared: all `.dxf.bin` and `.hmap`).

| | Before | Now |
|---|---|---|
| `knoll detection part 1` | 2.43 s | 1.23 s |
| `contour generation part 2` (2.5 m contours) | 0.30 s | 0.17 s |
| whole sheet (`vector_output`) | 7.26 s | 6.0 s |

What remains of `heightmap2contours` is the segment search (0.58 s, 8.1 M end points) and chaining the polylines through a hash map (0.31 s). They were not
changed; possible follow-ups are running the levels in parallel and replacing the hash map with a grid index.

## Time spent after these changes (large sheet, `vector_output`, 6.0 s)

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

There is no parallelism inside one sheet except the `image` crate's rayon in the PNG rendering (which `vector_output` skips) and the optional parallel LAZ
decompression (`parallel_laz_decompression`). One sheet uses about 1.5 of 8 cores; `processes` runs several sheets at the same time.

## Checks and how to repeat them

- `cargo fmt --check`, `cargo clippy --release` (no warnings) and `cargo test --release` (70 tests) pass.
- New unit tests: `nearest_view_matches_image_crate` and `nearest_view_uses_only_the_cropped_part` (`src/render.rs`).
- Comparison: run the original and the new binary on the same LAZ file in different folders and `cmp` `pullautus.png`, `pullautus_depr.png` and all files of
  `temp/`. In the vector mode compare `out2.dxf`, `formlines.dxf`, `c2g.dxf`, `c3g.dxf`, `dotknolls.dxf` and `vegetation.png`.
- Read the times with `RUST_LOG=debug pullauta <file>.laz` and the `Leaving section` lines.

## What was not done

- GPU computing was not tried: the heavy steps are branching geometry, and the measurements put the bottlenecks elsewhere (loops, copies).
- The `thinfactor` setting (thinning of the points) was not used, because it would change the result.
- The stamping of the curves was not changed, because the measurement did not show it to be a bottleneck.

## Commits

| Commit | Content |
|---|---|
| `60d5f5a` | `render_both`, `render_nodepressions`, `resize_nearest` |
| `e5311fe` | `NearestView`, row by row undergrowth, rebuilding the base |
| `71617b9` | knoll detection: `PointGrid` and `Vec` elevations |
| `1ad340b` | `vector_output`, `watercells`, DXF of the final layers only |
| `3b1af02` | contours: cells bucketed by level |
