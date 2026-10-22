use std::{error::Error, path::Path};

use log::info;
use std::path::PathBuf;

use crate::{config::Config, io::fs::FileSystem};

mod canvas;
mod mapping;
mod render;

pub use render::render;

/// Unzips the shape files and renders them to a canvas.
pub fn unzip_and_render(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
    filenames: &[String],
) -> Result<(), Box<dyn Error>> {
    for zip_name in filenames.iter() {
        info!("Opening zip file {zip_name}");
        fs.extract_zip(zip_name, tmpfolder)?;
    }

    render::render(fs, config, tmpfolder, false).unwrap();

    Ok(())
}

/// Unzips the shape files to specific folder
pub fn unzip_shapefiles(fs: &impl FileSystem, filenames: &[String]) -> Result<(), Box<dyn Error>> {
    let tmpfolder = PathBuf::from("temp_shapefiles".to_string());
    for zip_name in filenames.iter() {
        info!("Opening zip file {zip_name}");
        fs.extract_zip(zip_name, &tmpfolder)?;
    }
    Ok(())
}

/// Read a .shp file. Requires a neighboring .dbf file, and optionally a .shx file to exist.
///
/// Implementation details from the [`shapefile::Reader::from_path`] function.
fn read_shapefile(
    fs: &impl FileSystem,
    shp_file: impl AsRef<Path>,
) -> anyhow::Result<
    shapefile::Reader<impl std::io::Read + std::io::Seek, impl std::io::Read + std::io::Seek>,
> {
    let shp_file = shp_file.as_ref().to_owned();
    let dbf_path = shp_file.with_extension("dbf");
    if !fs.exists(&dbf_path) {
        anyhow::bail!("DBF file not found for shapefile: {}", shp_file.display());
    }

    let dbf_reader = shapefile::dbase::Reader::new(fs.open(dbf_path)?)?;

    let shx_path = shp_file.with_extension("shx");
    let shape_source = fs.open(shp_file)?;
    let shape_reader = if fs.exists(&shx_path) {
        shapefile::ShapeReader::with_shx(shape_source, fs.open(shx_path)?)?
    } else {
        shapefile::ShapeReader::new(shape_source)?
    };

    Ok(shapefile::Reader::new(shape_reader, dbf_reader))
}
