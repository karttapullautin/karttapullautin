use image::{RgbImage, Rgba, RgbaImage};
use log::info;
use rustc_hash::FxHashMap as HashMap;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::geometry::{BinaryDxf, Classification, Geometry, Point3, Points, Polylines};
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::vec2d::Vec2D;
use image::buffer::ConvertBuffer;

fn merge_png(
    fs: &impl FileSystem,
    config: &Config,
    png_files: Vec<PathBuf>,
    outfilename: &str,
    scale: f64,
) -> Result<(), Box<dyn Error>> {
    let batchoutfolder = &config.batchoutfolder;

    let mut xmin = f64::MAX;
    let mut ymin = f64::MAX;
    let mut xmax = f64::MIN;
    let mut ymax = f64::MIN;
    let mut min_res = f64::MAX;
    for png in png_files.iter() {
        let filename = png.as_path().file_name().unwrap().to_str().unwrap();
        let full_filename = format!("{batchoutfolder}/{filename}");
        let img = fs
            .read_image_png(&full_filename)
            .expect("Opening image failed");

        let width = img.width() as f64;
        let height = img.height() as f64;
        let pgw = full_filename.replace(".png", ".pgw");
        let input = Path::new(&pgw);
        if fs.exists(input) {
            let data = fs.read_to_string(input).expect("Can not read input file");
            let d: Vec<&str> = data.split('\n').collect();
            let res = d[0].trim().parse::<f64>().unwrap();
            let tfw4 = d[4].trim().parse::<f64>().unwrap();
            let tfw5 = d[5].trim().parse::<f64>().unwrap();

            if res < min_res {
                min_res = res
            }
            if tfw4 < xmin {
                xmin = tfw4;
            }
            if (tfw4 + width * res) > xmax {
                xmax = tfw4 + width * res;
            }
            if tfw5 > ymax {
                ymax = tfw5;
            }
            if (tfw5 - height * res) < ymin {
                ymin = tfw5 - height * res;
            }
        }
    }
    let mut im = RgbaImage::from_pixel(
        ((xmax - xmin) / min_res / scale) as u32,
        ((ymax - ymin) / min_res / scale) as u32,
        Rgba([255, 255, 255, 0]),
    );
    for png in png_files.iter() {
        let filename = png.as_path().file_name().unwrap().to_str().unwrap();
        let png = format!("{batchoutfolder}/{filename}");
        let pgw = png.replace(".png", ".pgw");
        let png = Path::new(&png);
        let pgw = Path::new(&pgw);
        let filesize = fs.file_size(png).unwrap();
        if fs.exists(png) && fs.exists(pgw) && filesize > 0 {
            let img = fs.read_image_png(png).expect("Opening image failed");
            let width = img.width() as f64;
            let height = img.height() as f64;

            let data = fs.read_to_string(pgw).expect("Can not read input file");
            let d: Vec<&str> = data.split('\n').collect();

            let res = d[0].trim().parse::<f64>().unwrap();
            let tfw4 = d[4].trim().parse::<f64>().unwrap();
            let tfw5 = d[5].trim().parse::<f64>().unwrap();

            let img2 = image::imageops::thumbnail(
                &img,
                (res / min_res / scale * width + 0.5) as u32,
                (res / min_res / scale * height + 0.5) as u32,
            );

            image::imageops::overlay(
                &mut im,
                &img2,
                ((tfw4 - xmin) / min_res / scale) as i64,
                ((ymax - tfw5) / min_res / scale) as i64,
            );
        }
    }

    let im_rgb8: RgbImage = im.convert();
    im_rgb8
        .write_to(
            &mut fs
                .create(format!("{outfilename}.jpg"))
                .expect("could not save output jpg"),
            image::ImageFormat::Jpeg,
        )
        .expect("could not save output jpg");

    im.write_to(
        &mut fs
            .create(format!("{outfilename}.png"))
            .expect("could not save output png"),
        image::ImageFormat::Png,
    )
    .expect("could not save output Png");

    let mut tfw_file = fs
        .create(format!("{outfilename}.pgw"))
        .expect("Unable to create file");
    write!(
        &mut tfw_file,
        "{}\r\n0\r\n0\r\n{}\r\n{}\r\n{}\r\n",
        min_res * scale,
        -min_res * scale,
        xmin,
        ymax
    )
    .expect("Could not write to file");
    drop(tfw_file);
    fs.copy(
        Path::new(&format!("{outfilename}.pgw")),
        Path::new(&format!("{outfilename}.jgw")),
    )
    .expect("Could not copy file");
    Ok(())
}

pub fn pngmergevege(
    fs: &impl FileSystem,
    config: &Config,
    scale: f64,
    include_undergrowth: bool,
) -> Result<(), Box<dyn Error>> {
    let batchoutfolder = &config.batchoutfolder;

    let mut png_files: Vec<PathBuf> = Vec::new();
    for path in fs.list(batchoutfolder).unwrap() {
        let filename = path.file_name().unwrap().to_str().unwrap();
        if filename.ends_with("_vege.png")
            || (include_undergrowth && filename.ends_with("_undergrowth.png"))
        {
            png_files.push(path);
        }
    }
    if png_files.is_empty() {
        info!("No _vege.png files found in output directory");
        return Ok(());
    }

    let output_name = if include_undergrowth {
        "merged_vege_undergrowth"
    } else {
        "merged_vege"
    };

    merge_png(fs, config, png_files, output_name, scale).unwrap();
    Ok(())
}

pub fn pngmerge(
    fs: &impl FileSystem,
    config: &Config,
    scale: f64,
    depr: bool,
) -> Result<(), Box<dyn Error>> {
    let batchoutfolder = &config.batchoutfolder;

    let mut png_files: Vec<PathBuf> = Vec::new();
    for path in fs.list(batchoutfolder).unwrap() {
        let filename = path.file_name().unwrap().to_str().unwrap();
        if filename.ends_with(".png")
            && !filename.ends_with("_undergrowth.png")
            && !filename.ends_with("_undergrowth_bit.png")
            && !filename.ends_with("_vege.png")
            && !filename.ends_with("_vege_bit.png")
            && ((depr && filename.ends_with("_depr.png"))
                || (!depr && !filename.ends_with("_depr.png")))
        {
            png_files.push(path);
        }
    }

    if png_files.is_empty() {
        info!("No files to merge found in output directory");
        return Ok(());
    }
    let mut outfilename = "merged";
    if depr {
        outfilename = "merged_depr";
    }
    merge_png(fs, config, png_files, outfilename, scale).unwrap();
    Ok(())
}

pub fn bindxfmerge(fs: &impl FileSystem, config: &Config) -> anyhow::Result<()> {
    let batchoutfolder = &config.batchoutfolder;

    // These are the different file suffixes we expect:
    let suffixes_to_merge = [
        "contours",
        // "c2f", // No such files exist anymore
        // "c2",  // No such files exist anymore
        "c2g",
        "basemap",
        "c3g",
        "formlines",
        "dotknolls",
        "detected",
    ];

    // a list of files for each suffix
    let mut dxf_files: Vec<Vec<PathBuf>> = vec![Vec::new(); suffixes_to_merge.len()];

    for path in fs.list(batchoutfolder).unwrap() {
        if let Some(filename) = path.file_name() {
            let filename = filename.to_str().unwrap();

            // check if this file matches any of the suffiexes we expect
            for (i, suffix) in suffixes_to_merge.iter().enumerate() {
                if filename.ends_with(&format!("_{suffix}.dxf.bin")) {
                    dxf_files[i].push(path.clone());
                }
            }
        }
    }

    if dxf_files.iter().all(|f| f.is_empty()) {
        info!("No dxf files found in output directory");
        return Ok(());
    }

    // For now (originally) we always use the bounds of the first file loaded for all the generated
    // files. TODO: use the actual new bounds from the loaded files instead.
    let mut first_file_bounds = None;

    let mut all_geometries = Vec::<Geometry>::new();
    for (suffix, files) in suffixes_to_merge.iter().zip(dxf_files) {
        if files.is_empty() {
            info!("No files found for suffix: {suffix}, skipping...");
            continue;
        }

        info!("Merging {} files for suffix: {suffix}", files.len());

        let output_file = PathBuf::from(format!("merged_{suffix}.dxf.bin"));

        let mut geometries: Vec<Geometry> = Vec::with_capacity(files.len());

        for file in files {
            let loaded = BinaryDxf::from_reader(&mut fs.open(&file)?)?;

            // we always use the bounds of the first loaded file
            if first_file_bounds.is_none() {
                first_file_bounds = Some(loaded.bounds().clone());
            }

            let geometry = loaded.take_geometry();

            geometries.extend(geometry.iter().cloned());

            // for the contours, we filter out the intermediate contours for the all_geometries
            if *suffix == "contours" {
                for geo in geometry {
                    let filtered_geo: Geometry = match geo {
                        Geometry::Points(points) => {
                            let mut filtered_points = Points::with_capacity(points.len());

                            for (p, c) in points.into_iter() {
                                if !c.is_intermed() {
                                    filtered_points.push(p, c);
                                }
                            }

                            filtered_points.into()
                        }
                        Geometry::Polylines2(polylines) => {
                            let mut filtered_lines = Polylines::with_capacity(polylines.len());
                            for (l, c) in polylines.into_iter() {
                                if !c.is_intermed() {
                                    filtered_lines.push(l, c);
                                }
                            }
                            filtered_lines.into()
                        }
                        Geometry::Polylines3(polylines) => {
                            let mut filtered_lines = Polylines::with_capacity(polylines.len());
                            for (l, c) in polylines.into_iter() {
                                if !c.0.is_intermed() {
                                    filtered_lines.push(l, c);
                                }
                            }
                            filtered_lines.into()
                        }
                    };

                    all_geometries.push(filtered_geo);
                }
            } else {
                all_geometries.extend(geometry);
            }
        }

        // write output file
        let output = BinaryDxf::new(
            first_file_bounds
                .clone()
                .expect("this should be set since we load at least one file"),
            geometries,
        );
        output.to_writer(&mut fs.create(&output_file)?)?;

        if config.output_dxf {
            let output_file = PathBuf::from(format!("merged_{suffix}.dxf"));
            output.to_dxf(&mut fs.create(&output_file)?)?;
        }
    }

    // output all geometries to a single file
    if let Some(all_bounds) = first_file_bounds {
        let out_merged = BinaryDxf::new(all_bounds, all_geometries);
        out_merged.to_writer(&mut fs.create("merged.dxf.bin")?)?;

        if config.output_dxf {
            out_merged.to_dxf(&mut fs.create("merged.dxf")?)?;
        }
    }

    Ok(())
}

pub fn smoothjoin(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("Smooth curves...");

    let &Config {
        scalefactor,
        inidotknolls,
        smoothing,
        curviness,
        mut indexcontours,
        formline,
        depression_length,
        contour_interval,
        ..
    } = config;

    let halfinterval = contour_interval / 2.0 * scalefactor;
    if formline > 0.0 {
        indexcontours = 5.0 * contour_interval;
    }

    let interval = halfinterval;

    let heightmap_in = tmpfolder.join("xyz_knolls.hmap");
    let hmap = HeightMap::from_bytes(&mut fs.open(heightmap_in)?)?;

    // in world coordinates
    let xstart = hmap.xoffset;
    let ystart = hmap.yoffset;
    let size = hmap.scale;
    let xmax = (hmap.grid.width() - 1) as u64;
    let ymax = (hmap.grid.height() - 1) as u64;
    let xyz = hmap.grid;

    let mut steepness = Vec2D::new((xmax + 1) as usize, (ymax + 1) as usize, f64::NAN);

    for i in 1..xmax as usize {
        for j in 1..ymax as usize {
            let mut low: f64 = f64::MAX;
            let mut high: f64 = f64::MIN;
            for ii in i - 1..i + 2 {
                for jj in j - 1..j + 2 {
                    let tmp = xyz[(ii, jj)];
                    if tmp < low {
                        low = tmp;
                    }
                    if tmp > high {
                        high = tmp;
                    }
                }
            }
            steepness[(i, j)] = high - low;
        }
    }

    // read the binary input
    let input = tmpfolder.join("out.dxf.bin");
    let input_dxf =
        BinaryDxf::from_reader(&mut fs.open(input)?).expect("Unable to read out.dxf.bin");

    let input_bounds = input_dxf.bounds().clone(); // store the bounds for usage in the output
    let Geometry::Polylines2(input_lines) = input_dxf.take_geometry().swap_remove(0) else {
        return Err(anyhow::anyhow!("out.dxf.bin does not contain polylines").into());
    };

    let mut out2_lines = Polylines::<Point3, (Classification, f64)>::new();

    let depr_output = tmpfolder.join("depressions.txt");
    let mut depr_fp = fs.create(depr_output).expect("Unable to create file");

    let mut dotknolls = Vec::new();

    let knollhead_output = tmpfolder.join("knollheads.txt");
    let mut knollhead_fp = fs.create(knollhead_output).expect("Unable to create file");

    // Internal type used to index into the hashmaps and vectors.
    // Since using f64 coordinates directly has problems with rounding (and do not impl Eq and
    // Hash), we can use an integer representation of the coordinates to index into the HashMaps.
    // By multiplying by 1000, we can keep a precision of 3 decimal places, which is sufficient for
    // what we need.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct Key {
        x: i64,
        y: i64,
    }
    impl Key {
        fn new(x: f64, y: f64) -> Self {
            Key {
                x: (x * 1000.0) as i64,
                y: (y * 1000.0) as i64,
            }
        }
    }

    let mut heads1: HashMap<Key, usize> = HashMap::default();
    let mut heads2: HashMap<Key, usize> = HashMap::default();
    let mut heads = Vec::<Key>::with_capacity(input_lines.len());
    let mut tails = Vec::<Key>::with_capacity(input_lines.len());
    let mut el_x = Vec::<Vec<f64>>::with_capacity(input_lines.len());
    let mut el_y = Vec::<Vec<f64>>::with_capacity(input_lines.len());

    for (j, (line, _c)) in input_lines.iter().enumerate() {
        let first = line.first().unwrap();
        let last = line.last().unwrap();

        let head = Key::new(first.x, first.y);
        let tail = Key::new(last.x, last.y);

        heads.push(head);
        tails.push(tail);

        // TODO: this is not very efficient (collecting all x and y separately into Vecs), but it means the logic further down can stay the same
        el_x.push(line.iter().map(|p| p.x).collect::<Vec<_>>());
        el_y.push(line.iter().map(|p| p.y).collect::<Vec<_>>());

        if *heads1.get(&head).unwrap_or(&0) == 0 {
            heads1.insert(head, j);
        } else {
            heads2.insert(head, j);
        }
        if *heads1.get(&tail).unwrap_or(&0) == 0 {
            heads1.insert(tail, j);
        } else {
            heads2.insert(tail, j);
        }
    }

    for l in 0..input_lines.len() {
        let mut to_join = 0;
        if !el_x[l].is_empty() {
            let mut end_loop = false;
            while !end_loop {
                let tmp = *heads1.get(&heads[l]).unwrap_or(&0);
                if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                    to_join = tmp;
                } else {
                    let tmp = *heads2.get(&heads[l]).unwrap_or(&0);
                    if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                        to_join = tmp;
                    } else {
                        let tmp = *heads2.get(&tails[l]).unwrap_or(&0);
                        if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                            to_join = tmp;
                        } else {
                            let tmp = *heads1.get(&tails[l]).unwrap_or(&0);
                            if tmp != 0 && tmp != l && !el_x[tmp].is_empty() {
                                to_join = tmp;
                            } else {
                                end_loop = true;
                            }
                        }
                    }
                }
                if !end_loop {
                    if tails[l] == heads[to_join] {
                        heads2.insert(tails[l], 0);
                        heads1.insert(tails[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        el_x[l].append(&mut to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        el_y[l].append(&mut to_append);
                        tails[l] = tails[to_join];
                        el_x[to_join].clear();
                    } else if tails[l] == tails[to_join] {
                        heads2.insert(tails[l], 0);
                        heads1.insert(tails[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        to_append.reverse();
                        el_x[l].append(&mut to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        to_append.reverse();
                        el_y[l].append(&mut to_append);
                        tails[l] = heads[to_join];
                        el_x[to_join].clear();
                    } else if heads[l] == tails[to_join] {
                        heads2.insert(heads[l], 0);
                        heads1.insert(heads[l], 0);
                        let to_append = el_x[to_join].to_vec();
                        el_x[l].splice(0..0, to_append);
                        let to_append = el_y[to_join].to_vec();
                        el_y[l].splice(0..0, to_append);
                        heads[l] = heads[to_join];
                        el_x[to_join].clear();
                    } else if heads[l] == heads[to_join] {
                        heads2.insert(heads[l], 0);
                        heads1.insert(heads[l], 0);
                        let mut to_append = el_x[to_join].to_vec();
                        to_append.reverse();
                        el_x[l].splice(0..0, to_append);
                        let mut to_append = el_y[to_join].to_vec();
                        to_append.reverse();
                        el_y[l].splice(0..0, to_append);
                        heads[l] = tails[to_join];
                        el_x[to_join].clear();
                    }
                }
            }
        }
    }
    for l in 0..input_lines.len() {
        let mut el_x_len = el_x[l].len();
        if el_x_len > 0 {
            let mut skip = false;
            let mut depression = 1;
            if el_x_len < 3 {
                skip = true;
                el_x[l].clear();
            }
            let mut h = f64::NAN;
            if !skip {
                let mut mm: isize = (((el_x_len - 1) as f64) / 3.0).floor() as isize - 1;
                if mm < 0 {
                    mm = 0;
                }
                let mut m = mm as usize;
                while m < el_x_len {
                    let xm = el_x[l][m];
                    let ym = el_y[l][m];
                    if (xm - xstart) / size == ((xm - xstart) / size).floor() {
                        let xx = ((xm - xstart) / size) as usize;
                        let yy = ((ym - ystart) / size) as usize;
                        let h1 = xyz[(xx, yy)];
                        let h2 = xyz[(xx, yy + 1)];
                        let h3 = h1 * (yy as f64 + 1.0 - (ym - ystart) / size)
                            + h2 * ((ym - ystart) / size - yy as f64);
                        h = (h3 / interval + 0.5).floor() * interval;
                        m += el_x_len;
                    } else if m < el_x_len - 1
                        && (el_y[l][m] - ystart) / size == ((el_y[l][m] - ystart) / size).floor()
                    {
                        let xx = ((xm - xstart) / size) as usize;
                        let yy = ((ym - ystart) / size) as usize;
                        let h1 = xyz[(xx, yy)];
                        let h2 = xyz[(xx + 1, yy)];
                        let h3 = h1 * (xx as f64 + 1.0 - (xm - xstart) / size)
                            + h2 * ((xm - xstart) / size - xx as f64);
                        h = (h3 / interval + 0.5).floor() * interval;
                        m += el_x_len;
                    } else {
                        m += 1;
                    }
                }
            }
            if !skip
                && el_x_len < depression_length
                && el_x[l].first() == el_x[l].last()
                && el_y[l].first() == el_y[l].last()
            {
                let mut mm: isize = (((el_x_len - 1) as f64) / 3.0).floor() as isize - 1;
                if mm < 0 {
                    mm = 0;
                }
                let mut m = mm as usize;
                let mut x_avg = el_x[l][m];
                let mut y_avg = el_y[l][m];
                while m < el_x_len {
                    let xm = (el_x[l][m] - xstart) / size;
                    let ym = (el_y[l][m] - ystart) / size;
                    if m < el_x_len - 3
                        && ym == ym.floor()
                        && (xm - xm.floor()).abs() > 0.5
                        && ym.floor() != ((el_y[l][0] - ystart) / size).floor()
                        && xm.floor() != ((el_x[l][0] - xstart) / size).floor()
                    {
                        x_avg = xm.floor() * size + xstart;
                        y_avg = el_y[l][m].floor();
                        m += el_x_len;
                    }
                    m += 1;
                }
                let foo_x = ((x_avg - xstart) / size) as usize;
                let foo_y = ((y_avg - ystart) / size) as usize;

                let h_center = xyz[(foo_x, foo_y)];

                let mut hit = 0;

                let xtest = foo_x as f64 * size + xstart;
                let ytest = foo_y as f64 * size + ystart;

                let mut x0 = f64::NAN;
                let mut y0 = f64::NAN;
                for n in 0..el_x[l].len() {
                    let x1 = el_x[l][n];
                    let y1 = el_y[l][n];
                    if n > 0
                        && ((y0 <= ytest && ytest < y1) || (y1 <= ytest && ytest < y0))
                        && (xtest < (x1 - x0) * (ytest - y0) / (y1 - y0) + x0)
                    {
                        hit += 1;
                    }
                    x0 = x1;
                    y0 = y1;
                }
                depression = 1;
                if (h_center < h && hit % 2 == 1) || (h_center > h && hit % 2 != 1) {
                    depression = -1;
                    write!(&mut depr_fp, "{},{}", el_x[l][0], el_y[l][0])
                        .expect("Unable to write file");
                    for k in 1..el_x[l].len() {
                        write!(&mut depr_fp, "|{},{}", el_x[l][k], el_y[l][k])
                            .expect("Unable to write file");
                    }
                    writeln!(&mut depr_fp).expect("Unable to write file");
                }
                if !skip {
                    // Check if knoll is distinct enough
                    let mut steepcounter = 0;
                    let mut minele = f64::MAX;
                    let mut maxele = f64::MIN;
                    for k in 0..(el_x_len - 1) {
                        let xx = ((el_x[l][k] - xstart) / size + 0.5) as usize;
                        let yy = ((el_y[l][k] - ystart) / size + 0.5) as usize;
                        let ss = steepness[(xx, yy)];
                        if minele > h - 0.5 * ss {
                            minele = h - 0.5 * ss;
                        }
                        if maxele < h + 0.5 * ss {
                            maxele = h + 0.5 * ss;
                        }
                        if ss > 1.0 {
                            steepcounter += 1;
                        }
                    }

                    if (steepcounter as f64) < 0.4 * (el_x_len as f64 - 1.0)
                        && el_x_len < 41
                        && depression as f64 * h_center - 1.9 < minele
                    {
                        if maxele - 0.45 * scalefactor * inidotknolls < minele {
                            skip = true;
                        }
                        if el_x_len < 33 && maxele - 0.75 * scalefactor * inidotknolls < minele {
                            skip = true;
                        }
                        if el_x_len < 19 && maxele - 0.9 * scalefactor * inidotknolls < minele {
                            skip = true;
                        }
                    }
                    if (steepcounter as f64) < inidotknolls * (el_x_len - 1) as f64 && el_x_len < 15
                    {
                        skip = true;
                    }
                }
            }
            if el_x_len < 5 {
                skip = true;
            }
            if !skip && el_x_len < 15 {
                // dot knoll
                let mut x_avg = 0.0;
                let mut y_avg = 0.0;
                for k in 0..(el_x_len - 1) {
                    x_avg += el_x[l][k];
                    y_avg += el_y[l][k];
                }
                x_avg /= (el_x_len - 1) as f64;
                y_avg /= (el_x_len - 1) as f64;

                dotknolls.push(super::knolls::Dotknoll {
                    x: x_avg,
                    y: y_avg,
                    is_knoll: depression == 1,
                });

                skip = true;
            }

            if !skip {
                // not skipped, lets save first coordinate pair for later form line knoll PIP analysis
                write!(&mut knollhead_fp, "{} {}\r\n", el_x[l][0], el_y[l][0])
                    .expect("Unable to write to file");
                // adaptive generalization
                if el_x_len > 101 {
                    let mut newx: Vec<f64> = vec![];
                    let mut newy: Vec<f64> = vec![];
                    let mut xpre = el_x[l][0];
                    let mut ypre = el_y[l][0];

                    newx.push(el_x[l][0]);
                    newy.push(el_y[l][0]);

                    for k in 1..(el_x_len - 1) {
                        let xx = ((el_x[l][k] - xstart) / size + 0.5) as usize;
                        let yy = ((el_y[l][k] - ystart) / size + 0.5) as usize;
                        let ss = steepness[(xx, yy)];
                        if ss.is_nan() || ss < 0.5 {
                            if ((xpre - el_x[l][k]).powi(2) + (ypre - el_y[l][k]).powi(2)).sqrt()
                                >= 4.0
                            {
                                newx.push(el_x[l][k]);
                                newy.push(el_y[l][k]);
                                xpre = el_x[l][k];
                                ypre = el_y[l][k];
                            }
                        } else {
                            newx.push(el_x[l][k]);
                            newy.push(el_y[l][k]);
                            xpre = el_x[l][k];
                            ypre = el_y[l][k];
                        }
                    }
                    newx.push(el_x[l][el_x_len - 1]);
                    newy.push(el_y[l][el_x_len - 1]);

                    el_x[l].clear();
                    el_x[l].append(&mut newx);
                    el_y[l].clear();
                    el_y[l].append(&mut newy);
                    el_x_len = el_x[l].len();
                }
                // Smoothing
                let mut dx: Vec<f64> = vec![f64::NAN; el_x_len];
                let mut dy: Vec<f64> = vec![f64::NAN; el_x_len];

                for k in 2..(el_x_len - 3) {
                    dx[k] = (el_x[l][k - 2]
                        + el_x[l][k - 1]
                        + el_x[l][k]
                        + el_x[l][k + 1]
                        + el_x[l][k + 2]
                        + el_x[l][k + 3])
                        / 6.0;
                    dy[k] = (el_y[l][k - 2]
                        + el_y[l][k - 1]
                        + el_y[l][k]
                        + el_y[l][k + 1]
                        + el_y[l][k + 2]
                        + el_y[l][k + 3])
                        / 6.0;
                }

                let mut xa: Vec<f64> = vec![f64::NAN; el_x_len];
                let mut ya: Vec<f64> = vec![f64::NAN; el_x_len];
                for k in 1..(el_x_len - 1) {
                    xa[k] = (el_x[l][k - 1] + el_x[l][k] / (0.01 + smoothing) + el_x[l][k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    ya[k] = (el_y[l][k - 1] + el_y[l][k] / (0.01 + smoothing) + el_y[l][k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                }

                if el_x[l].first() == el_x[l].last() && el_y[l].first() == el_y[l].last() {
                    let vx = (el_x[l][1] + el_x[l][0] / (0.01 + smoothing) + el_x[l][el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    let vy = (el_y[l][1] + el_y[l][0] / (0.01 + smoothing) + el_y[l][el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    xa[0] = vx;
                    ya[0] = vy;
                    xa[el_x_len - 1] = vx;
                    ya[el_x_len - 1] = vy;
                } else {
                    xa[0] = el_x[l][0];
                    ya[0] = el_y[l][0];
                    xa[el_x_len - 1] = el_x[l][el_x_len - 1];
                    ya[el_x_len - 1] = el_y[l][el_x_len - 1];
                }
                for k in 1..(el_x_len - 1) {
                    el_x[l][k] = (xa[k - 1] + xa[k] / (0.01 + smoothing) + xa[k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    el_y[l][k] = (ya[k - 1] + ya[k] / (0.01 + smoothing) + ya[k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                }
                if xa.first() == xa.last() && ya.first() == ya.last() {
                    let vx = (xa[1] + xa[0] / (0.01 + smoothing) + xa[el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    let vy = (ya[1] + ya[0] / (0.01 + smoothing) + ya[el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    el_x[l][0] = vx;
                    el_y[l][0] = vy;
                    el_x[l][el_x_len - 1] = vx;
                    el_y[l][el_x_len - 1] = vy;
                } else {
                    el_x[l][0] = xa[0];
                    el_y[l][0] = ya[0];
                    el_x[l][el_x_len - 1] = xa[el_x_len - 1];
                    el_y[l][el_x_len - 1] = ya[el_x_len - 1];
                }

                for k in 1..(el_x_len - 1) {
                    xa[k] = (el_x[l][k - 1] + el_x[l][k] / (0.01 + smoothing) + el_x[l][k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    ya[k] = (el_y[l][k - 1] + el_y[l][k] / (0.01 + smoothing) + el_y[l][k + 1])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                }

                if el_x[l].first() == el_x[l].last() && el_y[l].first() == el_y[l].last() {
                    let vx = (el_x[l][1] + el_x[l][0] / (0.01 + smoothing) + el_x[l][el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    let vy = (el_y[l][1] + el_y[l][0] / (0.01 + smoothing) + el_y[l][el_x_len - 2])
                        / (2.0 + 1.0 / (0.01 + smoothing));
                    xa[0] = vx;
                    ya[0] = vy;
                    xa[el_x_len - 1] = vx;
                    ya[el_x_len - 1] = vy;
                } else {
                    xa[0] = el_x[l][0];
                    ya[0] = el_y[l][0];
                    xa[el_x_len - 1] = el_x[l][el_x_len - 1];
                    ya[el_x_len - 1] = el_y[l][el_x_len - 1];
                }

                #[allow(clippy::manual_memcpy)]
                for k in 0..el_x_len {
                    el_x[l][k] = xa[k];
                    el_y[l][k] = ya[k];
                }

                let mut dx2: Vec<f64> = vec![f64::NAN; el_x_len];
                let mut dy2: Vec<f64> = vec![f64::NAN; el_x_len];
                for k in 2..(el_x_len - 3) {
                    dx2[k] = (el_x[l][k - 2]
                        + el_x[l][k - 1]
                        + el_x[l][k]
                        + el_x[l][k + 1]
                        + el_x[l][k + 2]
                        + el_x[l][k + 3])
                        / 6.0;
                    dy2[k] = (el_y[l][k - 2]
                        + el_y[l][k - 1]
                        + el_y[l][k]
                        + el_y[l][k + 1]
                        + el_y[l][k + 2]
                        + el_y[l][k + 3])
                        / 6.0;
                }
                for k in 3..(el_x_len - 3) {
                    let vx = el_x[l][k] + (dx[k] - dx2[k]) * curviness;
                    let vy = el_y[l][k] + (dy[k] - dy2[k]) * curviness;
                    el_x[l][k] = vx;
                    el_y[l][k] = vy;
                }

                let mut layer = if depression == -1 {
                    Classification::Depression
                } else {
                    Classification::Contour
                };

                if indexcontours != 0.0
                    && (((h / interval + 0.5).floor() * interval) / indexcontours).floor()
                        - ((h / interval + 0.5).floor() * interval) / indexcontours
                        == 0.0
                {
                    // "Add" Index flag
                    layer = match layer {
                        Classification::Contour => Classification::ContourIndex,
                        Classification::Depression => Classification::DepressionIndex,
                        other => other,
                    };
                }
                if formline > 0.0
                    && (((h / interval + 0.5).floor() * interval) / (2.0 * interval)).floor()
                        - ((h / interval + 0.5).floor() * interval) / (2.0 * interval)
                        != 0.0
                {
                    // "Add" Intermed flag
                    layer = match layer {
                        Classification::Contour => Classification::ContourIntermed,
                        Classification::ContourIndex => Classification::ContourIndexIntermed,
                        Classification::Depression => Classification::DepressionIntermed,
                        Classification::DepressionIndex => Classification::DepressionIndexIntermed,
                        other => other,
                    };
                }

                out2_lines.push(
                    el_x[l]
                        .iter()
                        .zip(el_y[l].iter())
                        .map(|(&x, &y)| Point3::new(x, y, h))
                        .collect(),
                    (layer, h),
                );
            } // -- if not dotkoll
        }
    }

    crate::util::write_object(
        &mut fs.create(tmpfolder.join("dotknolls.bin"))?,
        &super::knolls::Dotknolls { dotknolls },
    )?;

    let out2_dxf = BinaryDxf::new(input_bounds, vec![out2_lines.into()]);

    let output = tmpfolder.join("out2.dxf.bin");
    let mut fp = fs.create(output).expect("Unable to create file");
    out2_dxf.to_writer(&mut fp)?;

    if config.output_dxf {
        out2_dxf.to_dxf(&mut fs.create(tmpfolder.join("out2.dxf"))?)?;
    }

    info!("Done");
    Ok(())
}
