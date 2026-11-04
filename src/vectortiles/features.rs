//! The features of the tile layers in Web Mercator metres, read from the `.dxf.bin` files of a pullautin run.

use super::mvt::Value;
use super::project::Tm35finToMercator;
use super::simplify::douglas_peucker;
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

fn line_length(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

impl Features {
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
