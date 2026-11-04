//! Vector tiles (MVT in a PMTiles archive) straight from pullautin's output, replacing the Python scripts of proxy-server/mapant-tiles.
//!
//! Reads the `.dxf.bin` files of the layers (contours, formlines, cliffs, dot knolls) of every sheet run under one folder, cuts them into
//! Web Mercator tiles of one zoom level, and writes the tiles as a PMTiles archive (and optionally as `z/x/y.pbf` files).

pub mod features;
pub mod mvt;
pub mod pmtiles;
pub mod project;
pub mod raster;
pub mod simplify;
pub mod tiler;
pub mod vegetation;

use features::Features;
use log::info;
use std::path::{Path, PathBuf};
use tiler::{LayerCounts, TileBuilder};

pub struct BuildOptions {
    pub zoom: u8,
    /// also write every tile as `<out>/tiles/<z>/<x>/<y>.pbf`
    pub tile_files: bool,
    /// threads for the vegetation polygons, 0 = all cores
    pub threads: usize,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            zoom: 14,
            tile_files: true,
            threads: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct BuildStats {
    pub sheets: usize,
    pub tiles: usize,
    pub layer_counts: LayerCounts,
    pub tile_bytes: usize,
}

/// The folders of `runs` that hold a pullautin run (they have a `temp` folder), in name order
fn sheet_folders(runs: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut sheets: Vec<PathBuf> = std::fs::read_dir(runs)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("temp").is_dir())
        .collect();
    sheets.sort();
    Ok(sheets)
}

fn mercator_to_degrees(x: f64, y: f64) -> (f64, f64) {
    const R: f64 = 6_378_137.0;
    ((x / R).to_degrees(), (y / R).sinh().atan().to_degrees())
}

pub fn build(runs: &Path, out_dir: &Path, options: &BuildOptions) -> anyhow::Result<BuildStats> {
    let started = std::time::Instant::now();
    let projection = project::Tm35finToMercator::new();
    let sheets = sheet_folders(runs)?;
    anyhow::ensure!(
        !sheets.is_empty(),
        "no pullautin runs (folders with a temp folder) in {}",
        runs.display()
    );

    let mut features = Features::default();
    for sheet in &sheets {
        features.add_sheet(&sheet.join("temp"), &projection)?;
    }
    info!(
        "read {} sheets: {} contours, {} formlines, {} cliffs, {} knolls ({:.1?})",
        sheets.len(),
        features.contour.len(),
        features.formline.len(),
        features.cliff.len(),
        features.knoll.len(),
        started.elapsed()
    );

    let mosaic = raster::read_mosaic(&sheets)?;
    features.vegetation =
        vegetation::polygons(
        &mosaic,
        &vegetation::Params::default(),
        &projection,
        options.threads,
    );
    info!(
        "vegetation: {}x{} px mosaic, {} polygons ({:.1?})",
        mosaic.width,
        mosaic.height,
        features.vegetation.len(),
        started.elapsed()
    );

    let tiles = tiler::index_tiles(&features, options.zoom);
    info!("{} tiles touched", tiles.len());

    std::fs::create_dir_all(out_dir)?;
    let mut writer = pmtiles::PmtilesWriter::create(&out_dir.join("map.pmtiles"))?;
    let builder = TileBuilder {
        features: &features,
        zoom: options.zoom,
    };
    let mut stats = BuildStats {
        sheets: sheets.len(),
        ..Default::default()
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (id, tx, ty, refs) in &tiles {
        let Some(bytes) = builder.build(*tx, *ty, refs, &mut stats.layer_counts) else {
            continue;
        };
        writer.write_tile(*id, &bytes)?;
        if options.tile_files {
            let dir = out_dir
                .join("tiles")
                .join(options.zoom.to_string())
                .join(tx.to_string());
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join(format!("{ty}.pbf")), &bytes)?;
        }
        stats.tiles += 1;
        stats.tile_bytes += bytes.len();
        (min_x, min_y, max_x, max_y) = (
            min_x.min(*tx),
            min_y.min(*ty),
            max_x.max(*tx),
            max_y.max(*ty),
        );
    }
    anyhow::ensure!(stats.tiles > 0, "no tiles: the sheets hold no features");

    let size = tiler::tile_size(options.zoom);
    let corner = |tx: u32, ty: u32| {
        mercator_to_degrees(
            -project::MERCATOR_ORIGIN + tx as f64 * size,
            project::MERCATOR_ORIGIN - ty as f64 * size,
        )
    };
    let (min_lon, max_lat) = corner(min_x, min_y);
    let (max_lon, min_lat) = corner(max_x + 1, max_y + 1);
    let header = pmtiles::Header {
        min_zoom: options.zoom,
        max_zoom: options.zoom,
        min_lon,
        min_lat,
        max_lon,
        max_lat,
        center_zoom: options.zoom,
        center_lon: (min_lon + max_lon) / 2.0,
        center_lat: (min_lat + max_lat) / 2.0,
    };
    let metadata = format!(
        concat!(
            "{{\"name\":\"pullautin vector tiles\",\"format\":\"pbf\",\"minzoom\":{z},\"maxzoom\":{z},\"type\":\"overlay\",",
            "\"vector_layers\":[",
            "{{\"id\":\"contour\",\"fields\":{{\"kind\":\"String\",\"ele\":\"Number\"}}}},",
            "{{\"id\":\"formline\",\"fields\":{{}}}},",
            "{{\"id\":\"cliff\",\"fields\":{{\"kind\":\"String\"}}}},",
            "{{\"id\":\"knoll\",\"fields\":{{\"kind\":\"String\"}}}},",
            "{{\"id\":\"vegetation\",\"fields\":{{\"kind\":\"String\",\"level\":\"Number\"}}}}",
            "],\"bounds\":[{min_lon},{min_lat},{max_lon},{max_lat}]}}"
        ),
        z = options.zoom,
        min_lon = min_lon,
        min_lat = min_lat,
        max_lon = max_lon,
        max_lat = max_lat,
    );
    std::fs::write(out_dir.join("metadata.json"), &metadata)?;
    writer.finish(&header, &metadata)?;
    info!(
        "{} tiles, {:.2} MB raw, layer features {:?} ({:.1?})",
        stats.tiles,
        stats.tile_bytes as f64 / 1e6,
        stats.layer_counts.0,
        started.elapsed()
    );
    Ok(stats)
}
