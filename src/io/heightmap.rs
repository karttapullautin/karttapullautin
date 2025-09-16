use crate::vec2d::Vec2D;

use super::{bytes::FromToBytes, fs::FileSystem};

/// Simple container of a rectangular heightmap
#[derive(Debug, Clone, PartialEq)]
pub struct HeightMap {
    /// Offset to add to the x-component to get the cell coordinate.
    pub xoffset: f64,
    /// Offset to add to the y-component to get the cell coordinate.
    pub yoffset: f64,
    /// Scale to apply to get the cell coordinate.
    pub scale: f64,

    /// The actual grid data
    pub grid: Vec2D<f64>,
}

impl HeightMap {
    pub fn minx(&self) -> f64 {
        self.xoffset
    }
    pub fn miny(&self) -> f64 {
        self.yoffset
    }
    /// Get the maximum x-coordinate of the heightmap. This is the coordinate farthest away from zero (eg. the right side of the grid),
    /// which actually does not lie in a cell in the grid, but rather at the edge of the last cell.
    pub fn maxx(&self) -> f64 {
        self.xoffset + self.scale * self.grid.width() as f64
    }

    /// Get the maximum y-coordinate of the heightmap. This is the coordinate farthest away from zero (eg. the top side of the grid),
    /// which actually does not lie in a cell in the grid, but rather at the edge of the last cell.
    pub fn maxy(&self) -> f64 {
        self.yoffset + self.scale * self.grid.height() as f64
    }

    pub fn iter(&self) -> impl Iterator<Item = (f64, f64, f64)> + '_ {
        self.grid.iter().map(|(x, y, v)| {
            (
                self.xoffset + self.scale * x as f64,
                self.yoffset + self.scale * y as f64,
                v,
            )
        })
    }
}

impl HeightMap {
    /// Helper for easily reading a HeightMap from a file
    pub fn from_file<P: AsRef<std::path::Path>>(
        fs: &impl FileSystem,
        path: P,
    ) -> std::io::Result<Self> {
        let mut reader = fs.open(path)?;
        Self::from_bytes(&mut reader)
    }

    /// Helper for easily writing a HeightMap to a file
    pub fn to_file<P: AsRef<std::path::Path>>(
        &self,
        fs: &impl FileSystem,
        path: P,
    ) -> std::io::Result<()> {
        let mut file = fs.create(path)?;
        self.to_bytes(&mut file)
    }
}

impl FromToBytes for HeightMap {
    fn from_bytes<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let xoffset = f64::from_bytes(reader)?;
        let yoffset = f64::from_bytes(reader)?;
        let scale = f64::from_bytes(reader)?;
        let data = Vec2D::from_bytes(reader)?;

        Ok(HeightMap {
            xoffset,
            yoffset,
            scale,
            grid: data,
        })
    }

    fn to_bytes<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.xoffset.to_bytes(writer)?;
        self.yoffset.to_bytes(writer)?;
        self.scale.to_bytes(writer)?;
        self.grid.to_bytes(writer)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_bytes() {
        let mut data = Vec2D::new(2, 2, 0.0);
        data[(0, 0)] = 1.0;
        data[(1, 0)] = 2.0;
        data[(0, 1)] = 3.0;
        data[(1, 1)] = 4.0;

        let heightmap = super::HeightMap {
            xoffset: 3.0,
            yoffset: -5.0,
            scale: 1.5,
            grid: data,
        };

        let mut bytes = Vec::new();
        heightmap.to_bytes(&mut bytes).unwrap();
        let heightmap2 = super::HeightMap::from_bytes(&mut bytes.as_slice()).unwrap();

        assert_eq!(heightmap, heightmap2);
    }
}
