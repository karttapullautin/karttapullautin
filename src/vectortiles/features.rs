//! The features of the tile layers in Web Mercator metres, read from the `.dxf.bin` files of a pullautin run.

use super::mvt::Value;
use super::project::Tm35finToMercator;
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
            cliff: lines(&self.cliff, &OVERVIEW_CLIFF_KINDS, OVERVIEW_MIN_CLIFF_M),
            knoll: Vec::new(),
            vegetation,
        }
    }

    /// Adds the layers of one sheet: `temp` is the `temp` folder of its pullautin run.
    pub fn add_sheet(&mut self, temp: &Path, projection: &Tm35finToMercator) -> anyhow::Result<()> {
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
        // cliffs: Polylines2, not simplified
        for name in ["c2g", "c3g"] {
            if let Some(Geometry::Polylines2(lines)) = read(name)? {
                for (points, class) in lines.iter() {
                    if points.len() < 2 {
                        continue;
                    }
                    let raw: Vec<_> = points.iter().map(|p| (p.x, p.y)).collect();
                    self.cliff.push(LineFeature {
                        points: project(&raw),
                        props: vec![("kind", Value::Str(class.to_layer().to_string()))],
                    });
                }
            }
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
        let mut f = Features::default();
        f.contour = vec![
            line("index", 500.0),
            line("normal", 500.0),
            line("intermed", 500.0),
            line("depression", 500.0),
            line("normal", 30.0),
        ];
        f.formline = vec![line("", 500.0)];
        f.cliff = vec![
            line("cliff2", 100.0),
            line("cliff3", 100.0),
            line("cliff4", 100.0),
            line("cliff2", 20.0),
        ];
        f.knoll = vec![PointFeature {
            point: (1.0, 1.0),
            props: vec![],
        }];
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
