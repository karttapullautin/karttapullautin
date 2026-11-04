//! Vegetation as polygons: yellow open land and nested greens, from the class grid of the vegetation image.
//!
//! The steps (the same as proxy-server/mapant-tiles/vegetation_polygons.py):
//!
//! - the most common class of every 3 m block
//! - water cells (and the gaps between them) are left out
//! - open land is closed over gaps up to 15 m
//! - 4-connected areas smaller than 6 x 6 m are dropped
//! - a green area is drawn in its lightest level, and a denser area on top of it only where it is at least two levels denser (the
//!   polygons are nested)
//! - holes under 36 m2 are filled, the rings are smoothed and simplified, and a denser area is clipped to its parent

use super::features::{PolygonFeature, Props};
use super::mvt::Value;
use super::project::Tm35finToMercator;
use super::raster::{self, BACKGROUND, FIRST_GREEN, Mosaic, OPEN};
use geo::{Area, BooleanOps, Coord, LineString, MultiPolygon, Polygon};

pub struct Params {
    /// size of the grid cells in metres (pixels of the vegetation image)
    pub block: usize,
    /// areas and holes under this many square metres go
    pub min_area: f64,
    /// a denser green is drawn over a lighter one only when it is this many levels denser
    pub min_step: u8,
    pub simplify_m: f64,
    pub smooth_sigma_m: f64,
    pub smooth_step_m: f64,
    /// open land is closed over gaps narrower than this many cells (0 = off)
    pub close_open_cells: usize,
    /// the gaps between water cells that close, in cells
    pub water_close_cells: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            block: 3,
            min_area: 36.0,
            min_step: 2,
            simplify_m: 2.0,
            smooth_sigma_m: 6.0,
            smooth_step_m: 1.5,
            close_open_cells: 5,
            water_close_cells: 9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Label {
    Open,
    /// the level of the lightest cell of the area (1 = the lightest green)
    Green(u8),
}

struct Emitted {
    label: Label,
    /// cell offset of the component mask in the grid
    ox: usize,
    oy: usize,
    mask: raster::Component,
    parent: Option<usize>,
}

/// A mask of the grid (or of a part of it) at an offset
struct Region {
    ox: usize,
    oy: usize,
    w: usize,
    h: usize,
    mask: Vec<bool>,
}

type PixelRing = Vec<(f64, f64)>;

fn closed_ring(points: impl Iterator<Item = (f64, f64)>) -> PixelRing {
    let mut ring: PixelRing = points.collect();
    if let Some(&first) = ring.first() {
        ring.push(first);
    }
    ring
}

fn ring_area(ring: &[(f64, f64)]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].0 * w[1].1 - w[1].0 * w[0].1;
    }
    s.abs() / 2.0
}

/// Resamples a closed ring every `step` metres and low-pass filters it along its length (a Gaussian of `sigma_m`), which takes the
/// stair steps of the grid out. Rings too short to resample are returned as they are.
fn smooth_ring(ring: &[(f64, f64)], sigma_m: f64, step: f64) -> PixelRing {
    let pts = &ring[..ring.len() - 1];
    let n = pts.len();
    let mut cum = Vec::with_capacity(n + 1);
    cum.push(0.0);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        cum.push(cum[i] + (b.0 - a.0).hypot(b.1 - a.1));
    }
    let total = cum[n];
    let samples = (total / step) as usize;
    if samples < 12 {
        return ring.to_vec();
    }
    let mut xs = Vec::with_capacity(samples);
    let mut ys = Vec::with_capacity(samples);
    let mut segment = 0usize;
    for i in 0..samples {
        let t = total * i as f64 / samples as f64;
        while segment + 1 < n && cum[segment + 1] <= t {
            segment += 1;
        }
        let span = cum[segment + 1] - cum[segment];
        let f = if span > 0.0 {
            (t - cum[segment]) / span
        } else {
            0.0
        };
        let (a, b) = (pts[segment], pts[(segment + 1) % n]);
        xs.push(a.0 + (b.0 - a.0) * f);
        ys.push(a.1 + (b.1 - a.1) * f);
    }

    // scipy.ndimage.gaussian_filter1d(mode="wrap"): truncate 4 sigma, weights normalised
    let sigma = sigma_m / step;
    let radius = (4.0 * sigma + 0.5) as isize;
    let mut weights: Vec<f64> = (-radius..=radius)
        .map(|x| (-0.5 * (x as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = weights.iter().sum();
    weights.iter_mut().for_each(|w| *w /= sum);
    let len = samples as isize;
    let filtered = |v: &[f64], i: usize| -> f64 {
        weights
            .iter()
            .enumerate()
            .map(|(k, w)| w * v[(i as isize + k as isize - radius).rem_euclid(len) as usize])
            .sum()
    };
    closed_ring((0..samples).map(|i| (filtered(&xs, i), filtered(&ys, i))))
}

fn polygon_of(exterior: PixelRing, holes: Vec<PixelRing>) -> Polygon<f64> {
    let line = |r: PixelRing| {
        LineString::from(
            r.into_iter()
                .map(|(x, y)| Coord { x, y })
                .collect::<Vec<_>>(),
        )
    };
    Polygon::new(line(exterior), holes.into_iter().map(line).collect())
}

fn ring_of(line: &LineString<f64>) -> PixelRing {
    line.coords().map(|c| (c.x, c.y)).collect()
}

/// Simplified ring (closed), None when it does not have the four vertices of a ring
fn simplify_ring(ring: &[(f64, f64)], epsilon: f64) -> Option<PixelRing> {
    let simplified = super::simplify::simplify_ring(ring, epsilon);
    (simplified.len() >= 4).then_some(simplified)
}

fn simplify_polygon(polygon: &Polygon<f64>, epsilon: f64) -> Option<Polygon<f64>> {
    let exterior = simplify_ring(&ring_of(polygon.exterior()), epsilon)?;
    let holes = polygon
        .interiors()
        .iter()
        .filter_map(|h| simplify_ring(&ring_of(h), epsilon))
        .collect();
    Some(polygon_of(exterior, holes))
}

/// The grid of classes after the block filter, the removal of water and the closing of open land
struct Grid {
    classes: Vec<u8>,
    w: usize,
    h: usize,
}

impl Grid {
    fn from_mosaic(mosaic: &Mosaic, params: &Params) -> Self {
        let (mut classes, w, h) =
            raster::block_mode(&mosaic.classes, mosaic.width, mosaic.height, params.block);

        let mut water = vec![false; w * h];
        for &(cx, cy) in &mosaic.water {
            if cx >= 0 && cy >= 0 && (cx as usize) < w && (cy as usize) < h {
                water[cy as usize * w + cx as usize] = true;
            }
        }
        // sea surfaces return water points sparsely: close the gaps between them. The padding is 6 cells like the Python version.
        if params.water_close_cells > 1 {
            water = raster::closing_padded(&water, w, h, params.water_close_cells, 6);
        }
        for (c, &wet) in classes.iter_mut().zip(&water) {
            if wet {
                *c = BACKGROUND;
            }
        }

        if params.close_open_cells > 1 {
            let open: Vec<bool> = classes.iter().map(|&c| c == OPEN).collect();
            let closed = raster::closing_padded(
                &open,
                w,
                h,
                params.close_open_cells,
                params.close_open_cells,
            );
            for i in 0..w * h {
                if closed[i] && classes[i] == BACKGROUND && !water[i] {
                    classes[i] = OPEN;
                }
            }
        }
        Grid { classes, w, h }
    }

    /// green level of a cell: 1 = the lightest, 0 = not green
    fn level(&self, i: usize) -> u8 {
        if self.classes[i] >= FIRST_GREEN {
            self.classes[i] - FIRST_GREEN + 1
        } else {
            0
        }
    }
}

struct Collector<'a> {
    grid: &'a Grid,
    params: &'a Params,
    emitted: Vec<Emitted>,
}

impl Collector<'_> {
    fn too_small(&self, cells: usize) -> bool {
        ((cells * self.params.block * self.params.block) as f64) < self.params.min_area
    }

    fn green(&mut self, region: Region, parent: Option<usize>) {
        let (rw, rh) = (region.w, region.h);
        for comp in raster::label(&region.mask, rw, rh) {
            if self.too_small(comp.cells) {
                continue;
            }
            let (ox, oy) = (region.ox + comp.x0, region.oy + comp.y0);
            // the level of the lightest cell
            let mut lightest = u8::MAX;
            for y in 0..comp.h {
                for x in 0..comp.w {
                    if comp.mask[y * comp.w + x] {
                        lightest = lightest.min(self.grid.level((oy + y) * self.grid.w + ox + x));
                    }
                }
            }
            // a denser area on top: where the level is at least min_step above the lightest
            let sub_mask: Vec<bool> = (0..comp.h * comp.w)
                .map(|i| {
                    let (x, y) = (i % comp.w, i / comp.w);
                    comp.mask[i]
                        && self.grid.level((oy + y) * self.grid.w + ox + x)
                            >= lightest + self.params.min_step
                })
                .collect();
            let any_denser = sub_mask.iter().any(|&b| b);
            let (w, h) = (comp.w, comp.h);

            let index = self.emitted.len();
            self.emitted.push(Emitted {
                label: Label::Green(lightest),
                ox,
                oy,
                mask: comp,
                parent,
            });
            if any_denser {
                self.green(
                    Region {
                        ox,
                        oy,
                        w,
                        h,
                        mask: sub_mask,
                    },
                    Some(index),
                );
            }
        }
    }
}

/// The vegetation polygons of a mosaic, in map coordinates of the mosaic's CRS and then Web Mercator metres
pub fn polygons(
    mosaic: &Mosaic,
    params: &Params,
    projection: &Tm35finToMercator,
) -> Vec<PolygonFeature> {
    let grid = Grid::from_mosaic(mosaic, params);
    let mut collector = Collector {
        grid: &grid,
        params,
        emitted: Vec::new(),
    };

    // open land first, no nesting
    let open: Vec<bool> = grid.classes.iter().map(|&c| c == OPEN).collect();
    for comp in raster::label(&open, grid.w, grid.h) {
        if collector.too_small(comp.cells) {
            continue;
        }
        let (ox, oy) = (comp.x0, comp.y0);
        collector.emitted.push(Emitted {
            label: Label::Open,
            ox,
            oy,
            mask: comp,
            parent: None,
        });
    }
    // then the greens, nested
    let green: Vec<bool> = (0..grid.w * grid.h).map(|i| grid.level(i) >= 1).collect();
    collector.green(
        Region {
            ox: 0,
            oy: 0,
            w: grid.w,
            h: grid.h,
            mask: green,
        },
        None,
    );

    let b = params.block as f64;
    let mut smoothed: Vec<MultiPolygon<f64>> = Vec::with_capacity(collector.emitted.len());
    let mut features = Vec::new();
    let empty = MultiPolygon::<f64>::new(Vec::new());
    for e in &collector.emitted {
        // the outline in metres, y down, from the top-left corner of the mosaic
        let to_px = |&(x, y): &(i32, i32)| {
            (
                ((e.ox as i32 + x) as f64) * b,
                ((e.oy as i32 + y) as f64) * b,
            )
        };
        let mut parts: Vec<Polygon<f64>> = Vec::new();
        for traced in raster::trace(&e.mask.mask, e.mask.w, e.mask.h) {
            let exterior = closed_ring(traced.exterior.iter().map(to_px));
            let holes: Vec<PixelRing> = traced
                .holes
                .iter()
                .map(|h| closed_ring(h.iter().map(to_px)))
                .filter(|h| ring_area(h) >= params.min_area)
                .collect();
            let exterior = smooth_ring(&exterior, params.smooth_sigma_m, params.smooth_step_m);
            let holes = holes
                .iter()
                .map(|h| smooth_ring(h, params.smooth_sigma_m, params.smooth_step_m))
                .collect();
            // the union with nothing makes the polygon valid where the smoothing made a ring cross itself
            parts.extend(polygon_of(exterior, holes).union(&empty).0);
        }
        let mut geometry = MultiPolygon::new(parts);
        if let Some(parent) = e.parent {
            geometry = geometry.intersection(&smoothed[parent]);
        }
        smoothed.push(geometry.clone());

        let props: Props = match e.label {
            Label::Open => vec![
                ("kind", Value::Str("open".into())),
                ("level", Value::Int(0)),
            ],
            Label::Green(level) => vec![
                ("kind", Value::Str("green".into())),
                ("level", Value::Int(level as i64)),
            ],
        };
        for polygon in geometry.0 {
            let Some(simplified) = simplify_polygon(&polygon, params.simplify_m) else {
                continue;
            };
            if simplified.unsigned_area() < params.min_area * 0.5 {
                continue;
            }
            // pixel metres -> map coordinates of the mosaic (north up) -> Web Mercator
            let to_world = |c: &Coord<f64>| -> Coord<f64> {
                let (x, y) = projection.to_mercator(mosaic.x0 as f64 + c.x, mosaic.y0 as f64 - c.y);
                Coord { x, y }
            };
            let map_ring = |line: &LineString<f64>| {
                LineString::from(line.coords().map(to_world).collect::<Vec<_>>())
            };
            features.push(PolygonFeature {
                polygon: Polygon::new(
                    map_ring(simplified.exterior()),
                    simplified.interiors().iter().map(map_ring).collect(),
                ),
                props: props.clone(),
            });
        }
    }
    if log::log_enabled!(log::Level::Debug) {
        let mut emitted_by_label = std::collections::BTreeMap::new();
        for e in &collector.emitted {
            let key = match e.label {
                Label::Open => 0,
                Label::Green(l) => l as i64,
            };
            *emitted_by_label.entry(key).or_insert(0usize) += 1;
        }
        let mut polygons_by_level = std::collections::BTreeMap::new();
        for f in &features {
            if let Value::Int(l) = f.props[1].1 {
                *polygons_by_level.entry(l).or_insert(0usize) += 1;
            }
        }
        log::debug!(
            "vegetation areas by level (0 = open): {emitted_by_label:?}; polygons by level: {polygons_by_level:?}"
        );
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_matches_scipy() {
        let ring = [
            (0.0, 0.0),
            (90.0, 0.0),
            (90.0, 30.0),
            (60.0, 30.0),
            (60.0, 60.0),
            (0.0, 60.0),
            (0.0, 0.0),
        ];
        let out = smooth_ring(&ring, 6.0, 1.5);
        assert_eq!(out.len(), 201);
        // from the Python version (resample + scipy gaussian_filter1d, mode wrap)
        for (i, x, y) in [
            (0, 2.3807682114684785, 2.3807682114684785),
            (1, 3.205572536116718, 1.705572536116718),
            (7, 10.59408969267887, 0.09408969267886669),
            (40, 60.00000000000001, 0.0),
            (55, 82.20249665689062, 0.2975033431094014),
            (100, 62.38076821146849, 32.380768211468485),
            (199, 1.705572536116718, 3.205572536116718),
        ] {
            assert!(
                (out[i].0 - x).abs() < 1e-9 && (out[i].1 - y).abs() < 1e-9,
                "sample {i}: {:?} vs ({x}, {y})",
                out[i]
            );
        }
        assert_eq!(out.first(), out.last());
    }

    #[test]
    fn a_ring_too_short_to_resample_is_left_alone() {
        // 16 m round: fewer than 12 samples of 1.5 m
        let ring = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)];
        assert_eq!(smooth_ring(&ring, 6.0, 1.5), ring.to_vec());
    }

    fn mosaic(rows: &[&str], water: Vec<(i64, i64)>) -> Mosaic {
        // 1 m pixels: every character is a 3 x 3 m block of one class: '.' background, 'y' open, '1'..'7' green levels
        let (h, w) = (rows.len() * 3, rows[0].len() * 3);
        let mut classes = vec![0u8; w * h];
        for (by, row) in rows.iter().enumerate() {
            for (bx, c) in row.chars().enumerate() {
                let class = match c {
                    'y' => OPEN,
                    '1'..='7' => FIRST_GREEN + (c as u8 - b'1'),
                    _ => BACKGROUND,
                };
                for dy in 0..3 {
                    for dx in 0..3 {
                        classes[(by * 3 + dy) * w + bx * 3 + dx] = class;
                    }
                }
            }
        }
        Mosaic {
            x0: 400_000,
            y0: 7_000_000,
            width: w,
            height: h,
            classes,
            water,
        }
    }

    fn no_closing() -> Params {
        Params {
            close_open_cells: 0,
            water_close_cells: 0,
            smooth_sigma_m: 0.001,
            ..Params::default()
        }
    }

    #[test]
    fn denser_green_is_nested_only_when_two_levels_denser_and_inside_the_lighter() {
        let rows = [
            "............",
            "............",
            "............",
            "..1111111...",
            "..1133311...",
            "..1155511...",
            "..1155511...",
            "..1111111...",
            "............",
        ];
        let m = mosaic(&rows, vec![]);
        let projection = Tm35finToMercator::new();
        let features = polygons(&m, &no_closing(), &projection);
        let level = |f: &PolygonFeature| match f.props[1].1 {
            Value::Int(l) => l,
            _ => panic!(),
        };
        let levels: Vec<i64> = features.iter().map(level).collect();
        // the light area (level 1); the 3s are 2 levels denser -> a patch of level 3, and inside it the 5s (2 more) -> a patch of level 5
        assert_eq!(levels, vec![1, 3, 5]);
        let area = |f: &PolygonFeature| f.polygon.unsigned_area();
        assert!(area(&features[0]) > area(&features[1]) && area(&features[1]) > area(&features[2]));
    }

    #[test]
    fn open_areas_and_small_areas() {
        // one open area of 3 x 2 blocks and a single block (9 m2 < 36 m2) that is dropped
        let m = mosaic(&["yyy..", "yyy.y", ".....", "....."], vec![]);
        let features = polygons(&m, &no_closing(), &Tm35finToMercator::new());
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].props[0].1, Value::Str("open".into()));
    }

    #[test]
    fn water_cells_leave_the_vegetation_out() {
        let rows = ["yyyyyy", "yyyyyy", "yyyyyy", "yyyyyy"];
        let dry = polygons(
            &mosaic(&rows, vec![]),
            &no_closing(),
            &Tm35finToMercator::new(),
        );
        let wet = polygons(
            &mosaic(
                &rows,
                (0..3).flat_map(|y| (0..2).map(move |x| (x, y))).collect(),
            ),
            &no_closing(),
            &Tm35finToMercator::new(),
        );
        let area = |f: &[PolygonFeature]| f.iter().map(|p| p.polygon.unsigned_area()).sum::<f64>();
        assert_eq!(dry.len(), 1);
        assert!(
            area(&wet) < area(&dry) * 0.8,
            "{} vs {}",
            area(&wet),
            area(&dry)
        );
    }
}
