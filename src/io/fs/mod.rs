use std::{
    io::{self, BufRead, Seek, Write},
    path::{Path, PathBuf},
};

pub mod local;
pub mod memory;

/// Trait for file system operations.
pub trait FileSystem: std::fmt::Debug {
    /// Create a new directory.
    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error>;

    /// List the contents of a directory.
    fn list(&self, path: impl AsRef<Path>) -> Result<Vec<PathBuf>, io::Error>;

    /// Check if a file exists.
    fn exists(&self, path: impl AsRef<Path>) -> bool;

    /// Open a file for reading. This is always Buffered.
    fn open(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<impl BufRead + Seek + Send + 'static, io::Error>;

    /// Open a file for writing. This is always Buffered.
    fn create(&self, path: impl AsRef<Path>) -> Result<impl Write + Seek, io::Error>;

    /// Read a file into a String.
    fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, io::Error>;

    /// Remove a file.
    fn remove_file(&self, path: impl AsRef<Path>) -> Result<(), io::Error>;

    /// Remove a dir.
    fn remove_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error>;

    /// Get the size of a file in bytes.
    fn file_size(&self, path: impl AsRef<Path>) -> Result<u64, io::Error>;

    /// Copy a file.
    fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), io::Error>;

    /// Extract a ZIP archive to a directory.
    fn extract_zip(
        &self,
        archive: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> anyhow::Result<()>;

    /// Read an image in PNG format.
    fn read_image_png(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<image::DynamicImage, image::error::ImageError> {
        let mut reader = image::ImageReader::new(self.open(path).expect("Could not open file"));
        reader.set_format(image::ImageFormat::Png);
        reader.decode()
    }

    /// Read a .shp file. Requires a neighboring .dbf file, and optionally a .shx file to exist.
    ///
    /// Implementation details from the [`shapefile::Reader::from_path`] function.
    fn read_shapefile(
        &self,
        shp_file: impl AsRef<Path>,
    ) -> anyhow::Result<
        shapefile::Reader<impl std::io::Read + std::io::Seek, impl std::io::Read + std::io::Seek>,
    > {
        let shp_file = shp_file.as_ref().to_owned();
        let dbf_path = shp_file.with_extension("dbf");
        if !self.exists(&dbf_path) {
            anyhow::bail!("DBF file not found for shapefile: {}", shp_file.display());
        }

        let dbf_reader = shapefile::dbase::Reader::new(self.open(dbf_path)?)?;

        let shx_path = shp_file.with_extension("shx");
        let shape_source = self.open(shp_file)?;
        let shape_reader = if self.exists(&shx_path) {
            shapefile::ShapeReader::with_shx(shape_source, self.open(shx_path)?)?
        } else {
            shapefile::ShapeReader::new(shape_source)?
        };

        Ok(shapefile::Reader::new(shape_reader, dbf_reader))
    }
}
