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
    ) -> Result<impl BufRead + Seek + Send + Sync + 'static, io::Error>;

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

    /// Move a file.
    fn rename(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), io::Error>;

    /// Extract a ZIP archive to a directory.
    #[cfg(feature = "shapefile")]
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
        // The `image` crate defaults to a 512 MiB allocation ceiling when decoding, which
        // is not a memory measurement — it is a guard against hostile files. Every PNG we
        // open here is one this process just wrote, and the batch merge reads whole
        // rendered tiles: a 25 km² map tripped it as `InsufficientMemory` on a machine
        // with tens of gigabytes free, after the render had already completed.
        reader.limits(image::Limits::no_limits());
        reader.decode()
    }
}
