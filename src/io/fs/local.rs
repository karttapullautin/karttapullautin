use std::io::{self, BufRead, BufReader, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::FileSystem;

/// [`FileSystem`] implementation for the local file system.
#[derive(Debug, Clone)]
pub struct LocalFileSystem;

impl FileSystem for LocalFileSystem {
    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        std::fs::create_dir_all(path)
    }

    fn list(&self, path: impl AsRef<Path>) -> Result<Vec<PathBuf>, io::Error> {
        let path = path.as_ref();
        let mut entries = vec![];
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            entries.push(path.join(entry.file_name()));
        }
        Ok(entries)
    }

    fn exists(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().exists()
    }

    fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, io::Error> {
        std::fs::read_to_string(path)
    }

    fn open(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<impl BufRead + Seek + Send + 'static, io::Error> {
        Ok(BufReader::with_capacity(
            crate::ONE_MEGABYTE,
            std::fs::File::open(path)?,
        ))
    }

    fn create(&self, path: impl AsRef<Path>) -> Result<impl Write + Seek, io::Error> {
        Ok(BufWriter::with_capacity(
            crate::ONE_MEGABYTE,
            std::fs::File::create(path)?,
        ))
    }

    fn remove_file(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        std::fs::remove_file(path)
    }

    fn remove_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        std::fs::remove_dir_all(path)
    }

    fn file_size(&self, path: impl AsRef<Path>) -> Result<u64, io::Error> {
        let metadata = std::fs::metadata(path)?;
        Ok(metadata.len())
    }

    fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), io::Error> {
        std::fs::copy(from, to)?;
        Ok(())
    }

    fn extract_zip(
        &self,
        archive: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let file = self.open(&archive).context("opening zip file")?;
        let mut zip_archive = zip::ZipArchive::new(file).context("reading zip archive")?;
        log::info!(
            "Extracting {:?} kB from {}",
            zip_archive.decompressed_size().map(|s| s / 1024),
            archive.as_ref().display()
        );

        zip_archive
            .extract(target)
            .context("extracting zip archive")?;

        Ok(())
    }
}
