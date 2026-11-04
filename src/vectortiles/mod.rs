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
    /// the detail zoom
    pub zoom: u8,
    /// a lighter overview zoom below it (`Features::overview`: fewer and simpler features), None = only the detail zoom
    pub overview_zoom: Option<u8>,
    /// also write every tile as `<out>/tiles/<z>/<x>/<y>.pbf`
    pub tile_files: bool,
    /// threads for the vegetation polygons, 0 = all cores
    pub threads: usize,
    /// the coordinate system of the pullautin output
    pub crs: project::Crs,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            zoom: 14,
            overview_zoom: Some(13),
            tile_files: true,
            threads: 2,
            crs: project::Crs::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct BuildStats {
    pub sheets: usize,
    /// tiles and bytes of the detail zoom
    pub tiles: usize,
    /// tiles and bytes of the overview zoom (0 without one)
    pub overview_tiles: usize,
    pub overview_bytes: usize,
    pub layer_counts: LayerCounts,
    pub tile_bytes: usize,
}

fn write_tile_file(
    out_dir: &Path,
    options: &BuildOptions,
    zoom: u8,
    tx: u32,
    ty: u32,
    bytes: &[u8],
) -> std::io::Result<()> {
    if !options.tile_files {
        return Ok(());
    }
    let dir = out_dir
        .join("tiles")
        .join(zoom.to_string())
        .join(tx.to_string());
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{ty}.pbf")), bytes)
}

/// The year of the laser scanning of a sheet: the number in `scan_year.txt` of the sheet's folder (or its `temp` folder), else the two digits
/// after the `y` that starts the sheet's name (Lantmäteriet's `y25g017-...` is 2025), else None
pub fn sheet_scan_year(folder: &Path) -> Option<u16> {
    let valid = |y: u16| (2000..=2063).contains(&y).then_some(y);
    for path in [
        folder.join("scan_year.txt"),
        folder.join("temp").join("scan_year.txt"),
    ] {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some(year) = text.split_whitespace().next().and_then(|t| t.parse().ok())
        {
            return valid(year);
        }
    }
    let name = folder.file_name()?.to_str()?.as_bytes();
    match name {
        [b'y', a, b, rest @ ..]
            if a.is_ascii_digit()
                && b.is_ascii_digit()
                && !rest.first().is_some_and(|c| c.is_ascii_digit()) =>
        {
            valid(2000 + ((a - b'0') as u16) * 10 + (b - b'0') as u16)
        }
        _ => None,
    }
}

/// For each tile of a zoom the first and last scan year of the sheets that touch it (by the sheet's bounds)
fn scan_years_by_tile(
    sheets: &[PathBuf],
    projection: &project::Tm35finToMercator,
    zoom: u8,
) -> std::collections::HashMap<(u32, u32), (u16, u16)> {
    let mut years = std::collections::HashMap::new();
    for sheet in sheets {
        let (Some(year), Ok((x0, y0, x1, y1))) =
            (sheet_scan_year(sheet), raster::sheet_bounds(sheet))
        else {
            continue;
        };
        let corners =
            [(x0, y0), (x0, y1), (x1, y0), (x1, y1)].map(|(x, y)| projection.to_mercator(x, y));
        let bounds = (
            corners.iter().map(|c| c.0).fold(f64::MAX, f64::min),
            corners.iter().map(|c| c.1).fold(f64::MAX, f64::min),
            corners.iter().map(|c| c.0).fold(f64::MIN, f64::max),
            corners.iter().map(|c| c.1).fold(f64::MIN, f64::max),
        );
        let (xs, ys) = tiler::tiles_of_bounds(bounds, zoom);
        for tx in xs {
            for ty in ys.clone() {
                let e = years.entry((tx, ty)).or_insert((year, year));
                *e = (e.0.min(year), e.1.max(year));
            }
        }
    }
    years
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
    let projection = options.crs.projection();
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
    features.vegetation = vegetation::polygons(
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

    std::fs::create_dir_all(out_dir)?;
    let mut writer = pmtiles::PmtilesWriter::create(&out_dir.join("map.pmtiles"))?;
    let mut stats = BuildStats {
        sheets: sheets.len(),
        ..Default::default()
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);

    // The tile ids of a lower zoom are smaller (PMTiles orders the tiles by id), so the overview goes first.
    let overview = options
        .overview_zoom
        .filter(|&z| z < options.zoom)
        .map(|z| (z, features.overview()));
    if let Some((z, lighter)) = &overview {
        let tiles = tiler::index_tiles(lighter, *z);
        let years = scan_years_by_tile(&sheets, &projection, *z);
        let builder = TileBuilder {
            features: lighter,
            zoom: *z,
            years: Some(&years),
        };
        let mut counts = tiler::LayerCounts::default();
        for (id, tx, ty, refs) in &tiles {
            let Some(bytes) = builder.build(*tx, *ty, refs, &mut counts) else {
                continue;
            };
            writer.write_tile(*id, &bytes)?;
            write_tile_file(out_dir, options, *z, *tx, *ty, &bytes)?;
            stats.overview_tiles += 1;
            stats.overview_bytes += bytes.len();
        }
        info!(
            "overview z{z}: {} tiles, {:.2} MB raw, layer features {:?} ({:.1?})",
            stats.overview_tiles,
            stats.overview_bytes as f64 / 1e6,
            counts.0,
            started.elapsed()
        );
    }

    let tiles = tiler::index_tiles(&features, options.zoom);
    info!("{} tiles touched", tiles.len());
    let years = scan_years_by_tile(&sheets, &projection, options.zoom);
    let builder = TileBuilder {
        features: &features,
        zoom: options.zoom,
        years: Some(&years),
    };
    for (id, tx, ty, refs) in &tiles {
        let Some(bytes) = builder.build(*tx, *ty, refs, &mut stats.layer_counts) else {
            continue;
        };
        writer.write_tile(*id, &bytes)?;
        write_tile_file(out_dir, options, options.zoom, *tx, *ty, &bytes)?;
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
    let min_zoom = overview.as_ref().map_or(options.zoom, |(z, _)| *z);
    let header = pmtiles::Header {
        min_zoom,
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
            "{{\"name\":\"pullautin vector tiles\",\"format\":\"pbf\",\"minzoom\":{minz},\"maxzoom\":{z},\"type\":\"overlay\",",
            "\"vector_layers\":[",
            "{{\"id\":\"contour\",\"fields\":{{\"kind\":\"String\",\"ele\":\"Number\"}}}},",
            "{{\"id\":\"formline\",\"fields\":{{}}}},",
            "{{\"id\":\"cliff\",\"fields\":{{\"kind\":\"String\"}}}},",
            "{{\"id\":\"knoll\",\"fields\":{{\"kind\":\"String\"}}}},",
            "{{\"id\":\"vegetation\",\"fields\":{{\"kind\":\"String\",\"level\":\"Number\"}}}}",
            "],\"bounds\":[{min_lon},{min_lat},{max_lon},{max_lat}]}}"
        ),
        z = options.zoom,
        minz = min_zoom,
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

#[cfg(test)]
mod scan_year_tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vt_scan_year_{}_{name}", std::process::id()));
        std::fs::create_dir_all(p.join("temp")).unwrap();
        p
    }

    #[test]
    fn the_scan_year_is_in_scan_year_txt_or_in_the_name_of_a_lantmateriet_sheet() {
        let named = std::env::temp_dir().join("y25g017-71750_7825_25");
        assert_eq!(sheet_scan_year(&named), Some(2025));
        assert_eq!(
            sheet_scan_year(&std::env::temp_dir().join("y20f036-70850_7550_25")),
            Some(2020)
        );
        // an MML sheet name has no year
        assert_eq!(sheet_scan_year(&std::env::temp_dir().join("M4211E1")), None);
        // the file wins over the name, in the folder or in temp
        let f = folder("file");
        std::fs::write(f.join("scan_year.txt"), "2008\n").unwrap();
        assert_eq!(sheet_scan_year(&f), Some(2008));
        let g = folder("temp");
        std::fs::write(g.join("temp").join("scan_year.txt"), "2019 laser").unwrap();
        assert_eq!(sheet_scan_year(&g), Some(2019));
        // a year the encoding cannot hold is no year
        let h = folder("bad");
        std::fs::write(h.join("scan_year.txt"), "1985").unwrap();
        assert_eq!(sheet_scan_year(&h), None);
        for p in [f, g, h] {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}
