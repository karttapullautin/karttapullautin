//! The features of the tile layers in Web Mercator metres, read from the `.dxf.bin` files of a pullautin run.

use super::mvt::Value;
use super::project::Projection;
use super::simplify::{douglas_peucker, simplify_ring};
use crate::geometry::{BinaryDxf, Classification, Geometry};
use std::path::Path;

pub type Props = Vec<(&'static str, Value)>;

#[derive(Clone, Debug)]
pub struct LineFeature {
    pub points: Vec<(f64, f64)>,
    pub props: Props,
}

#[derive(Clone, Debug)]
pub struct PointFeature {
    pub point: (f64, f64),
    pub props: Props,
}

#[derive(Clone, Debug)]
pub struct PolygonFeature {
    pub polygon: geo::Polygon<f64>,
    pub props: Props,
}

/// All features of an area, per layer. The order of the layers is the order in the tiles.
#[derive(Default)]
pub struct Features {
    pub contour: Vec<LineFeature>,
    pub formline: Vec<LineFeature>,
    pub cliff: Vec<LineFeature>,
    pub knoll: Vec<PointFeature>,
    pub vegetation: Vec<PolygonFeature>,
}

/// Simplification tolerance of contours and formlines, metres (ETRS-TM35FIN)
pub const LINE_SIMPLIFY_M: f64 = 0.3;

/// Cliff pieces of the same kind with vertices closer than this (metres in the map's CRS) belong to one cluster
pub const CLIFF_CLUSTER_GAP_M: f64 = 3.0;
/// A cluster is drawn as one short, nearly straight line: the points of a cluster are binned along its axis by this many metres and the bin
/// centres simplified by `CLIFF_SIMPLIFY_M`
pub const CLIFF_BIN_M: f64 = 6.0;
pub const CLIFF_SIMPLIFY_M: f64 = 1.5;
/// A cluster longer than this along its axis is cut in two at the middle of its pieces, again and again, so that every line stays short and straight
pub const CLIFF_MAX_LINE_M: f64 = 40.0;
/// The `size` property of a cliff line: 1 = a single piece, 2 = a cluster of 2-5 pieces, 3 = a cluster of 6 or more (higher cliffs: drawn thicker)
pub fn cliff_size_class(pieces: usize) -> i64 {
    match pieces {
        0 | 1 => 1,
        2..=5 => 2,
        _ => 3,
    }
}

/// A cliff line made of `pieces` pieces
#[derive(Debug, Clone, PartialEq)]
pub struct CliffLine {
    pub points: Vec<(f64, f64)>,
    pub kind: String,
    pub pieces: usize,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The line of a group of pieces (indices into `pieces`): the piece itself when there is one, else the centre line of their points
/// along the principal axis, cut in two (recursively) when it is longer than `max_len`.
fn cliff_lines_of(
    pieces: &[(Vec<(f64, f64)>, String)],
    group: &[usize],
    max_len: f64,
    out: &mut Vec<CliffLine>,
) {
    let kind = pieces[group[0]].1.clone();
    if group.len() == 1 {
        let points = &pieces[group[0]].0;
        out.push(CliffLine {
            points: douglas_peucker(points, CLIFF_SIMPLIFY_M),
            kind,
            pieces: 1,
        });
        return;
    }
    let points: Vec<(f64, f64)> = group
        .iter()
        .flat_map(|&i| pieces[i].0.iter().copied())
        .collect();
    let n = points.len() as f64;
    let (mx, my) = (
        points.iter().map(|p| p.0).sum::<f64>() / n,
        points.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in &points {
        let (dx, dy) = (p.0 - mx, p.1 - my);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    // the principal axis u and its perpendicular v
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (u, v) = ((theta.cos(), theta.sin()), (-theta.sin(), theta.cos()));
    let along = |p: &(f64, f64)| (p.0 - mx) * u.0 + (p.1 - my) * u.1;
    let across = |p: &(f64, f64)| (p.0 - mx) * v.0 + (p.1 - my) * v.1;
    let (amin, amax) = points
        .iter()
        .map(along)
        .fold((f64::MAX, f64::MIN), |(lo, hi), a| (lo.min(a), hi.max(a)));
    if amax - amin > max_len && group.len() > 1 {
        // cut at the middle of the pieces along the axis
        let mut order: Vec<(f64, usize)> = group
            .iter()
            .map(|&i| {
                let c = &pieces[i].0;
                (c.iter().map(along).sum::<f64>() / c.len() as f64, i)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let half = order.len() / 2;
        let (first, second): (Vec<usize>, Vec<usize>) = (
            order[..half].iter().map(|o| o.1).collect(),
            order[half..].iter().map(|o| o.1).collect(),
        );
        if !first.is_empty() && !second.is_empty() {
            cliff_lines_of(pieces, &first, max_len, out);
            cliff_lines_of(pieces, &second, max_len, out);
            return;
        }
    }
    // the centre line: the mean of the points of every bin along the axis
    let mut bins: std::collections::BTreeMap<i64, (f64, f64, f64)> =
        std::collections::BTreeMap::new();
    for p in &points {
        let e = bins
            .entry((along(p) / CLIFF_BIN_M).floor() as i64)
            .or_insert((0.0, 0.0, 0.0));
        e.0 += along(p);
        e.1 += across(p);
        e.2 += 1.0;
    }
    let mut line: Vec<(f64, f64)> = bins
        .values()
        .map(|&(a, b, c)| {
            let (a, b) = (a / c, b / c);
            (mx + a * u.0 + b * v.0, my + a * u.1 + b * v.1)
        })
        .collect();
    if line.len() < 2 {
        // everything in one bin: the axis across the extent of the points
        let b = points.iter().map(across).sum::<f64>() / n;
        line = [amin, amax]
            .iter()
            .map(|&a| (mx + a * u.0 + b * v.0, my + a * u.1 + b * v.1))
            .collect();
    }
    out.push(CliffLine {
        points: douglas_peucker(&line, CLIFF_SIMPLIFY_M),
        kind,
        pieces: group.len(),
    });
}

/// Draws the cliff pieces as few, short, nearly straight lines: pieces of the same kind with vertices within `gap` of each other form a cluster (a group of
/// cliffs, or a longer cliff the pieces of which meet), and a cluster becomes one line along its axis (`cliff_lines_of`) with the number of its pieces, so
/// that a dense cluster no longer makes a back and forth zigzag. A lone piece stays as it is.
pub fn cluster_cliffs(
    pieces: Vec<(Vec<(f64, f64)>, String)>,
    gap: f64,
    max_len: f64,
) -> Vec<CliffLine> {
    use std::collections::HashMap;
    let n = pieces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let cell = |p: (f64, f64)| ((p.0 / gap).floor() as i64, (p.1 / gap).floor() as i64);
    // the vertices of the pieces by grid cell: (piece, vertex)
    type VertexGrid = HashMap<(i64, i64), Vec<(usize, (f64, f64))>>;
    let mut grid: VertexGrid = HashMap::new();
    for (i, (points, _)) in pieces.iter().enumerate() {
        for &p in points {
            grid.entry(cell(p)).or_default().push((i, p));
        }
    }
    for (i, (points, kind)) in pieces.iter().enumerate() {
        for &p in points {
            let (cx, cy) = cell(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &(j, q) in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                        if j != i && pieces[j].1 == *kind && (p.0 - q.0).hypot(p.1 - q.1) <= gap {
                            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                            if a != b {
                                parent[a] = b;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out = Vec::new();
    for group in groups.values() {
        cliff_lines_of(&pieces, group, max_len, &mut out);
    }
    out
}

/// The `kind` property of a contour: the layer name of the DXF, with the names of the main classes shortened.
fn contour_kind(class: Classification) -> String {
    match class.to_layer() {
        "contour" => "normal",
        "contour_index" => "index",
        "contour_intermed" => "intermed",
        "slope_line" => "slope",
        other => other,
    }
    .to_string()
}

/// What the overview zoom (z13) keeps of the detail features, in Web Mercator metres (a z13 pixel is about 19 m wide there, about 9 m
/// on the ground in Finland and Sweden): the contours of the main classes only, the large cliffs, no formlines and no knolls, vegetation
/// polygons and holes of at least `OVERVIEW_MIN_AREA_M2`, all simplified more. The list of kept contour kinds is the one a reader needs at 1:25 000.
pub const OVERVIEW_LINE_TOLERANCE_M: f64 = 3.0;
pub const OVERVIEW_MIN_LINE_M: f64 = 60.0;
/// cliffs are short by nature and matter in the terrain: a lower limit than the contours'
pub const OVERVIEW_MIN_CLIFF_M: f64 = 25.0;
pub const OVERVIEW_POLYGON_TOLERANCE_M: f64 = 6.0;
pub const OVERVIEW_MIN_AREA_M2: f64 = 1600.0;
const OVERVIEW_CONTOUR_KINDS: [&str; 2] = ["normal", "index"];
const OVERVIEW_CLIFF_KINDS: [&str; 2] = ["cliff2", "cliff3"];

fn kind_of(props: &Props) -> Option<&str> {
    props.iter().find_map(|(k, v)| match (k, v) {
        (&"kind", Value::Str(s)) => Some(s.as_str()),
        _ => None,
    })
}

fn ring_area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (x0, y0) = ring[i];
            let (x1, y1) = ring[(i + 1) % n];
            x0 * y1 - x1 * y0
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn ring_coords(line: &geo::LineString<f64>) -> Vec<(f64, f64)> {
    line.coords().map(|c| (c.x, c.y)).collect()
}

fn simplified_ring(line: &geo::LineString<f64>) -> Option<geo::LineString<f64>> {
    let ring = ring_coords(line);
    if ring_area(&ring) < OVERVIEW_MIN_AREA_M2 {
        return None;
    }
    let simplified = simplify_ring(&ring, OVERVIEW_POLYGON_TOLERANCE_M);
    (simplified.len() >= 4 && ring_area(&simplified) >= OVERVIEW_MIN_AREA_M2)
        .then(|| geo::LineString::from(simplified))
}

fn line_length(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

impl Features {
    /// The features of the overview zoom: fewer and simpler (see `OVERVIEW_*`).
    pub fn overview(&self) -> Features {
        let lines = |source: &[LineFeature], kinds: &[&str], min_length: f64| -> Vec<LineFeature> {
            source
                .iter()
                .filter(|f| kind_of(&f.props).is_some_and(|k| kinds.contains(&k)))
                .filter_map(|f| {
                    let points = douglas_peucker(&f.points, OVERVIEW_LINE_TOLERANCE_M);
                    (points.len() >= 2 && line_length(&points) >= min_length).then(|| LineFeature {
                        points,
                        props: f.props.clone(),
                    })
                })
                .collect()
        };
        // the larger cliff lines: of kinds cliff2 and cliff3, the clusters (size 2 and 3) and the single pieces that are long enough
        let cliffs = |source: &[LineFeature]| -> Vec<LineFeature> {
            let clustered = |f: &LineFeature| {
                f.props
                    .iter()
                    .any(|(k, v)| matches!((k, v), (&"size", Value::Int(n)) if *n >= 2))
            };
            lines(source, &OVERVIEW_CLIFF_KINDS, 0.0)
                .into_iter()
                .filter(|f| clustered(f) || line_length(&f.points) >= OVERVIEW_MIN_CLIFF_M)
                .collect()
        };
        let vegetation = self
            .vegetation
            .iter()
            .filter_map(|f| {
                let exterior = simplified_ring(f.polygon.exterior())?;
                let holes = f
                    .polygon
                    .interiors()
                    .iter()
                    .filter_map(simplified_ring)
                    .collect();
                Some(PolygonFeature {
                    polygon: geo::Polygon::new(exterior, holes),
                    props: f.props.clone(),
                })
            })
            .collect();
        Features {
            contour: lines(&self.contour, &OVERVIEW_CONTOUR_KINDS, OVERVIEW_MIN_LINE_M),
            formline: Vec::new(),
            cliff: cliffs(&self.cliff),
            knoll: Vec::new(),
            vegetation,
        }
    }

    /// Adds the layers of one sheet: `temp` is the `temp` folder of its pullautin run.
    pub fn add_sheet(&mut self, temp: &Path, projection: &Projection) -> anyhow::Result<()> {
        let project = |points: &[(f64, f64)]| -> Vec<(f64, f64)> {
            points
                .iter()
                .map(|&(x, y)| projection.to_mercator(x, y))
                .collect()
        };
        let read = |name: &str| -> anyhow::Result<Option<Geometry>> {
            let path = temp.join(format!("{name}.dxf.bin"));
            if !path.exists() {
                return Ok(None);
            }
            let mut file = std::io::BufReader::new(std::fs::File::open(&path)?);
            Ok(BinaryDxf::from_reader(&mut file)?
                .take_geometry()
                .into_iter()
                .next())
        };

        // contours: Polylines3 with the height
        if let Some(Geometry::Polylines3(lines)) = read("out2")? {
            for (points, (class, height)) in lines.iter() {
                if points.len() < 2 {
                    continue;
                }
                let raw: Vec<_> = points.iter().map(|p| (p.x, p.y)).collect();
                let simplified = douglas_peucker(&raw, LINE_SIMPLIFY_M);
                if simplified.len() < 2 || line_length(&simplified) < 1.0 {
                    continue;
                }
                self.contour.push(LineFeature {
                    points: project(&simplified),
                    props: vec![
                        ("kind", Value::Str(contour_kind(*class))),
                        ("ele", Value::Double((height * 10.0).round() / 10.0)),
                    ],
                });
            }
        }
        // formlines: Polylines2
        if let Some(Geometry::Polylines2(lines)) = read("formlines")? {
            for (points, _) in lines.iter() {
                if points.len() < 2 {
                    continue;
                }
                let raw: Vec<_> = points.iter().map(|p| (p.x, p.y)).collect();
                let simplified = douglas_peucker(&raw, LINE_SIMPLIFY_M);
                if simplified.len() < 2 || line_length(&simplified) < 1.0 {
                    continue;
                }
                self.formline.push(LineFeature {
                    points: project(&simplified),
                    props: Vec::new(),
                });
            }
        }
        // cliffs: Polylines2 of short pieces; clusters of them become short, nearly straight lines with a `size` class (higher cliffs: thicker)
        let mut cliff_pieces: Vec<(Vec<(f64, f64)>, String)> = Vec::new();
        for name in ["c2g", "c3g"] {
            if let Some(Geometry::Polylines2(lines)) = read(name)? {
                for (points, class) in lines.iter() {
                    if points.len() < 2 {
                        continue;
                    }
                    let raw: Vec<_> = points.iter().map(|p| (p.x, p.y)).collect();
                    cliff_pieces.push((raw, class.to_layer().to_string()));
                }
            }
        }
        for line in cluster_cliffs(cliff_pieces, CLIFF_CLUSTER_GAP_M, CLIFF_MAX_LINE_M) {
            if line.points.len() < 2 {
                continue;
            }
            self.cliff.push(LineFeature {
                points: project(&line.points),
                props: vec![
                    ("kind", Value::Str(line.kind)),
                    ("size", Value::Int(cliff_size_class(line.pieces))),
                ],
            });
        }
        // knolls: Points
        if let Some(Geometry::Points(points)) = read("dotknolls")? {
            for (p, class) in points.iter() {
                self.knoll.push(PointFeature {
                    point: projection.to_mercator(p.x, p.y),
                    props: vec![("kind", Value::Str(class.to_layer().to_string()))],
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod overview_tests {
    use super::*;

    fn line(kind: &str, length: f64) -> LineFeature {
        LineFeature {
            points: vec![(0.0, 0.0), (length / 2.0, 0.5), (length, 0.0)],
            props: vec![("kind", Value::Str(kind.to_string()))],
        }
    }

    fn square(side: f64) -> geo::Polygon<f64> {
        geo::Polygon::new(
            geo::LineString::from(vec![
                (0.0, 0.0),
                (side, 0.0),
                (side, side),
                (0.0, side),
                (0.0, 0.0),
            ]),
            vec![],
        )
    }

    #[test]
    fn the_overview_keeps_the_main_contours_and_big_cliffs_and_drops_the_rest() {
        let f = Features {
            contour: vec![
                line("index", 500.0),
                line("normal", 500.0),
                line("intermed", 500.0),
                line("depression", 500.0),
                line("normal", 30.0),
            ],
            formline: vec![line("", 500.0)],
            cliff: vec![
                line("cliff2", 100.0),
                line("cliff3", 100.0),
                line("cliff4", 100.0),
                line("cliff2", 20.0),
            ],
            knoll: vec![PointFeature {
                point: (1.0, 1.0),
                props: vec![],
            }],
            ..Default::default()
        };
        let o = f.overview();
        let kinds = |l: &[LineFeature]| {
            l.iter()
                .map(|x| kind_of(&x.props).unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(&o.contour), ["index", "normal"]);
        assert!(o.formline.is_empty() && o.knoll.is_empty());
        assert_eq!(kinds(&o.cliff), ["cliff2", "cliff3"]);
        // the nearly straight line has lost its middle vertex
        assert_eq!(o.contour[0].points.len(), 2);
    }

    #[test]
    fn the_overview_drops_small_polygons_and_holes_and_simplifies_the_rest() {
        let mut big = geo::Polygon::new(
            square(100.0).exterior().clone(),
            vec![
                square(10.0).exterior().clone(),
                geo::LineString::from(vec![
                    (20.0, 20.0),
                    (70.0, 20.0),
                    (70.0, 70.0),
                    (20.0, 70.0),
                    (20.0, 20.0),
                ]),
            ],
        );
        // a wobbly edge that the tolerance flattens
        let mut ring: Vec<_> = big.exterior().coords().map(|c| (c.x, c.y)).collect();
        ring.insert(1, (50.0, 1.0));
        big = geo::Polygon::new(geo::LineString::from(ring), big.interiors().to_vec());
        let mut f = Features::default();
        let props = vec![("kind", Value::Str("green".into()))];
        f.vegetation = vec![
            PolygonFeature {
                polygon: big,
                props: props.clone(),
            },
            PolygonFeature {
                polygon: square(20.0),
                props: props.clone(),
            }, // 400 m2
        ];
        let o = f.overview();
        assert_eq!(o.vegetation.len(), 1);
        let p = &o.vegetation[0].polygon;
        assert_eq!(
            p.interiors().len(),
            1,
            "the 100 m2 hole goes, the 2500 m2 one stays"
        );
        assert!(
            p.exterior().coords().count() <= 5,
            "the wobble is flattened"
        );
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    fn piece(a: (f64, f64), b: (f64, f64), kind: &str) -> (Vec<(f64, f64)>, String) {
        (vec![a, b], kind.to_string())
    }

    #[test]
    fn a_dense_back_and_forth_cluster_becomes_one_nearly_straight_line_with_the_number_of_its_pieces()
     {
        // eight short pieces in a zigzag along the x axis, within 1.5 m of the axis
        let pieces: Vec<_> = (0..8)
            .map(|i| {
                let x = i as f64 * 3.0;
                piece(
                    (x, if i % 2 == 0 { 1.0 } else { -1.0 }),
                    (x + 2.0, if i % 2 == 0 { -1.0 } else { 1.0 }),
                    "cliff2",
                )
            })
            .collect();
        let lines = cluster_cliffs(pieces, 3.0, 40.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].pieces, 8);
        assert!(
            lines[0].points.len() <= 4,
            "{} points",
            lines[0].points.len()
        );
        // straight: every point within the tolerance of the axis
        assert!(lines[0].points.iter().all(|p| p.1.abs() < 1.6));
        assert_eq!(cliff_size_class(lines[0].pieces), 3);
    }

    #[test]
    fn a_lone_piece_stays_a_line_of_its_own_and_other_kinds_or_far_pieces_are_other_lines() {
        let lines = cluster_cliffs(
            vec![
                piece((0.0, 0.0), (6.0, 0.0), "cliff2"),
                piece((3.0, 0.5), (9.0, 0.5), "cliff3"), // close, but another kind
                piece((100.0, 0.0), (106.0, 0.0), "cliff2"), // far away
            ],
            3.0,
            40.0,
        );
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.pieces == 1 && l.points.len() == 2));
        assert_eq!(cliff_size_class(1), 1);
        assert_eq!(cliff_size_class(2), 2);
        assert_eq!(cliff_size_class(5), 2);
        assert_eq!(cliff_size_class(6), 3);
    }

    #[test]
    fn a_long_cluster_is_cut_into_lines_of_at_most_the_maximum_length() {
        // 30 pieces end to end along 180 m
        let pieces: Vec<_> = (0..30)
            .map(|i| piece((i as f64 * 6.0, 0.0), (i as f64 * 6.0 + 5.0, 0.0), "cliff2"))
            .collect();
        let lines = cluster_cliffs(pieces, 3.0, 40.0);
        assert!(lines.len() >= 4, "{} lines", lines.len());
        let length = |l: &CliffLine| {
            let (a, b) = (l.points[0], l.points[l.points.len() - 1]);
            (b.0 - a.0).hypot(b.1 - a.1)
        };
        assert!(
            lines.iter().all(|l| length(l) <= 45.0),
            "{:?}",
            lines.iter().map(length).collect::<Vec<_>>()
        );
        assert_eq!(lines.iter().map(|l| l.pieces).sum::<usize>(), 30);
    }
}
