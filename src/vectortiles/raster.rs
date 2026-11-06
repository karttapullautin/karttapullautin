//! Raster operations on the vegetation class grid: the mosaic of the sheets, the block mode filter, binary morphology, connected components
//! and tracing the outline of a mask.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

pub const BACKGROUND: u8 = 0;
pub const OPEN: u8 = 1;
/// Class of the lightest green; the denser shades follow (class - 1 is the green level 1, 2, ...)
pub const FIRST_GREEN: u8 = 2;

/// Palette indices of pullautin's vegetation.png (palette.rs): yellow, and the first of the green shades
const PALETTE_YELLOW: u8 = 3;
const PALETTE_FIRST_GREEN: u8 = 16;
const PALETTE_GREEN_SHADES: u8 = 64;

/// Size of a water cell in metres (water.txt)
pub const WATER_CELL_M: f64 = 3.0;

pub struct Mosaic {
    /// Map coordinates of the top-left corner of the mosaic (1 m pixels, north up)
    pub x0: i64,
    pub y0: i64,
    pub width: usize,
    pub height: usize,
    pub classes: Vec<u8>,
    /// the 3 m cells with water, counted from the top-left corner of the mosaic
    pub water: Vec<(i64, i64)>,
}

fn class_of_palette_index(i: u8) -> u8 {
    if i == PALETTE_YELLOW {
        OPEN
    } else if (PALETTE_FIRST_GREEN..PALETTE_FIRST_GREEN + PALETTE_GREEN_SHADES).contains(&i) {
        FIRST_GREEN + (i - PALETTE_FIRST_GREEN)
    } else {
        BACKGROUND
    }
}

struct Sheet {
    x0: i64,
    y0: i64,
    width: usize,
    height: usize,
    classes: Vec<u8>,
    water: Vec<(i64, i64)>,
}

/// The world file of vegetation.pgw: 1 m pixels, north up; returns the map coordinates of the top-left corner
fn read_pgw(path: &Path) -> anyhow::Result<(i64, i64)> {
    let text = std::fs::read_to_string(path)?;
    let v: Vec<f64> = text
        .split_whitespace()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    anyhow::ensure!(v.len() >= 6, "{} has fewer than 6 lines", path.display());
    anyhow::ensure!(
        v[0] == 1.0 && v[3] == -1.0,
        "{}: pixels must be 1 m, north up",
        path.display()
    );
    Ok((v[4].round() as i64, v[5].round() as i64))
}

/// Where the files of a pullautin run are (`vegetation.png`, the `.dxf.bin` layers ...): the `temp` folder of the run folder, or the folder itself when it
/// has no `temp` folder (a `temp`, `temp1` ... folder of the working directory, or one saved by the batch mode's `savetempfolders`: `temp_<name>_dir`)
pub fn temp_of(folder: &Path) -> PathBuf {
    let temp = folder.join("temp");
    if temp.is_dir() {
        temp
    } else {
        folder.to_path_buf()
    }
}

/// Whether a folder is a pullautin run: it has a `temp` folder, or it is itself a temp folder (it has the `vegetation.png` of a `vector_output = 1` run)
pub fn is_run(folder: &Path) -> bool {
    folder.join("temp").is_dir() || folder.join("vegetation.png").is_file()
}

/// The map coordinates of a sheet's raster: (min x, min y, max x, max y), from `vegetation.pgw` and the size in the PNG header
pub fn sheet_bounds(folder: &Path) -> anyhow::Result<(f64, f64, f64, f64)> {
    let temp = temp_of(folder);
    let (x0, y0) = read_pgw(&temp.join("vegetation.pgw"))?;
    let reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(
        temp.join("vegetation.png"),
    )?))
    .read_info()?;
    let (w, h) = (reader.info().width as f64, reader.info().height as f64);
    Ok((x0 as f64, y0 as f64 - h, x0 as f64 + w, y0 as f64))
}

fn read_sheet(folder: &Path) -> anyhow::Result<Sheet> {
    let temp = temp_of(folder);
    let (x0, y0) = read_pgw(&temp.join("vegetation.pgw"))?;

    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(
        temp.join("vegetation.png"),
    )?));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info()?;
    let (color, depth) = reader.output_color_type();
    anyhow::ensure!(
        color == png::ColorType::Indexed && depth == png::BitDepth::Eight,
        "{}: vegetation.png must be an 8-bit indexed PNG",
        folder.display()
    );
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    let mut buffer = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or_else(|| anyhow::anyhow!("vegetation.png is too large"))?
    ];
    let frame = reader.next_frame(&mut buffer)?;
    buffer.truncate(frame.buffer_size());
    let classes = buffer.iter().map(|&i| class_of_palette_index(i)).collect();

    // water.txt: "x y water total" per 3 m cell, from the top-left corner of this sheet
    let mut water = Vec::new();
    for path in [temp.join("water.txt"), folder.join("water.txt")] {
        if path.exists() {
            for line in std::fs::read_to_string(&path)?.lines() {
                let mut f = line.split_whitespace();
                if let (Some(cx), Some(cy), Some(w)) = (f.next(), f.next(), f.next())
                    && w.parse::<u32>()? >= 1
                {
                    water.push((cx.parse()?, cy.parse()?));
                }
            }
            break;
        }
    }
    Ok(Sheet {
        x0,
        y0,
        width,
        height,
        classes,
        water,
    })
}

/// Joins the vegetation of the sheets (in the order given: a later sheet overwrites an earlier one where they overlap) into one grid.
pub fn read_mosaic(folders: &[PathBuf]) -> anyhow::Result<Mosaic> {
    let sheets: Vec<Sheet> = folders
        .iter()
        .map(|f| read_sheet(f))
        .collect::<anyhow::Result<_>>()?;
    anyhow::ensure!(!sheets.is_empty(), "no sheets");
    let x_min = sheets.iter().map(|s| s.x0).min().unwrap();
    let y_max = sheets.iter().map(|s| s.y0).max().unwrap();
    let x_max = sheets.iter().map(|s| s.x0 + s.width as i64).max().unwrap();
    let y_min = sheets.iter().map(|s| s.y0 - s.height as i64).min().unwrap();
    let (width, height) = ((x_max - x_min) as usize, (y_max - y_min) as usize);

    let mut classes = vec![BACKGROUND; width * height];
    let mut water = Vec::new();
    for s in &sheets {
        let (left, top) = ((s.x0 - x_min) as usize, (y_max - s.y0) as usize);
        for row in 0..s.height {
            let dst = (top + row) * width + left;
            classes[dst..dst + s.width]
                .copy_from_slice(&s.classes[row * s.width..(row + 1) * s.width]);
        }
        // the origins of the sheets differ by a metre or two, so the 3 m cells line up to within a cell
        let dx = ((s.x0 - x_min) as f64 / WATER_CELL_M).round() as i64;
        let dy = ((y_max - s.y0) as f64 / WATER_CELL_M).round() as i64;
        water.extend(s.water.iter().map(|&(cx, cy)| (cx + dx, cy + dy)));
    }
    Ok(Mosaic {
        x0: x_min,
        y0: y_max,
        width,
        height,
        classes,
        water,
    })
}

/// The most common class of every `block` x `block` pixels (ties go to the lower class). Returns the grid and its size.
pub fn block_mode(
    classes: &[u8],
    width: usize,
    height: usize,
    block: usize,
) -> (Vec<u8>, usize, usize) {
    let (wc, hc) = (width / block, height / block);
    let mut out = vec![0u8; wc * hc];
    let mut counts = [0u16; 256];
    for by in 0..hc {
        for bx in 0..wc {
            let mut present = [0u8; 16];
            let mut n_present = 0usize;
            let mut seen_other = false;
            for dy in 0..block {
                let row = (by * block + dy) * width + bx * block;
                for &c in &classes[row..row + block] {
                    if counts[c as usize] == 0 {
                        if n_present < present.len() {
                            present[n_present] = c;
                            n_present += 1;
                        } else {
                            seen_other = true;
                        }
                    }
                    counts[c as usize] += 1;
                }
            }
            debug_assert!(
                !seen_other,
                "a block of {block}x{block} has at most {} classes",
                block * block
            );
            let (mut best, mut best_n) = (0u8, 0u16);
            for &c in &present[..n_present] {
                let n = counts[c as usize];
                if n > best_n || (n == best_n && c < best) {
                    best = c;
                    best_n = n;
                }
                counts[c as usize] = 0;
            }
            out[by * wc + bx] = best;
        }
    }
    (out, wc, hc)
}

/// Binary dilation or erosion with a k x k square (k odd); the outside of the array counts as false.
fn window_op(src: &[bool], w: usize, h: usize, k: usize, dilate: bool) -> Vec<bool> {
    let r = k / 2;
    let apply = |len: usize, get: &dyn Fn(usize) -> bool, set: &mut dyn FnMut(usize, bool)| {
        // prefix sums of the trues along one line
        let mut prefix = vec![0u32; len + 1];
        for i in 0..len {
            prefix[i + 1] = prefix[i] + get(i) as u32;
        }
        for i in 0..len {
            let (lo, hi) = (i as isize - r as isize, i + r);
            let count = prefix[(hi + 1).min(len)] - prefix[lo.max(0) as usize];
            let inside = lo >= 0 && hi < len;
            set(
                i,
                if dilate {
                    count > 0
                } else {
                    inside && count as usize == k
                },
            );
        }
    };
    // along x
    let mut tmp = vec![false; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut out_row = vec![false; w];
        apply(w, &|i| row[i], &mut |i, v| out_row[i] = v);
        tmp[y * w..(y + 1) * w].copy_from_slice(&out_row);
    }
    // along y
    let mut out = vec![false; w * h];
    let mut column_out = vec![false; h];
    for x in 0..w {
        apply(h, &|i| tmp[i * w + x], &mut |i, v| column_out[i] = v);
        for y in 0..h {
            out[y * w + x] = column_out[y];
        }
    }
    out
}

/// Binary dilation with a k x k square (k odd); the outside of the array counts as false.
pub fn dilate(src: &[bool], w: usize, h: usize, k: usize) -> Vec<bool> {
    window_op(src, w, h, k, true)
}

/// scipy.ndimage.binary_closing with a k x k square after `np.pad(mask, pad, mode="edge")`, cropped back: gaps up to k - 1 cells close,
/// and the edge of the array is not treated as empty.
pub fn closing_padded(mask: &[bool], w: usize, h: usize, k: usize, pad: usize) -> Vec<bool> {
    let (pw, ph) = (w + 2 * pad, h + 2 * pad);
    let mut padded = vec![false; pw * ph];
    for y in 0..ph {
        let sy = y.saturating_sub(pad).min(h - 1);
        for x in 0..pw {
            let sx = x.saturating_sub(pad).min(w - 1);
            padded[y * pw + x] = mask[sy * w + sx];
        }
    }
    let closed = window_op(&window_op(&padded, pw, ph, k, true), pw, ph, k, false);
    let mut out = vec![false; w * h];
    for y in 0..h {
        out[y * w..(y + 1) * w]
            .copy_from_slice(&closed[(y + pad) * pw + pad..(y + pad) * pw + pad + w]);
    }
    out
}

/// A 4-connected component of a mask: its bounding box (relative to the mask that was labelled) and a mask cropped to the box
pub struct Component {
    pub x0: usize,
    pub y0: usize,
    pub w: usize,
    pub h: usize,
    pub cells: usize,
    pub mask: Vec<bool>,
}

/// The 4-connected components, in the order of their first cell in raster order (like scipy.ndimage.label)
pub fn label(mask: &[bool], w: usize, h: usize) -> Vec<Component> {
    let mut visited = vec![false; w * h];
    let mut components = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut cells: Vec<u32> = Vec::new();
    for start in 0..w * h {
        if !mask[start] || visited[start] {
            continue;
        }
        cells.clear();
        stack.push(start as u32);
        visited[start] = true;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (x, y) = (i as usize % w, i as usize / w);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            let mut visit = |j: usize| {
                if mask[j] && !visited[j] {
                    visited[j] = true;
                    stack.push(j as u32);
                }
            };
            if x > 0 {
                visit(i as usize - 1);
            }
            if x + 1 < w {
                visit(i as usize + 1);
            }
            if y > 0 {
                visit(i as usize - w);
            }
            if y + 1 < h {
                visit(i as usize + w);
            }
        }
        let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut crop = vec![false; cw * ch];
        for &i in &cells {
            let (x, y) = (i as usize % w, i as usize / w);
            crop[(y - y0) * cw + (x - x0)] = true;
        }
        components.push(Component {
            x0,
            y0,
            w: cw,
            h: ch,
            cells: cells.len(),
            mask: crop,
        });
    }
    components
}

pub type Ring = Vec<(i32, i32)>;

pub struct TracedPolygon {
    pub exterior: Ring,
    pub holes: Vec<Ring>,
}

fn area2(ring: &[(i32, i32)]) -> i64 {
    let mut s = 0i64;
    for i in 0..ring.len() {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % ring.len()];
        s += x0 as i64 * y1 as i64 - x1 as i64 * y0 as i64;
    }
    s
}

fn point_in_ring(p: (f64, f64), ring: &[(i32, i32)]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (ax, ay) = (ring[i].0 as f64, ring[i].1 as f64);
        let (bx, by) = (
            ring[(i + 1) % ring.len()].0 as f64,
            ring[(i + 1) % ring.len()].1 as f64,
        );
        if (ay > p.1) != (by > p.1) && p.0 < (bx - ax) * (p.1 - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    inside
}

/// The outline of a mask as polygons with holes, with corners at the cell corners (x right, y down). Cells that touch only at a corner are
/// not joined (the mask is read 4-connected).
pub fn trace(mask: &[bool], w: usize, h: usize) -> Vec<TracedPolygon> {
    let on = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && mask[y as usize * w + x as usize]
    };
    // directed edges with the cell on the right (y down: clockwise on screen around an exterior)
    let mut edges: FxHashMap<(i32, i32), Vec<(i32, i32)>> = FxHashMap::default();
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !on(x, y) {
                continue;
            }
            let (xi, yi) = (x as i32, y as i32);
            if !on(x, y - 1) {
                edges.entry((xi, yi)).or_default().push((xi + 1, yi));
            }
            if !on(x + 1, y) {
                edges
                    .entry((xi + 1, yi))
                    .or_default()
                    .push((xi + 1, yi + 1));
            }
            if !on(x, y + 1) {
                edges
                    .entry((xi + 1, yi + 1))
                    .or_default()
                    .push((xi, yi + 1));
            }
            if !on(x - 1, y) {
                edges.entry((xi, yi + 1)).or_default().push((xi, yi));
            }
        }
    }

    let mut rings: Vec<Ring> = Vec::new();
    // start from vertices with one way out first, so that a ring never starts at a corner shared with another ring
    let mut starts: Vec<(i32, i32)> = edges.keys().copied().collect();
    starts.sort_unstable_by_key(|v| (edges[v].len(), v.1, v.0));
    for start in starts {
        while edges.get(&start).is_some_and(|e| !e.is_empty()) {
            let mut ring = vec![start];
            let mut v = start;
            let mut prev_dir: Option<(i32, i32)> = None;
            loop {
                let outs = edges
                    .get_mut(&v)
                    .expect("every vertex of a ring has a way out");
                let pick = if outs.len() == 1 {
                    0
                } else {
                    // two ways out at a corner shared by two cells: turn right, around the cell the ring has been following
                    let (dx, dy) = prev_dir.expect("a ring starts at a vertex with one way out");
                    let right = (-dy, dx);
                    outs.iter()
                        .position(|o| (o.0 - v.0, o.1 - v.1) == right)
                        .unwrap_or(0)
                };
                let next = outs.swap_remove(pick);
                prev_dir = Some((next.0 - v.0, next.1 - v.1));
                v = next;
                if v == start {
                    break;
                }
                ring.push(v);
            }
            // keep only the corners: drop the vertices in the middle of a straight run
            let n = ring.len();
            let mut corners = Vec::with_capacity(n);
            for i in 0..n {
                let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
                if (b.0 - a.0, b.1 - a.1) != (c.0 - b.0, c.1 - b.1) {
                    corners.push(b);
                }
            }
            rings.push(corners);
        }
    }

    let mut polygons: Vec<TracedPolygon> = Vec::new();
    let mut holes: Vec<Ring> = Vec::new();
    for ring in rings {
        if area2(&ring) > 0 {
            polygons.push(TracedPolygon {
                exterior: ring,
                holes: Vec::new(),
            });
        } else {
            holes.push(ring);
        }
    }
    for hole in holes {
        // a point just inside the hole's first edge identifies the exterior it belongs to
        let (a, b) = (hole[0], hole[1 % hole.len()]);
        let probe = ((a.0 + b.0) as f64 / 2.0, (a.1 + b.1) as f64 / 2.0);
        let owner = polygons
            .iter()
            .position(|p| point_in_ring(probe, &p.exterior))
            .unwrap_or(0);
        polygons[owner].holes.push(hole);
    }
    polygons
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_of(rows: &[&str]) -> (Vec<bool>, usize, usize) {
        let h = rows.len();
        let w = rows[0].len();
        (
            rows.iter()
                .flat_map(|r| r.chars().map(|c| c == '#'))
                .collect(),
            w,
            h,
        )
    }

    fn rows_of(mask: &[bool], w: usize) -> Vec<String> {
        mask.chunks(w)
            .map(|r| r.iter().map(|&b| if b { '#' } else { '.' }).collect())
            .collect()
    }

    #[test]
    fn block_mode_takes_the_most_common_class_and_the_lower_on_a_tie() {
        // 4x2 pixels, blocks of 2: left block 1,1,0,2 -> class 1 (twice); right block 3,3,2,2 -> tie of 2 and 3 -> 2
        let classes = [1, 1, 3, 3, 0, 2, 2, 2];
        let (grid, wc, hc) = block_mode(&classes, 4, 2, 2);
        assert_eq!((wc, hc), (2, 1));
        assert_eq!(grid, vec![1, 2]);
    }

    #[test]
    fn closing_fills_gaps_narrower_than_the_square_and_keeps_the_edge() {
        // scipy: np.pad(edge, 3), binary_closing(5x5), crop, for this mask (computed with scipy 1.17)
        let (mask, w, h) = mask_of(&[
            "###...##.#",
            "###...##..",
            "..........",
            "...#..#...",
            "...#..#...",
            "...#..#...",
            "..........",
            "#........#",
            "#........#",
            "#........#",
        ]);
        let expected = [
            "##########",
            "########..",
            "...####...",
            "...####...",
            "...####...",
            "...####...",
            "..........",
            "#........#",
            "#........#",
            "#........#",
        ];
        let got = rows_of(&closing_padded(&mask, w, h, 5, 5), w);
        assert_eq!(got, expected.map(String::from).to_vec());
    }

    #[test]
    fn labels_are_4_connected_and_in_raster_order() {
        let (mask, w, h) = mask_of(&["#..#", ".#..", "...#", "##.#"]);
        let comps = label(&mask, w, h);
        // the diagonal neighbours do not join: (0,0), (3,0), (1,1), the right column from (3,2), and (0,3)-(1,3)
        let sizes: Vec<_> = comps.iter().map(|c| (c.x0, c.y0, c.cells)).collect();
        assert_eq!(
            sizes,
            vec![(0, 0, 1), (3, 0, 1), (1, 1, 1), (3, 2, 2), (0, 3, 2)]
        );
    }

    #[test]
    fn traces_a_square_with_a_hole() {
        let (mask, w, h) = mask_of(&["####", "#..#", "#..#", "####"]);
        let polygons = trace(&mask, w, h);
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].exterior.len(), 4);
        assert_eq!(area2(&polygons[0].exterior), 32);
        assert_eq!(polygons[0].holes.len(), 1);
        assert_eq!(area2(&polygons[0].holes[0]), -8);
    }

    #[test]
    fn cells_that_touch_at_a_corner_are_separate_polygons() {
        let (mask, w, h) = mask_of(&["#.", ".#"]);
        let polygons = trace(&mask, w, h);
        assert_eq!(polygons.len(), 2);
        assert!(
            polygons
                .iter()
                .all(|p| p.exterior.len() == 4 && area2(&p.exterior) == 2)
        );
    }

    #[test]
    fn an_l_shape_has_six_corners() {
        let (mask, w, h) = mask_of(&["#.", "##"]);
        let polygons = trace(&mask, w, h);
        assert_eq!(polygons[0].exterior.len(), 6);
        assert_eq!(area2(&polygons[0].exterior), 6);
    }
}
