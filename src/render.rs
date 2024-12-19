use crate::config::Config;
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use image::ImageBuffer;
use image::Rgba;
use imageproc::drawing::{draw_filled_circle_mut, draw_line_segment_mut};
use log::info;
use rustc_hash::FxHashMap as HashMap;
use std::error::Error;
use std::f64::consts::PI;
use std::io::BufRead;
use std::io::BufReader;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn render(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    angle_deg: f64,
    nwidth: usize,
    nodepressions: bool,
) -> Result<(), Box<dyn Error>> {
    info!("Rendering...");

    let scalefactor = config.scalefactor;

    let angle = -angle_deg / 180.0 * PI;

    // Draw vegetation ----------
    let tfw_in = tmpfolder.join("vegetation.pgw");
    let mut lines = BufReader::new(fs.open(tfw_in).expect("PGW file does not exist")).lines();
    let x0 = lines
        .nth(4)
        .expect("no 4 line")
        .expect("Could not read line 5")
        .parse::<f64>()
        .unwrap();
    let y0 = lines
        .next()
        .expect("no 5 line")
        .expect("Could not read line 6")
        .parse::<f64>()
        .unwrap();

    let mut img_reader = image::ImageReader::new(BufReader::new(
        fs.open(tmpfolder.join("vegetation.png"))
            .expect("Opening vegetation image failed"),
    ));
    img_reader.set_format(image::ImageFormat::Png);
    img_reader.no_limits();
    let img = img_reader.decode().unwrap();

    let mut imgug_reader = image::ImageReader::new(BufReader::new(
        fs.open(tmpfolder.join("undergrowth.png"))
            .expect("Opening undergrowth image failed"),
    ));
    imgug_reader.set_format(image::ImageFormat::Png);
    imgug_reader.no_limits();
    let imgug = imgug_reader.decode().unwrap();

    let w = img.width();
    let h = img.height();

    let eastoff = -((x0 - (-angle).tan() * y0)
        - ((x0 - (-angle).tan() * y0) / (250.0 / angle.cos())).floor() * (250.0 / angle.cos()))
        / 254.0
        * 600.0;

    let new_width = (w as f64 * 600.0 / 254.0 / scalefactor) as u32;
    let new_height = (h as f64 * 600.0 / 254.0 / scalefactor) as u32;
    let mut img = image::imageops::resize(
        &img,
        new_width,
        new_height,
        image::imageops::FilterType::Nearest,
    );

    let imgug = image::imageops::resize(
        &imgug,
        new_width,
        new_height,
        image::imageops::FilterType::Nearest,
    );

    image::imageops::overlay(&mut img, &imgug, 0, 0);

    let low_file = tmpfolder.join("low.png");
    if fs.exists(&low_file) {
        let mut low_reader = image::ImageReader::new(BufReader::new(
            fs.open(low_file).expect("Opening low image failed"),
        ));
        low_reader.set_format(image::ImageFormat::Png);
        low_reader.no_limits();
        let low = low_reader.decode().unwrap();
        let low = image::imageops::resize(
            &low,
            new_width,
            new_height,
            image::imageops::FilterType::Nearest,
        );
        image::imageops::overlay(&mut img, &low, 0, 0);
    }

    // north lines ----------------
    if angle != 999.0 {
        let mut i: f64 = eastoff - 600.0 * 250.0 / 254.0 / angle.cos() * 100.0 / scalefactor;
        while i < w as f64 * 5.0 * 600.0 / 254.0 / scalefactor {
            for m in 0..nwidth {
                draw_line_segment_mut(
                    &mut img,
                    (i as f32 + m as f32, 0.0),
                    (
                        (i as f32
                            + (angle.tan() * (h as f64) * 600.0 / 254.0 / scalefactor) as f32)
                            + m as f32,
                        (h as f32 * 600.0 / 254.0 / scalefactor as f32),
                    ),
                    Rgba([0, 0, 200, 255]),
                );
            }
            i += 600.0 * 250.0 / 254.0 / angle.cos() / scalefactor;
        }
    }

    draw_curves(fs, config, &mut img, tmpfolder, nodepressions, true).unwrap();

    // dotknolls----------
    let input = tmpfolder.join("dotknolls.dxf");
    let data = fs.read_to_string(input).expect("Can not read input file");
    let data = data.split("POINT");

    for (j, rec) in data.enumerate() {
        let mut x: f64 = 0.0;
        let mut y: f64 = 0.0;
        if j > 0 {
            let val = rec.split('\n').collect::<Vec<&str>>();
            let layer = val[2].trim();
            for (i, v) in val.iter().enumerate() {
                let vt = v.trim_end();
                if vt == " 10" {
                    x = (val[i + 1].trim().parse::<f64>().unwrap() - x0) * 600.0
                        / 254.0
                        / scalefactor;
                }
                if vt == " 20" {
                    y = (y0 - val[i + 1].trim().parse::<f64>().unwrap()) * 600.0
                        / 254.0
                        / scalefactor;
                }
            }
            if layer == "dotknoll" {
                let color = Rgba([166, 85, 43, 255]);

                draw_filled_circle_mut(&mut img, (x as i32, y as i32), 7, color)
            }
        }
    }
    // blocks -------------
    let blocks_file = tmpfolder.join("blocks.png");
    if fs.exists(&blocks_file) {
        let mut blockpurple_reader = image::ImageReader::new(BufReader::new(
            fs.open(blocks_file).expect("Opening blocks image failed"),
        ));
        blockpurple_reader.set_format(image::ImageFormat::Png);
        blockpurple_reader.no_limits();
        let blockpurple = blockpurple_reader.decode().unwrap();
        let mut blockpurple = blockpurple.to_rgba8();
        for p in blockpurple.pixels_mut() {
            if p[0] == 255 && p[1] == 255 && p[2] == 255 {
                p[3] = 0;
            }
        }
        let blockpurple = image::imageops::crop(&mut blockpurple, 0, 0, w, h).to_image();
        let blockpurple_thumb = image::imageops::resize(
            &blockpurple,
            new_width,
            new_height,
            image::imageops::FilterType::Nearest,
        );

        for i in 0..3 {
            for j in 0..3 {
                image::imageops::overlay(
                    &mut img,
                    &blockpurple_thumb,
                    (i as i64 - 1) * 2,
                    (j as i64 - 1) * 2,
                );
            }
        }
        image::imageops::overlay(&mut img, &blockpurple_thumb, 0, 0);
    }
    // blueblack -------------
    let blueblack_file = tmpfolder.join("blueblack.png");
    if fs.exists(&blueblack_file) {
        let mut imgbb_reader = image::ImageReader::new(BufReader::new(
            fs.open(blueblack_file)
                .expect("Opening blueblack image failed"),
        ));
        imgbb_reader.set_format(image::ImageFormat::Png);
        imgbb_reader.no_limits();
        let imgbb = imgbb_reader.decode().unwrap();
        let mut imgbb = imgbb.to_rgba8();
        for p in imgbb.pixels_mut() {
            if p[0] == 255 && p[1] == 255 && p[2] == 255 {
                p[3] = 0;
            }
        }
        let imgbb = image::imageops::crop(&mut imgbb, 0, 0, w, h).to_image();
        let imgbb_thumb = image::imageops::resize(
            &imgbb,
            new_width,
            new_height,
            image::imageops::FilterType::Nearest,
        );
        image::imageops::overlay(&mut img, &imgbb_thumb, 0, 0);
    }

    let black = Rgba([0, 0, 0, 255]);

    let mut cliffcolor =
        HashMap::from_iter([("cliff2", black), ("cliff3", black), ("cliff4", black)]);
    if config.cliffdebug {
        cliffcolor = HashMap::from_iter([
            ("cliff2", Rgba([100, 0, 100, 255])),
            ("cliff3", Rgba([0, 100, 100, 255])),
            ("cliff4", Rgba([100, 100, 0, 255])),
        ]);
    }
    let input = tmpfolder.join("c2g.dxf");
    let data = fs.read_to_string(input).expect("Can not read input file");
    let data: Vec<&str> = data.split("POLYLINE").collect();

    for (j, rec) in data.iter().enumerate() {
        let mut x = Vec::<f64>::new();
        let mut y = Vec::<f64>::new();
        let mut xline = 0;
        let mut yline = 0;
        let mut layer = "";
        if j > 0 {
            let r = rec.split("VERTEX").collect::<Vec<&str>>();
            let apu = r[1];
            let val = apu.split('\n').collect::<Vec<&str>>();
            layer = val[2].trim();
            for (i, v) in val.iter().enumerate() {
                let vt = v.trim_end();
                if vt == " 10" {
                    xline = i + 1;
                }
                if vt == " 20" {
                    yline = i + 1;
                }
            }
            for (i, v) in r.iter().enumerate() {
                if i > 0 {
                    let val = v.trim_end().split('\n').collect::<Vec<&str>>();
                    x.push(
                        (val[xline].trim().parse::<f64>().unwrap() - x0) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                    y.push(
                        (y0 - val[yline].trim().parse::<f64>().unwrap()) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                }
            }
        }
        let last_idx = x.len() - 1;
        if x.first() != x.last() || y.first() != y.last() {
            let dist = ((x[0] - x[last_idx]).powi(2) + (y[0] - y[last_idx]).powi(2)).sqrt();
            if dist > 0.0 {
                let dx = x[0] - x[last_idx];
                let dy = y[0] - y[last_idx];
                x[0] += dx / dist * 1.5;
                y[0] += dy / dist * 1.5;
                x[last_idx] -= dx / dist * 1.5;
                y[last_idx] -= dy / dist * 1.5;
                draw_filled_circle_mut(
                    &mut img,
                    (x[0] as i32, y[0] as i32),
                    3,
                    *cliffcolor.get(&layer).unwrap_or(&black),
                );
                draw_filled_circle_mut(
                    &mut img,
                    (x[last_idx] as i32, y[last_idx] as i32),
                    3,
                    *cliffcolor.get(&layer).unwrap_or(&black),
                );
            }
        }
        for i in 1..x.len() {
            for n in 0..6 {
                for m in 0..6 {
                    draw_line_segment_mut(
                        &mut img,
                        (
                            (x[i - 1] + (n as f64) - 3.0).floor() as f32,
                            (y[i - 1] + (m as f64) - 3.0).floor() as f32,
                        ),
                        (
                            (x[i] + (n as f64) - 3.0).floor() as f32,
                            (y[i] + (m as f64) - 3.0).floor() as f32,
                        ),
                        *cliffcolor.get(&layer).unwrap_or(&black),
                    )
                }
            }
        }
    }

    let input = &tmpfolder.join("c3g.dxf");
    let data = fs.read_to_string(input).expect("Can not read input file");
    let data: Vec<&str> = data.split("POLYLINE").collect();

    for (j, rec) in data.iter().enumerate() {
        let mut x = Vec::<f64>::new();
        let mut y = Vec::<f64>::new();
        let mut xline = 0;
        let mut yline = 0;
        let mut layer = "";
        if j > 0 {
            let r = rec.split("VERTEX").collect::<Vec<&str>>();
            let apu = r[1];
            let val = apu.split('\n').collect::<Vec<&str>>();
            layer = val[2].trim();
            for (i, v) in val.iter().enumerate() {
                let vt = v.trim_end();
                if vt == " 10" {
                    xline = i + 1;
                }
                if vt == " 20" {
                    yline = i + 1;
                }
            }
            for (i, v) in r.iter().enumerate() {
                if i > 0 {
                    let val = v.trim_end().split('\n').collect::<Vec<&str>>();
                    x.push(
                        (val[xline].trim().parse::<f64>().unwrap() - x0) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                    y.push(
                        (y0 - val[yline].trim().parse::<f64>().unwrap()) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                }
            }
        }
        let last_idx = x.len() - 1;
        if x.first() != x.last() || y.first() != y.last() {
            let dist = ((x[0] - x[last_idx]).powi(2) + (y[0] - y[last_idx]).powi(2)).sqrt();
            if dist > 0.0 {
                let dx = x[0] - x[last_idx];
                let dy = y[0] - y[last_idx];
                x[0] += dx / dist * 1.5;
                y[0] += dy / dist * 1.5;
                x[last_idx] -= dx / dist * 1.5;
                y[last_idx] -= dy / dist * 1.5;

                draw_filled_circle_mut(
                    &mut img,
                    (x[0] as i32, y[0] as i32),
                    3,
                    *cliffcolor.get(&layer).unwrap_or(&black),
                );
                draw_filled_circle_mut(
                    &mut img,
                    (x[last_idx] as i32, y[last_idx] as i32),
                    3,
                    *cliffcolor.get(&layer).unwrap_or(&black),
                );
            }
        }
        for i in 1..x.len() {
            for n in 0..6 {
                for m in 0..6 {
                    draw_line_segment_mut(
                        &mut img,
                        (
                            (x[i - 1] + (n as f64) - 3.0).floor() as f32,
                            (y[i - 1] + (m as f64) - 3.0).floor() as f32,
                        ),
                        (
                            (x[i] + (n as f64) - 3.0).floor() as f32,
                            (y[i] + (m as f64) - 3.0).floor() as f32,
                        ),
                        *cliffcolor.get(&layer).unwrap_or(&black),
                    )
                }
            }
        }
    }
    // high -------------
    let high_file = tmpfolder.join("high.png");
    if fs.exists(&high_file) {
        let mut high_reader = image::ImageReader::new(BufReader::new(
            fs.open(high_file).expect("Opening high image failed"),
        ));
        high_reader.set_format(image::ImageFormat::Png);
        high_reader.no_limits();
        let high = high_reader.decode().unwrap();
        let high_thumb = image::imageops::resize(
            &high,
            new_width,
            new_height,
            image::imageops::FilterType::Nearest,
        );
        image::imageops::overlay(&mut img, &high_thumb, 0, 0);
    }

    let filename = if nodepressions {
        format!("pullautus{}", thread)
    } else {
        format!("pullautus_depr{}", thread)
    };

    img.write_to(
        &mut BufWriter::new(
            fs.create(format!("{}.png", filename))
                .expect("could not save output png"),
        ),
        image::ImageFormat::Png,
    )
    .expect("could not write image");

    let file_in = tmpfolder.join("vegetation.pgw");
    let pgw_file_out = fs
        .create(format!("{}.pgw", filename))
        .expect("Unable to create file");
    let mut pgw_file_out = BufWriter::new(pgw_file_out);

    if let Ok(lines) = fs.open(file_in) {
        for (i, line) in BufReader::new(lines).lines().enumerate() {
            let ip = line.unwrap_or(String::new());
            let x: f64 = ip.parse::<f64>().unwrap();
            if i == 0 || i == 3 {
                write!(&mut pgw_file_out, "{}\r\n", x / 600.0 * 254.0 * scalefactor)
                    .expect("Unable to write to file");
            } else {
                write!(&mut pgw_file_out, "{}\r\n", ip).expect("Unable to write to file");
            }
        }
    }
    info!("Done");
    Ok(())
}

pub fn draw_curves(
    fs: &impl FileSystem,
    config: &Config,
    canvas: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    tmpfolder: &Path,
    nodepressions: bool,
    draw_image: bool,
) -> Result<(), Box<dyn Error>> {
    // Drawing curves --------------
    let &Config {
        scalefactor,
        mut formlinesteepness,
        formline,
        formlineaddition,
        dashlength,
        gaplength,
        minimumgap,
        label_depressions,
        remove_touching_contours,
        ..
    } = config;
    formlinesteepness *= scalefactor;

    let mut size: f64 = 0.0;
    let mut xstart: f64 = 0.0;
    let mut ystart: f64 = 0.0;
    let tfw_in = tmpfolder.join("vegetation.pgw");
    let mut lines = BufReader::new(fs.open(tfw_in).expect("PGW file does not exist")).lines();
    let x0 = lines
        .nth(4)
        .expect("no 4 line")
        .expect("Could not read line 5")
        .parse::<f64>()
        .unwrap();
    let y0 = lines
        .next()
        .expect("no 5 line")
        .expect("Could not read line 6")
        .parse::<f64>()
        .unwrap();
    let mut steepness: HashMap<(usize, usize), f64> = HashMap::default();

    let heightmap_in = tmpfolder.join("xyz2.hmap");
    let mut reader = BufReader::new(fs.open(heightmap_in)?);
    let hmap = HeightMap::from_bytes(&mut reader)?;
    let xyz = &hmap.grid;

    if formline > 0.0 {
        xstart = hmap.xoffset;
        ystart = hmap.yoffset;
        size = hmap.scale;

        let sxmax = hmap.grid.width() - 1;
        let symax = hmap.grid.height() - 1;

        for i in 6..(sxmax - 7) {
            for j in 6..(symax - 7) {
                let mut det: f64 = 0.0;
                let mut high: f64 = f64::MIN;

                let mut temp = (xyz[(i - 4, j)] - xyz[(i, j)]).abs() / 4.0;
                let temp2 = (xyz[(i, j)] - xyz[(i + 4, j)]).abs() / 4.0;
                let det2 = (xyz[(i, j)] - 0.5 * (xyz[(i - 4, j)] + xyz[(i + 4, j)])).abs()
                    - 0.05 * (xyz[(i - 4, j)] - xyz[(i + 4, j)]).abs();
                let mut porr = (((xyz[(i - 6, j)] - xyz[(i + 6, j)]) / 12.0).abs()
                    - ((xyz[(i - 3, j)] - xyz[(i + 3, j)]) / 6.0).abs())
                .abs();

                if det2 > det {
                    det = det2;
                }
                if temp2 < temp {
                    temp = temp2;
                }
                if temp > high {
                    high = temp;
                }

                let mut temp = (xyz[(i, j - 4)] - xyz[(i, j)]).abs() / 4.0;
                let temp2 = (xyz[(i, j)] - xyz[(i, j - 4)]).abs() / 4.0;
                let det2 = (xyz[(i, j)] - 0.5 * (xyz[(i, j - 4)] + xyz[(i, j + 4)])).abs()
                    - 0.05 * (xyz[(i, j - 4)] - xyz[(i, j + 4)]).abs();
                let porr2 = (((xyz[(i, j - 6)] - xyz[(i, j + 6)]) / 12.0).abs()
                    - ((xyz[(i, j - 3)] - xyz[(i, j + 3)]) / 6.0).abs())
                .abs();

                if porr2 > porr {
                    porr = porr2;
                }
                if det2 > det {
                    det = det2;
                }
                if temp2 < temp {
                    temp = temp2;
                }
                if temp > high {
                    high = temp;
                }

                let mut temp = (xyz[(i - 4, j - 4)] - xyz[(i, j)]).abs() / 5.6;
                let temp2 = (xyz[(i, j)] - xyz[(i + 4, j + 4)]).abs() / 5.6;
                let det2 = (xyz[(i, j)] - 0.5 * (xyz[(i - 4, j - 4)] + xyz[(i + 4, j + 4)])).abs()
                    - 0.05 * (xyz[(i - 4, j - 4)] - xyz[(i + 4, j + 4)]).abs();
                let porr2 = (((xyz[(i - 6, j - 6)] - xyz[(i + 6, j + 6)]) / 17.0).abs()
                    - ((xyz[(i - 3, j - 3)] - xyz[(i + 3, j + 3)]) / 8.5).abs())
                .abs();

                if porr2 > porr {
                    porr = porr2;
                }
                if det2 > det {
                    det = det2;
                }
                if temp2 < temp {
                    temp = temp2;
                }
                if temp > high {
                    high = temp;
                }

                let mut temp = (xyz[(i - 4, j + 4)] - xyz[(i, j)]).abs() / 5.6;
                let temp2 = (xyz[(i, j)] - xyz[(i + 4, j - 4)]).abs() / 5.6;
                let det2 = (xyz[(i, j)] - 0.5 * (xyz[(i + 4, j - 4)] + xyz[(i - 4, j + 4)])).abs()
                    - 0.05 * (xyz[(i + 4, j - 4)] - xyz[(i - 4, j + 4)]).abs();
                let porr2 = (((xyz[(i + 6, j - 6)] - xyz[(i - 6, j + 6)]) / 17.0).abs()
                    - ((xyz[(i + 3, j - 3)] - xyz[(i - 3, j + 3)]) / 8.5).abs())
                .abs();

                if porr2 > porr {
                    porr = porr2;
                }
                if det2 > det {
                    det = det2;
                }
                if temp2 < temp {
                    temp = temp2;
                }
                if temp > high {
                    high = temp;
                }

                let mut val = 12.0 * high / (1.0 + 8.0 * det);
                if porr > 0.25 * 0.67 / (0.3 + formlinesteepness) {
                    val = 0.01;
                }
                if high > val {
                    val = high;
                }
                steepness.insert((i, j), val);
            }
        }
    }

    let input = &tmpfolder.join("out2.dxf");
    let data = fs.read_to_string(input).expect("Can not read input file");
    let data: Vec<&str> = data.split("POLYLINE").collect();

    // only create the file if condition is met
    let mut fp = if formline == 2.0 && !nodepressions {
        let output = tmpfolder.join("formlines.dxf");
        let fp = fs.create(output).expect("Unable to create file");
        let mut fp = BufWriter::new(fp);
        fp.write_all(data[0].as_bytes())
            .expect("Could not write file");

        Some(fp)
    } else {
        None
    };

    for (j, rec) in data.iter().enumerate() {
        let mut x = Vec::<f64>::new();
        let mut y = Vec::<f64>::new();
        let mut xline = 0;
        let mut yline = 0;
        let mut layer = "";
        if j > 0 {
            let r = rec.split("VERTEX").collect::<Vec<&str>>();
            let apu = r[1];
            let val = apu.split('\n').collect::<Vec<&str>>();
            layer = val[2].trim();
            for (i, v) in val.iter().enumerate() {
                let vt = v.trim_end();
                if vt == " 10" {
                    xline = i + 1;
                }
                if vt == " 20" {
                    yline = i + 1;
                }
            }
            for (i, v) in r.iter().enumerate() {
                if i > 0 {
                    let val = v.trim_end().split('\n').collect::<Vec<&str>>();
                    x.push(
                        (val[xline].trim().parse::<f64>().unwrap() - x0) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                    y.push(
                        (y0 - val[yline].trim().parse::<f64>().unwrap()) * 600.0
                            / 254.0
                            / scalefactor,
                    );
                }
            }
        }
        let mut color = Rgba([200, 0, 200, 255]); // purple
        if layer.contains("contour") {
            color = Rgba([166, 85, 43, 255]) // brown
        }

        if !nodepressions || layer.contains("contour") {
            let mut curvew = 2.0;
            if layer.contains("index") {
                curvew = 3.0;
            }
            if formline > 0.0 {
                if formline == 1.0 {
                    curvew = 2.5
                }
                if layer.contains("intermed") {
                    curvew = 1.0
                }
                if layer.contains("index") {
                    curvew = 3.0
                }
            }

            let mut smallringtest = false;
            let mut help = vec![false; x.len()];
            let mut help2 = vec![false; x.len()];
            let mut help3 = vec![false; x.len()];
            if curvew == 1.0 {
                for i in 0..x.len() {
                    help[i] = false;
                    help2[i] = true;
                    help3[i] = false;
                    let xx = (((x[i] / 600.0 * 254.0 * scalefactor + x0) - xstart) / size).floor()
                        as usize;
                    let yy = (((-y[i] / 600.0 * 254.0 * scalefactor + y0) - ystart) / size).floor()
                        as usize;
                    if curvew != 1.0
                        || formline == 0.0
                        || steepness.get(&(xx, yy)).unwrap_or(&0.0) < &formlinesteepness
                        || steepness.get(&(xx, yy + 1)).unwrap_or(&0.0) < &formlinesteepness
                        || steepness.get(&(xx + 1, yy)).unwrap_or(&0.0) < &formlinesteepness
                        || steepness.get(&(xx + 1, yy + 1)).unwrap_or(&0.0) < &formlinesteepness
                    {
                        help[i] = true;
                    }
                    if formline == 0.0
                        || ((xyz[(xx - 1, yy)] - xyz[(xx + 1, yy)]).abs() < 2.5
                            && (xyz[(xx, yy - 1)] - xyz[(xx, yy + 1)]).abs() < 2.5
                            && (xyz[(xx, yy)] - xyz[(xx + 1, yy + 1)]).abs() < 3.5
                            && (xyz[(xx - 1, yy - 1)] - xyz[(xx + 1, yy + 1)]).abs() < 3.5
                            && (xyz[(xx + 1, yy - 1)] - xyz[(xx - 1, yy + 1)]).abs() < 3.5)
                    {
                        help3[i] = true;
                    }
                }
                for i in 5..(x.len() - 6) {
                    let mut apu = 0;
                    for j in (i - 5)..(i + 4) {
                        if help[j] {
                            apu += 1;
                        }
                    }
                    if apu < 5 {
                        help2[i] = false;
                    }
                }
                for i in 0..6 {
                    help2[i] = help2[6]
                }
                for i in (x.len() - 6)..x.len() {
                    help2[i] = help2[x.len() - 7]
                }
                let mut on = 0.0;
                for i in 0..x.len() {
                    if help2[i] {
                        on = formlineaddition
                    }
                    if on > 0.0 {
                        help2[i] = true;
                        on -= 1.0;
                    }
                }
                if x.first() == x.last() && y.first() == y.last() && on > 0.0 {
                    let mut i = 0;
                    while i < x.len() && on > 0.0 {
                        help2[i] = true;
                        on -= 1.0;
                        i += 1;
                    }
                }
                let mut on = 0.0;
                for i in 0..x.len() {
                    let ii = x.len() - i - 1;
                    if help2[ii] {
                        on = formlineaddition
                    }
                    if on > 0.0 {
                        help2[ii] = true;
                        on -= 1.0;
                    }
                }
                if x.first() == x.last() && y.first() == y.last() && on > 0.0 {
                    let mut i = (x.len() - 1) as i32;
                    while i > -1 && on > 0.0 {
                        help2[i as usize] = true;
                        on -= 1.0;
                        i -= 1;
                    }
                }
                // Let's not break small form line rings
                smallringtest = false;
                if x.first() == x.last() && y.first() == y.last() && x.len() < 122 {
                    for i in 1..x.len() {
                        if help2[i] {
                            smallringtest = true
                        }
                    }
                }
                if smallringtest {
                    for i in 1..x.len() {
                        help2[i] = true;
                    }
                }
                smallringtest = false;
                if x.first() == x.last() && y.first() == y.last() && x.len() < 60 {
                    for i in 1..x.len() {
                        if help2[i] {
                            smallringtest = true
                        }
                    }
                }

                // Let's draw short gaps together
                if !smallringtest {
                    let mut tester = 1;
                    for i in 1..x.len() {
                        if help2[i] {
                            if tester < i && ((i - tester) as u32) < minimumgap {
                                for j in tester..(i + 1) {
                                    help2[j] = true;
                                }
                            }
                            tester = i;
                        }
                    }
                    // Ring handling
                    if x.first() == x.last() && y.first() == y.last() && x.len() < 2 {
                        let mut i = 1;
                        while i < x.len() && !help2[i] {
                            i += 1
                        }
                        let mut j = x.len() - 1;
                        while j > 1 && !help2[i] {
                            j -= 1
                        }
                        if ((x.len() - j + i - 1) as u32) < minimumgap && j > i {
                            for k in 0..(i + 1) {
                                help2[k] = true
                            }
                            for k in j..x.len() {
                                help2[k] = true
                            }
                        }
                    }
                }
                if remove_touching_contours {
                    // remove formlines when it is really steep
                    let mut touched = false;
                    for i in 0..x.len() {
                        if !help3[i] {
                            for k in 0..5 {
                                if i + k < x.len() {
                                    help2[i + k] = false;
                                    touched = true;
                                }
                                if i - k + 1 > 0 && i - k < x.len() {
                                    help2[i - k] = false;
                                    touched = true;
                                }
                            }
                        }
                    }
                    if touched {
                        let mut k = 0;
                        for i in 0..x.len() {
                            if help2[i] {
                                k += 1;
                            }
                            if k > 0 && !help2[i] && k < 15 {
                                for l in (i - k)..i {
                                    help2[l] = false;
                                }
                            }
                            if !help2[i] {
                                k = 0;
                            }
                        }
                    }
                }
            }

            let mut linedist = 0.0;
            let mut onegapdone = false;
            let mut gap = 0.0;
            let mut formlinestart = false;

            let f_label = if layer.contains("depression") && label_depressions {
                "formline_depression"
            } else {
                "formline"
            };

            for i in 1..x.len() {
                if curvew != 1.0 || formline == 0.0 || help2[i] || smallringtest {
                    if let (Some(fp), true) = (fp.as_mut(), curvew == 1.0) {
                        if !formlinestart {
                            write!(fp, "POLYLINE\r\n 66\r\n1\r\n  8\r\n{}\r\n  0\r\n", f_label)
                                .expect("Could not write file");
                            formlinestart = true;
                        }
                        write!(
                            fp,
                            "VERTEX\r\n  8\r\n{}\r\n 10\r\n{}\r\n 20\r\n{}\r\n  0\r\n",
                            f_label,
                            x[i] / 600.0 * 254.0 * scalefactor + x0,
                            -y[i] / 600.0 * 254.0 * scalefactor + y0
                        )
                        .expect("Could not write file");
                    }

                    if draw_image {
                        if curvew == 1.0 && formline == 2.0 {
                            let step =
                                ((x[i - 1] - x[i]).powi(2) + (y[i - 1] - y[i]).powi(2)).sqrt();
                            if i < 4 {
                                linedist = 0.0
                            }
                            linedist += step;
                            if linedist > dashlength && i > 10 && i < x.len() - 11 {
                                let mut sum = 0.0;
                                for k in (i - 4)..(i + 6) {
                                    sum += ((x[k - 1] - x[k]).powi(2) + (y[k - 1] - y[k]).powi(2))
                                        .sqrt()
                                }
                                let mut toonearend = false;
                                for k in (i - 10)..(i + 10) {
                                    if !help2[k] {
                                        toonearend = true;
                                        break;
                                    }
                                }
                                if !toonearend
                                    && ((x[i - 5] - x[i + 5]).powi(2)
                                        + (y[i - 5] - y[i + 5]).powi(2))
                                    .sqrt()
                                        * 1.138
                                        > sum
                                {
                                    linedist = 0.0;
                                    gap = gaplength;
                                    onegapdone = true;
                                }
                            }
                            if !onegapdone && (i < x.len() - 9) && i > 6 {
                                gap = gaplength * 0.82;
                                onegapdone = true;
                                linedist = 0.0
                            }
                            if gap > 0.0 {
                                gap -= step;
                                if gap < 0.0 && onegapdone && step > 0.0 {
                                    let mut n = -curvew - 0.5;
                                    while n < curvew + 0.5 {
                                        let mut m = -curvew - 0.5;
                                        while m < curvew + 0.5 {
                                            draw_line_segment_mut(
                                                canvas,
                                                (
                                                    ((-x[i - 1] * gap + (step + gap) * x[i]) / step
                                                        + n)
                                                        as f32,
                                                    ((-y[i - 1] * gap + (step + gap) * y[i]) / step
                                                        + m)
                                                        as f32,
                                                ),
                                                ((x[i] + n) as f32, (y[i] + m) as f32),
                                                color,
                                            );
                                            m += 1.0;
                                        }
                                        n += 1.0;
                                    }
                                    gap = 0.0;
                                }
                            } else {
                                let mut n = -curvew - 0.5;
                                while n < curvew + 0.5 {
                                    let mut m = -curvew - 0.5;
                                    while m < curvew + 0.5 {
                                        draw_line_segment_mut(
                                            canvas,
                                            ((x[i - 1] + n) as f32, (y[i - 1] + m) as f32),
                                            ((x[i] + n) as f32, (y[i] + m) as f32),
                                            color,
                                        );
                                        m += 1.0;
                                    }
                                    n += 1.0;
                                }
                            }
                        } else {
                            let mut n = -curvew;
                            while n < curvew {
                                let mut m = -curvew;
                                while m < curvew {
                                    draw_line_segment_mut(
                                        canvas,
                                        ((x[i - 1] + n) as f32, (y[i - 1] + m) as f32),
                                        ((x[i] + n) as f32, (y[i] + m) as f32),
                                        color,
                                    );
                                    m += 1.0;
                                }
                                n += 1.0;
                            }
                        }
                    }
                } else if let (Some(fp), true) = (fp.as_mut(), formlinestart) {
                    fp.write_all(b"SEQEND\r\n  0\r\n")
                        .expect("Could not write file");
                    formlinestart = false;
                }
            }
            if let (Some(fp), true) = (fp.as_mut(), formlinestart) {
                fp.write_all(b"SEQEND\r\n  0\r\n")
                    .expect("Could not write file");
            }
        }
    }
    if let Some(fp) = fp.as_mut() {
        fp.write_all(b"ENDSEC\r\n  0\r\nEOF\r\n")
            .expect("Could not write file");
    }
    Ok(())
}
