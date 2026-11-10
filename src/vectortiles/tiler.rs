//! Cuts the features into Web Mercator tiles of one zoom level, clips them with a buffer and encodes the tiles as MVT.

use super::features::{Features, LineFeature, PointFeature, PolygonFeature};
use super::mvt::{self, EXTENT, Layer, Point};
use super::pmtiles::tile_id;
use super::project::MERCATOR_ORIGIN;
use geo::{BooleanOps, Coord, LineString, Polygon};

/// How far a tile's geometry runs past its edge, in tile units
pub const BUFFER_UNITS: f64 = 64.0;

/// The layers in the order they have in a tile, with their names
pub const LAYER_NAMES: [&str; 5] = ["contour", "formline", "cliff", "knoll", "vegetation"];

#[derive(Default, Clone, Debug)]
pub struct TileRefs {
    pub contour: Vec<u32>,
    pub formline: Vec<u32>,
    pub cliff: Vec<u32>,
    pub knoll: Vec<u32>,
    pub vegetation: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    minx: f64,
    miny: f64,
    maxx: f64,
    maxy: f64,
}

impl Rect {
    fn expand(&self, by: f64) -> Rect {
        Rect {
            minx: self.minx - by,
            miny: self.miny - by,
            maxx: self.maxx + by,
            maxy: self.maxy + by,
        }
    }

    fn contains_rect(&self, other: &Rect) -> bool {
        other.minx >= self.minx
            && other.maxx <= self.maxx
            && other.miny >= self.miny
            && other.maxy <= self.maxy
    }

    fn polygon(&self) -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                (self.minx, self.miny),
                (self.maxx, self.miny),
                (self.maxx, self.maxy),
                (self.minx, self.maxy),
                (self.minx, self.miny),
            ]),
            vec![],
        )
    }
}

/// Tile size in Web Mercator metres at a zoom level
pub fn tile_size(zoom: u8) -> f64 {
    2.0 * MERCATOR_ORIGIN / (1u64 << zoom) as f64
}

fn tile_rect(zoom: u8, tx: u32, ty: u32) -> Rect {
    let size = tile_size(zoom);
    Rect {
        minx: -MERCATOR_ORIGIN + tx as f64 * size,
        maxx: -MERCATOR_ORIGIN + (tx + 1) as f64 * size,
        maxy: MERCATOR_ORIGIN - ty as f64 * size,
        miny: MERCATOR_ORIGIN - (ty + 1) as f64 * size,
    }
}

fn bounds_of(points: impl Iterator<Item = (f64, f64)>) -> Option<Rect> {
    let mut r: Option<Rect> = None;
    for (x, y) in points {
        r = Some(match r {
            None => Rect {
                minx: x,
                miny: y,
                maxx: x,
                maxy: y,
            },
            Some(r) => Rect {
                minx: r.minx.min(x),
                miny: r.miny.min(y),
                maxx: r.maxx.max(x),
                maxy: r.maxy.max(y),
            },
        });
    }
    r
}

/// The tiles that world metres bounds (min x, min y, max x, max y) touch at a zoom
pub fn tiles_of_bounds(
    (minx, miny, maxx, maxy): (f64, f64, f64, f64),
    zoom: u8,
) -> (std::ops::RangeInclusive<u32>, std::ops::RangeInclusive<u32>) {
    tiles_of(
        &Rect {
            minx,
            miny,
            maxx,
            maxy,
        },
        zoom,
    )
}

/// Which tiles a feature with these bounds touches
fn tiles_of(
    bounds: &Rect,
    zoom: u8,
) -> (std::ops::RangeInclusive<u32>, std::ops::RangeInclusive<u32>) {
    let size = tile_size(zoom);
    let max = (1u64 << zoom) as f64 - 1.0;
    let tx0 = ((bounds.minx + MERCATOR_ORIGIN) / size)
        .floor()
        .clamp(0.0, max) as u32;
    let tx1 = ((bounds.maxx + MERCATOR_ORIGIN) / size)
        .floor()
        .clamp(0.0, max) as u32;
    let ty0 = ((MERCATOR_ORIGIN - bounds.maxy) / size)
        .floor()
        .clamp(0.0, max) as u32;
    let ty1 = ((MERCATOR_ORIGIN - bounds.miny) / size)
        .floor()
        .clamp(0.0, max) as u32;
    (tx0..=tx1, ty0..=ty1)
}

/// The tiles that have anything in them, in tile id order, with the features they hold
pub fn index_tiles(features: &Features, zoom: u8) -> Vec<(u64, u32, u32, TileRefs)> {
    use std::collections::HashMap;
    let mut tiles: HashMap<(u32, u32), TileRefs> = HashMap::new();

    fn add(
        tiles: &mut HashMap<(u32, u32), TileRefs>,
        zoom: u8,
        bounds: Option<Rect>,
        push: impl Fn(&mut TileRefs),
    ) {
        let Some(bounds) = bounds else { return };
        let (xs, ys) = tiles_of(&bounds, zoom);
        for tx in xs {
            for ty in ys.clone() {
                push(tiles.entry((tx, ty)).or_default());
            }
        }
    }

    for (i, f) in features.contour.iter().enumerate() {
        add(
            &mut tiles,
            zoom,
            bounds_of(f.line.coords().map(|c| (c.x, c.y))),
            |t| t.contour.push(i as u32),
        );
    }
    for (i, f) in features.formline.iter().enumerate() {
        add(
            &mut tiles,
            zoom,
            bounds_of(f.line.coords().map(|c| (c.x, c.y))),
            |t| t.formline.push(i as u32),
        );
    }
    for (i, f) in features.cliff.iter().enumerate() {
        add(
            &mut tiles,
            zoom,
            bounds_of(f.line.coords().map(|c| (c.x, c.y))),
            |t| t.cliff.push(i as u32),
        );
    }
    for (i, f) in features.knoll.iter().enumerate() {
        add(
            &mut tiles,
            zoom,
            bounds_of(std::iter::once(f.point.x_y())),
            |t| t.knoll.push(i as u32),
        );
    }
    for (i, f) in features.vegetation.iter().enumerate() {
        add(
            &mut tiles,
            zoom,
            bounds_of(f.polygon.exterior().coords().map(|c| (c.x, c.y))),
            |t| t.vegetation.push(i as u32),
        );
    }
    let mut out: Vec<_> = tiles
        .into_iter()
        .map(|((tx, ty), refs)| (tile_id(zoom, tx, ty), tx, ty, refs))
        .collect();
    out.sort_by_key(|t| t.0);
    out
}

/// World metres to tile units, y down
fn quantize(x: f64, y: f64, tile: &Rect, size: f64) -> Point {
    (
        ((x - tile.minx) / size * EXTENT as f64).round() as i32,
        ((tile.maxy - y) / size * EXTENT as f64).round() as i32,
    )
}

/// Liang-Barsky: the part of the segment inside the rectangle, as parameters along the segment
fn clip_segment(a: (f64, f64), b: (f64, f64), r: &Rect) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.0 - r.minx),
        (dx, r.maxx - a.0),
        (-dy, a.1 - r.miny),
        (dy, r.maxy - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return None;
                }
                t1 = t1.min(t);
            }
        }
    }
    Some((t0, t1))
}

/// The pieces of a line inside the rectangle
fn clip_line(points: &[Coord<f64>], r: &Rect) -> Vec<Vec<(f64, f64)>> {
    let mut pieces = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = ((w[0].x, w[0].y), (w[1].x, w[1].y));
        match clip_segment(a, b, r) {
            None => {
                if current.len() >= 2 {
                    pieces.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
            Some((t0, t1)) => {
                let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                if current.is_empty() {
                    current.push(at(t0));
                }
                current.push(at(t1));
                if t1 < 1.0 {
                    pieces.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if current.len() >= 2 {
        pieces.push(current);
    }
    pieces
}

fn dedupe(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// A ring in tile units: no repeated points, at least three, not flat; with the winding the format wants.
fn clean_ring(points: Vec<Point>, exterior: bool) -> Option<Vec<Point>> {
    let mut ring = dedupe(points);
    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 {
        return None;
    }
    let area = mvt::ring_area2(&ring);
    if area == 0 {
        return None;
    }
    if (area > 0) != exterior {
        ring.reverse();
    }
    Some(ring)
}

fn polygon_to_mvt(polygon: &Polygon<f64>, tile: &Rect, size: f64) -> Option<mvt::Geometry> {
    let ring = |line: &LineString<f64>| -> Vec<Point> {
        line.coords()
            .map(|c| quantize(c.x, c.y, tile, size))
            .collect()
    };
    let exterior = clean_ring(ring(polygon.exterior()), true)?;
    let mut rings = vec![exterior];
    for hole in polygon.interiors() {
        if let Some(h) = clean_ring(ring(hole), false) {
            rings.push(h);
        }
    }
    Some(mvt::Geometry::Polygon(rings))
}

/// The first and last scan year of the data of each tile, by tile
pub type TileYears = std::collections::HashMap<(u32, u32), (u16, u16)>;

pub struct TileBuilder<'a> {
    pub features: &'a Features,
    pub zoom: u8,
    /// the first and last scan year of the data of each tile (`mvt::scan_years_layer`), None = not told
    pub years: Option<&'a TileYears>,
}

/// How many features each layer of the encoded tiles got
#[derive(Default, Debug, Clone)]
pub struct LayerCounts(pub [usize; 5]);

impl TileBuilder<'_> {
    /// None when nothing of the features ends up in the tile
    pub fn build(
        &self,
        tx: u32,
        ty: u32,
        refs: &TileRefs,
        counts: &mut LayerCounts,
    ) -> Option<Vec<u8>> {
        let size = tile_size(self.zoom);
        let tile = tile_rect(self.zoom, tx, ty);
        let clip_rect = tile.expand(size * BUFFER_UNITS / EXTENT as f64);

        let mut layers: Vec<Layer> = LAYER_NAMES.iter().map(|n| Layer::new(n)).collect();

        let lines = |layer: &mut Layer, source: &[LineFeature], ids: &[u32]| {
            for &i in ids {
                let f = &source[i as usize];
                for piece in clip_line(&f.line.0, &clip_rect) {
                    let points = dedupe(
                        piece
                            .iter()
                            .map(|&(x, y)| quantize(x, y, &tile, size))
                            .collect(),
                    );
                    if points.len() >= 2 {
                        layer.add(&mvt::Feature {
                            geometry: mvt::Geometry::Line(points),
                            properties: f.props.clone(),
                        });
                    }
                }
            }
        };
        lines(&mut layers[0], &self.features.contour, &refs.contour);
        lines(&mut layers[1], &self.features.formline, &refs.formline);
        lines(&mut layers[2], &self.features.cliff, &refs.cliff);

        let points = |layer: &mut Layer, source: &[PointFeature], ids: &[u32]| {
            for &i in ids {
                let f = &source[i as usize];
                let (x, y) = f.point.x_y();
                if x >= tile.minx && x < tile.maxx && y >= tile.miny && y < tile.maxy {
                    layer.add(&mvt::Feature {
                        geometry: mvt::Geometry::Point(quantize(x, y, &tile, size)),
                        properties: f.props.clone(),
                    });
                }
            }
        };
        points(&mut layers[3], &self.features.knoll, &refs.knoll);

        let polygons = |layer: &mut Layer, source: &[PolygonFeature], ids: &[u32]| {
            let clip_polygon = clip_rect.polygon();
            for &i in ids {
                let f: &PolygonFeature = &source[i as usize];
                let bounds = bounds_of(
                    f.polygon
                        .exterior()
                        .coords()
                        .map(|c: &Coord<f64>| (c.x, c.y)),
                );
                let inside = bounds.is_some_and(|b| clip_rect.contains_rect(&b));
                let parts: Vec<Polygon<f64>> = if inside {
                    vec![f.polygon.clone()]
                } else {
                    f.polygon.intersection(&clip_polygon).0
                };
                for part in parts {
                    if let Some(geometry) = polygon_to_mvt(&part, &tile, size) {
                        layer.add(&mvt::Feature {
                            geometry,
                            properties: f.props.clone(),
                        });
                    }
                }
            }
        };
        polygons(&mut layers[4], &self.features.vegetation, &refs.vegetation);

        for (c, l) in counts.0.iter_mut().zip(&layers) {
            *c += l.len();
        }
        if layers.iter().all(|l| l.is_empty()) {
            return None;
        }
        let mut bytes = mvt::encode_tile(&layers);
        if let Some((first, last)) = self.years.and_then(|y| y.get(&(tx, ty))).copied()
            && let Some(layer) = mvt::scan_years_layer(first, last)
        {
            bytes.extend_from_slice(&layer);
        }
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_a_line_into_the_pieces_inside_the_rectangle() {
        let r = Rect {
            minx: 0.0,
            miny: 0.0,
            maxx: 10.0,
            maxy: 10.0,
        };
        // enters, leaves, comes back
        let line: Vec<Coord<f64>> = [
            (-5.0, 5.0),
            (5.0, 5.0),
            (15.0, 5.0),
            (15.0, 8.0),
            (5.0, 8.0),
        ]
        .map(Coord::from)
        .to_vec();
        let pieces = clip_line(&line, &r);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0], vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]);
        assert_eq!(pieces[1], vec![(10.0, 8.0), (5.0, 8.0)]);
        assert!(clip_line(&[(20.0, 20.0).into(), (30.0, 30.0).into()], &r).is_empty());
    }

    #[test]
    fn a_ring_gets_the_winding_of_its_role() {
        let cw = vec![(0, 0), (10, 0), (10, 10), (0, 10)];
        let ccw: Vec<_> = cw.iter().rev().copied().collect();
        assert!(mvt::ring_area2(&clean_ring(ccw.clone(), true).unwrap()) > 0);
        assert!(mvt::ring_area2(&clean_ring(cw.clone(), false).unwrap()) < 0);
        // repeated and closing points go, flat rings go
        assert_eq!(
            clean_ring(vec![(0, 0), (0, 0), (10, 0), (10, 10), (0, 0)], true)
                .unwrap()
                .len(),
            3
        );
        assert!(clean_ring(vec![(0, 0), (5, 0), (10, 0)], true).is_none());
    }

    #[test]
    fn tile_rects_tile_the_world() {
        let z = 3;
        let a = tile_rect(z, 0, 0);
        assert!((a.minx + MERCATOR_ORIGIN).abs() < 1e-6 && (a.maxy - MERCATOR_ORIGIN).abs() < 1e-6);
        let b = tile_rect(z, 1, 0);
        assert!((b.minx - a.maxx).abs() < 1e-6);
    }
}
