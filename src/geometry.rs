//! This mod contains structs for storing and loading different types of geometry, like Polylines
//! and a list of Points.
//!
//! These types also have helpers for exporting them to DXF format.

/// A 2D point
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point2 {
    /// The x coordinate of this point.
    pub x: f64,
    /// The y coordinate of this point.
    pub y: f64,
}

impl Point2 {
    /// Create a new point from the given coordinates.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A 3D point (eg. 2D + height)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point3 {
    /// The x coordinate of this point.
    pub x: f64,
    /// The y coordinate of this point.
    pub y: f64,
    /// The z coordinate of this point (height).
    pub z: f64,
}

impl Point3 {
    /// Create a new point from the given coordinates.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// A collection of points with associated classification. This classification is also used to put
/// the DXF objects into separate layers.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Points {
    points: Vec<Point2>,
    classification: Vec<Classification>,
}

impl Points {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            classification: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            points: Vec::with_capacity(capacity),
            classification: Vec::with_capacity(capacity),
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Add a point to this collection.
    pub fn push(&mut self, point: Point2, class: Classification) {
        self.points.push(point);
        self.classification.push(class);
    }

    /// Iterate over the points in this collection.
    pub fn iter(&self) -> impl Iterator<Item = (&Point2, &Classification)> {
        self.points.iter().zip(self.classification.iter())
    }
}

impl IntoIterator for Points {
    type Item = (Point2, Classification);

    type IntoIter = std::iter::Zip<std::vec::IntoIter<Point2>, std::vec::IntoIter<Classification>>;

    fn into_iter(self) -> Self::IntoIter {
        self.points.into_iter().zip(self.classification)
    }
}

/// A collection polylines with associated classification.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Polylines<P, C> {
    polylines: Vec<Vec<P>>, // TODO: flatten to single vector?
    classification: Vec<C>,
}

impl<P, C> Polylines<P, C> {
    pub fn new() -> Self {
        Self {
            polylines: Vec::new(),
            classification: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            polylines: Vec::with_capacity(capacity),
            classification: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, polyline: Vec<P>, class: C) {
        self.polylines.push(polyline);
        self.classification.push(class);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Vec<P>, &C)> {
        self.polylines.iter().zip(self.classification.iter())
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.polylines.len()
    }
}

impl<P, C> IntoIterator for Polylines<P, C> {
    type Item = (Vec<P>, C);

    type IntoIter = std::iter::Zip<std::vec::IntoIter<Vec<P>>, std::vec::IntoIter<C>>;

    fn into_iter(self) -> Self::IntoIter {
        self.polylines.into_iter().zip(self.classification)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Geometry {
    Points(Points),

    /// Polylines2 is used for 2D polylines with a classification.
    Polylines2(Polylines<Point2, Classification>),

    /// Polylines3 is used for 2D polylines with a height (z coordinate).
    Polylines3(Polylines<Point3, (Classification, f64)>), // Classification + height
}

impl From<Points> for Geometry {
    fn from(points: Points) -> Self {
        Geometry::Points(points)
    }
}
impl From<Polylines<Point2, Classification>> for Geometry {
    fn from(polylines: Polylines<Point2, Classification>) -> Self {
        Geometry::Polylines2(polylines)
    }
}
impl From<Polylines<Point3, (Classification, f64)>> for Geometry {
    fn from(polylines: Polylines<Point3, (Classification, f64)>) -> Self {
        Geometry::Polylines3(polylines)
    }
}

/// The version of the BinaryDxf file format. If any content of the [`BinaryDxf`] struct changes,
/// including any sub-fields (basically anything in this mod) we need to increase this version.
const BINARY_DXF_VERSION: usize = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BinaryDxf {
    /// the version of the program that created this file, used to detect stale temp files
    version: String,
    bounds: Bounds,
    data: Vec<Geometry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bounds {
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
}

impl Bounds {
    pub fn new(xmin: f64, xmax: f64, ymin: f64, ymax: f64) -> Self {
        Self {
            xmin,
            xmax,
            ymin,
            ymax,
        }
    }
}

impl BinaryDxf {
    pub fn new(bounds: Bounds, data: Vec<Geometry>) -> Self {
        Self {
            version: BINARY_DXF_VERSION.to_string(),
            bounds,
            data,
        }
    }

    pub fn bounds(&self) -> &Bounds {
        &self.bounds
    }

    /// Get the points in this geometry, or [`None`] if does not contain [`Polylines`] data.
    pub fn take_geometry(self) -> Vec<Geometry> {
        self.data
    }

    /// Serialize this object to a writer.
    pub fn to_writer<W: std::io::Write>(&self, writer: &mut W) -> anyhow::Result<()> {
        crate::util::write_object(writer, self)
    }
    /// Read this object from a reader. Returns an error if the version does not match.
    pub fn from_reader<R: std::io::Read>(reader: &mut R) -> anyhow::Result<Self> {
        let object: Self = crate::util::read_object(reader)?;

        // Prevously we were using the crate version, which is a string starting
        // with 2.X.X, but now we use a simple integer version. Version 1 supports all those
        // "2.X.X" versions as well.
        if (object.version == BINARY_DXF_VERSION.to_string())
            || (BINARY_DXF_VERSION == 1 && object.version.starts_with("2."))
        {
            Ok(object)
        } else {
            anyhow::bail!(
                "This DXF.BIN file version is not supported by this executable. Please re-run this command with an executable that supports this version (dxf.bin file version: {})",
                object.version,
            );
        }
    }

    /// Write this geometry to a DXF file.
    pub fn to_dxf<W: std::io::Write>(&self, writer: &mut W) -> anyhow::Result<()> {
        write!(
            writer,
            "  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$EXTMIN\r\n 10\r\n{}\r\n 20\r\n{}\r\n  9\r\n$EXTMAX\r\n 10\r\n{}\r\n 20\r\n{}\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\n",
            self.bounds.xmin, self.bounds.ymin, self.bounds.xmax, self.bounds.ymax
        )?;

        for geom in &self.data {
            match geom {
                Geometry::Points(points) => {
                    for (point, class) in points.points.iter().zip(&points.classification) {
                        let layer = class.to_layer();

                        write!(
                            writer,
                            "POINT\r\n  8\r\n{layer}\r\n 10\r\n{}\r\n 20\r\n{}\r\n 50\r\n0\r\n  0\r\n",
                            point.x, point.y
                        )?;
                    }
                }
                Geometry::Polylines2(polylines) => {
                    for (polyline, class) in
                        polylines.polylines.iter().zip(&polylines.classification)
                    {
                        let layer = class.to_layer();
                        write!(writer, "POLYLINE\r\n 66\r\n1\r\n  8\r\n{layer}\r\n  0\r\n")?;

                        for p in polyline {
                            write!(
                                writer,
                                "VERTEX\r\n  8\r\n{layer}\r\n 10\r\n{}\r\n 20\r\n{}\r\n  0\r\n",
                                p.x, p.y,
                            )?;
                        }
                        write!(writer, "SEQEND\r\n  0\r\n")?;
                    }
                }
                Geometry::Polylines3(polylines) => {
                    for (polyline, (class, height)) in
                        polylines.polylines.iter().zip(&polylines.classification)
                    {
                        let layer = class.to_layer();

                        write!(
                            writer,
                            "POLYLINE\r\n 66\r\n1\r\n  8\r\n{layer}\r\n 38\r\n{height}\r\n  0\r\n"
                        )?;

                        for p in polyline {
                            write!(
                                writer,
                                "VERTEX\r\n  8\r\n{}\r\n 10\r\n{}\r\n 20\r\n{}\r\n 30\r\n{}\r\n  0\r\n",
                                layer, p.x, p.y, height
                            )?;
                        }
                        write!(writer, "SEQEND\r\n  0\r\n")?;
                    }
                }
            }
        }

        writer.write_all("ENDSEC\r\n  0\r\nEOF\r\n".as_bytes())?;
        Ok(())
    }
}

/// Classification used for contour generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Classification {
    /// Used in first contour generation step
    ContourSimple,

    /// Used in second contour generation step (smoothjoin)
    Contour,
    ContourIndex,
    ContourIntermed,
    ContourIndexIntermed,

    Depression,
    DepressionIndex,
    DepressionIntermed,
    DepressionIndexIntermed,

    /// Use for formlines (generated in render)
    Formline,
    FormlineDepression,

    /// Used in dotknoll detections
    Dotknoll,
    Udepression,
    UglyDotknoll,
    UglyUdepression,

    /// Comes from knolldetector
    Knoll1010,

    /// Used for cliff generations
    Cliff2,
    Cliff3,
    Cliff4,
}

impl Classification {
    /// Get the layer name for this classification.
    pub fn to_layer(&self) -> &str {
        match self {
            Self::ContourSimple => "cont",

            Self::Contour => "contour",
            Self::ContourIndex => "contour_index",
            Self::ContourIntermed => "contour_intermed",
            Self::ContourIndexIntermed => "contour_index_intermed",

            Self::Depression => "depression",
            Self::DepressionIndex => "depression_index",
            Self::DepressionIntermed => "depression_intermed",
            Self::DepressionIndexIntermed => "depression_index_intermed",

            Self::Formline => "formline",
            Self::FormlineDepression => "formline_depression",

            Self::Dotknoll => "dotknoll",
            Self::Udepression => "udepression",
            Self::UglyDotknoll => "uglydotknoll",
            Self::UglyUdepression => "uglyudepression",

            Self::Knoll1010 => "1010",

            Self::Cliff2 => "cliff2",
            Self::Cliff3 => "cliff3",
            Self::Cliff4 => "cliff4",
        }
    }

    pub fn is_contour(&self) -> bool {
        matches!(
            self,
            Self::Contour | Self::ContourIndex | Self::ContourIntermed | Self::ContourIndexIntermed
        )
    }

    pub fn is_depression(&self) -> bool {
        matches!(
            self,
            Self::Depression
                | Self::DepressionIndex
                | Self::DepressionIntermed
                | Self::DepressionIndexIntermed
        )
    }

    pub fn is_index(&self) -> bool {
        matches!(
            self,
            Self::ContourIndex
                | Self::ContourIndexIntermed
                | Self::DepressionIndex
                | Self::DepressionIndexIntermed
        )
    }

    pub fn is_intermed(&self) -> bool {
        matches!(
            self,
            Self::ContourIntermed
                | Self::ContourIndexIntermed
                | Self::DepressionIntermed
                | Self::DepressionIndexIntermed
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_classification_size_is_single_byte() {
        assert_eq!(
            std::mem::size_of::<super::Classification>(),
            1,
            "Classification should be a single byte"
        );
    }
}
