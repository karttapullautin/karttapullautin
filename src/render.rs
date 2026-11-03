use crate::config::Config;
use crate::constants::{INTERMEDIATE_CONTOUR_WIDTH, PX_PER_M};
use crate::geometry::BinaryDxf;
use crate::geometry::Classification;
use crate::geometry::Geometry;
use crate::geometry::Point2;
use crate::geometry::Polylines;
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::palette::{
    BLACK_RGBA, CLIFF2_DEBUG_RGBA, CLIFF3_DEBUG_RGBA, CLIFF4_DEBUG_RGBA, CONTOUR_BROWN_RGBA,
    NORTH_LINE_RGBA,
};
use crate::util::Timing;
use crate::vec2d::Vec2D;
use image::DynamicImage;
use image::ImageBuffer;
use image::Rgba;
use image::RgbaImage;
use imageproc::drawing::{draw_filled_circle_mut, draw_line_segment_mut};
use log::info;
use std::error::Error;
use std::f64::consts::PI;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;

type Canvas = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// Geometry of the map canvas, shared by every layer of one tile.
struct Frame {
    x0: f64,
    y0: f64,
    w: u32,
    h: u32,
    new_width: u32,
    new_height: u32,
}

/// Renders the map with and without depressions. The layers that do not depend on the depression
/// setting are composed once, and the heightmap and steepness are read once.
pub fn render_both(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    angle_deg: f64,
    nwidth: usize,
) -> Result<(), Box<dyn Error>> {
    let mut timing = Timing::start_now("render_both");
    timing.start_section("base layers");
    let (base, frame) = base_canvas(fs, config, tmpfolder, angle_deg, nwidth)?;
    timing.start_section("curve inputs");
    let inputs = load_curve_inputs(fs, config, tmpfolder)?;

    // The depression render also writes formlines.dxf.bin, so it goes first.
    if !config.render_nodepressions {
        return render_one(
            fs,
            config,
            thread,
            tmpfolder,
            base,
            &frame,
            &inputs,
            false,
            &mut timing,
        );
    }

    render_one(
        fs,
        config,
        thread,
        tmpfolder,
        base.clone(),
        &frame,
        &inputs,
        false,
        &mut timing,
    )?;
    render_one(
        fs,
        config,
        thread,
        tmpfolder,
        base,
        &frame,
        &inputs,
        true,
        &mut timing,
    )
}

pub fn render(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    angle_deg: f64,
    nwidth: usize,
    nodepressions: bool,
) -> Result<(), Box<dyn Error>> {
    let mut timing = Timing::start_now("render");
    let (base, frame) = base_canvas(fs, config, tmpfolder, angle_deg, nwidth)?;
    let inputs = load_curve_inputs(fs, config, tmpfolder)?;
    render_one(
        fs,
        config,
        thread,
        tmpfolder,
        base,
        &frame,
        &inputs,
        nodepressions,
        &mut timing,
    )
}

fn decode_png(fs: &impl FileSystem, path: &Path) -> Result<DynamicImage, Box<dyn Error>> {
    let mut reader = image::ImageReader::new(fs.open(path)?);
    reader.set_format(image::ImageFormat::Png);
    reader.no_limits();
    Ok(reader.decode()?)
}

/// Nearest-neighbour resize of the top-left `src_w` x `src_h` part of `src`.
///
/// Picks the same pixels as `image::imageops::resize` with `FilterType::Nearest`, but writes the
/// output directly. The image crate goes through an f32 intermediate image, which for an upscale
/// to a full tile costs several hundred MB and seconds per layer.
fn resize_nearest(src: &RgbaImage, src_w: u32, src_h: u32, new_w: u32, new_h: u32) -> RgbaImage {
    let src_w = src_w.min(src.width());
    let src_h = src_h.min(src.height());
    let stride = src.width() as usize * 4;

    // Same arithmetic as the image crate: the source pixel is the one under the centre of the
    // output pixel, computed in f32.
    let ratio_x = src_w as f32 / new_w as f32;
    let ratio_y = src_h as f32 / new_h as f32;
    let src_cols: Vec<usize> = (0..new_w)
        .map(|x| ((x as f32 + 0.5) * ratio_x).floor() as usize)
        .map(|sx| sx.min(src_w as usize - 1) * 4)
        .collect();

    let raw = src.as_raw();
    let row_len = new_w as usize * 4;
    let mut out = vec![0u8; row_len * new_h as usize];
    for (y, dst_row) in out.chunks_exact_mut(row_len).enumerate() {
        let sy = (((y as f32 + 0.5) * ratio_y).floor() as usize).min(src_h as usize - 1);
        let src_row = &raw[sy * stride..];
        for (dst, &sx) in dst_row.chunks_exact_mut(4).zip(&src_cols) {
            dst.copy_from_slice(&src_row[sx..sx + 4]);
        }
    }
    RgbaImage::from_raw(new_w, new_h, out).expect("output buffer has the right size")
}

/// Vegetation, undergrowth, low ground and north lines. These come before the curves and do not
/// depend on the depression setting, so they are drawn once per tile.
fn base_canvas(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
    angle_deg: f64,
    nwidth: usize,
) -> Result<(Canvas, Frame), Box<dyn Error>> {
    info!("Rendering...");

    let scalefactor = config.scalefactor;

    let angle = -angle_deg / 180.0 * PI;

    // Draw vegetation ----------
    let tfw_in = tmpfolder.join("vegetation.pgw");
    let mut lines = fs.open(tfw_in).expect("PGW file does not exist").lines();
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

    let vegetation = decode_png(fs, &tmpfolder.join("vegetation.png"))?.into_rgba8();
    let w = vegetation.width();
    let h = vegetation.height();

    let eastoff = -((x0 - (-angle).tan() * y0)
        - ((x0 - (-angle).tan() * y0) / (250.0 / angle.cos())).floor() * (250.0 / angle.cos()))
        * PX_PER_M;

    let new_width = (w as f64 * PX_PER_M / scalefactor) as u32;
    let new_height = (h as f64 * PX_PER_M / scalefactor) as u32;
    let mut img = resize_nearest(&vegetation, w, h, new_width, new_height);
    drop(vegetation);

    let undergrowth = decode_png(fs, &tmpfolder.join("undergrowth.png"))?.into_rgba8();
    let imgug = resize_nearest(
        &undergrowth,
        undergrowth.width(),
        undergrowth.height(),
        new_width,
        new_height,
    );
    image::imageops::overlay(&mut img, &imgug, 0, 0);

    let low_file = tmpfolder.join("low.png");
    if fs.exists(&low_file) {
        let low = decode_png(fs, &low_file)?.into_rgba8();
        let low = resize_nearest(&low, low.width(), low.height(), new_width, new_height);
        image::imageops::overlay(&mut img, &low, 0, 0);
    }

    // north lines ----------------
    if angle != 999.0 {
        let mut i: f64 = eastoff - 250.0 * PX_PER_M / angle.cos() * 100.0 / scalefactor;
        while i < w as f64 * 5.0 * PX_PER_M / scalefactor {
            for m in 0..nwidth {
                draw_line_segment_mut(
                    &mut img,
                    (i as f32 + m as f32, 0.0),
                    (
                        (i as f32 + (angle.tan() * (h as f64) * PX_PER_M / scalefactor) as f32)
                            + m as f32,
                        (h as f32 * PX_PER_M as f32 / scalefactor as f32),
                    ),
                    NORTH_LINE_RGBA,
                );
            }
            i += 250.0 * PX_PER_M / angle.cos() / scalefactor;
        }
    }

    let frame = Frame {
        x0,
        y0,
        w,
        h,
        new_width,
        new_height,
    };
    Ok((img, frame))
}

/// Draws the curves and the layers above them, then writes one map.
#[allow(clippy::too_many_arguments)]
fn render_one(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    mut img: Canvas,
    frame: &Frame,
    inputs: &CurveInputs,
    nodepressions: bool,
    timing: &mut Timing,
) -> Result<(), Box<dyn Error>> {
    timing.start_section(if nodepressions {
        "curves without depressions"
    } else {
        "curves with depressions"
    });
    draw_curves_with(fs, config, &mut img, tmpfolder, nodepressions, true, inputs)?;

    timing.start_section("layers above curves");
    draw_overlays(fs, config, tmpfolder, &mut img, frame)?;

    timing.start_section("write png");
    let filename = if nodepressions {
        format!("pullautus{thread}")
    } else {
        format!("pullautus_depr{thread}")
    };

    img.write_to(
        &mut fs
            .create(format!("{filename}.png"))
            .expect("could not save output png"),
        image::ImageFormat::Png,
    )
    .expect("could not write image");

    let file_in = tmpfolder.join("vegetation.pgw");
    let mut pgw_file_out = fs
        .create(format!("{filename}.pgw"))
        .expect("Unable to create file");

    if let Ok(lines) = fs.open(file_in) {
        for (i, line) in lines.lines().enumerate() {
            let ip = line.unwrap_or(String::new());
            let x: f64 = ip.parse::<f64>().unwrap();
            if i == 0 || i == 3 {
                write!(
                    &mut pgw_file_out,
                    "{}\r\n",
                    x / PX_PER_M * config.scalefactor
                )
                .expect("Unable to write to file");
            } else {
                write!(&mut pgw_file_out, "{ip}\r\n").expect("Unable to write to file");
            }
        }
    }
    info!("Done");
    Ok(())
}

/// Dot knolls, buildings, blue-black layer, cliffs and high ground, drawn over the curves.
fn draw_overlays(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
    img: &mut Canvas,
    frame: &Frame,
) -> Result<(), Box<dyn Error>> {
    let scalefactor = config.scalefactor;
    let &Frame {
        x0,
        y0,
        w,
        h,
        new_width,
        new_height,
    } = frame;

    // dotknolls----------
    let input = tmpfolder.join("dotknolls.dxf.bin");
    let data = BinaryDxf::from_reader(&mut fs.open(input)?)?;
    let Geometry::Points(points) = data.take_geometry().swap_remove(0) else {
        return Err(anyhow::anyhow!("dotknolls.dxf.bin should contain points").into());
    };

    for (point, layer) in points.iter() {
        if *layer != Classification::Dotknoll {
            continue;
        }

        // convert point to image coordinates
        let x = (point.x - x0) * PX_PER_M / scalefactor;
        let y = (y0 - point.y) * PX_PER_M / scalefactor;

        let color = CONTOUR_BROWN_RGBA;
        draw_filled_circle_mut(img, (x as i32, y as i32), 7, color)
    }

    // blocks -------------
    let blocks_file = tmpfolder.join("blocks.png");
    if fs.exists(&blocks_file) {
        let blockpurple_thumb = {
            let mut full = decode_png(fs, &blocks_file)?.into_rgba8();
            for p in full.pixels_mut() {
                if p[0] == 255 && p[1] == 255 && p[2] == 255 {
                    p[3] = 0;
                }
            }
            resize_nearest(&full, w, h, new_width, new_height)
        };

        for i in 0..3 {
            for j in 0..3 {
                image::imageops::overlay(
                    img,
                    &blockpurple_thumb,
                    (i as i64 - 1) * 2,
                    (j as i64 - 1) * 2,
                );
            }
        }
        image::imageops::overlay(img, &blockpurple_thumb, 0, 0);
    }

    // blueblack -------------
    let blueblack_file = tmpfolder.join("blueblack.png");
    if fs.exists(&blueblack_file) {
        let imgbb_thumb = {
            let mut full = decode_png(fs, &blueblack_file)?.into_rgba8();
            for p in full.pixels_mut() {
                if p[0] == 255 && p[1] == 255 && p[2] == 255 {
                    p[3] = 0;
                }
            }
            resize_nearest(&full, w, h, new_width, new_height)
        };
        image::imageops::overlay(img, &imgbb_thumb, 0, 0);
    }

    draw_cliffs(fs, config, tmpfolder, "c2g.dxf.bin", img, x0, y0)
        .expect("draw cliffs c2g.dxf.bin");
    draw_cliffs(fs, config, tmpfolder, "c3g.dxf.bin", img, x0, y0)
        .expect("draw cliffs c3g.dxf.bin");

    // high -------------
    let high_file = tmpfolder.join("high.png");
    if fs.exists(&high_file) {
        let high = decode_png(fs, &high_file)?.into_rgba8();
        let high_thumb = resize_nearest(&high, high.width(), high.height(), new_width, new_height);
        image::imageops::overlay(img, &high_thumb, 0, 0);
    }
    Ok(())
}

fn draw_cliffs(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
    file: &str,
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    x0: f64,
    y0: f64,
) -> Result<(), Box<dyn Error>> {
    let scalefactor = config.scalefactor;

    let input = tmpfolder.join(file);
    let dxf = BinaryDxf::from_reader(&mut fs.open(input)?)?;

    let Geometry::Polylines2(lines) = dxf.take_geometry().swap_remove(0) else {
        return Err(anyhow::anyhow!("cliff data should contain polylines").into());
    };

    for (mut line, class) in lines.into_iter() {
        // based on the layer we select the cliffcolor
        let cliffcolor = if config.cliffdebug {
            match class {
                Classification::Cliff2 => CLIFF2_DEBUG_RGBA,
                Classification::Cliff3 => CLIFF3_DEBUG_RGBA,
                Classification::Cliff4 => CLIFF4_DEBUG_RGBA,
                _ => BLACK_RGBA,
            }
        } else {
            BLACK_RGBA
        };

        // scale and flip all points into pixel-space
        for p in line.iter_mut() {
            p.x = (p.x - x0) * PX_PER_M / scalefactor;
            p.y = (y0 - p.y) * PX_PER_M / scalefactor;
        }

        if line.first() != line.last() {
            // trick to borrow both first and last as mutable at the same time. If not possible (eg
            // len == 0, then we should skip this line anyways)
            let [first, .., last] = &mut line[..] else {
                continue;
            };

            let dx = first.x - last.x;
            let dy = first.y - last.y;
            let dist = (dx.powi(2) + dy.powi(2)).sqrt();
            if dist > 0.0 {
                first.x += dx / dist * 1.5;
                first.y += dy / dist * 1.5;
                last.x -= dx / dist * 1.5;
                last.y -= dy / dist * 1.5;
                draw_filled_circle_mut(img, (first.x as i32, first.y as i32), 3, cliffcolor);
                draw_filled_circle_mut(img, (last.x as i32, last.y as i32), 3, cliffcolor);
            }
        }
        for i in 1..line.len() {
            for n in 0..6 {
                for m in 0..6 {
                    draw_line_segment_mut(
                        img,
                        (
                            (line[i - 1].x + (n as f64) - 3.0).floor() as f32,
                            (line[i - 1].y + (m as f64) - 3.0).floor() as f32,
                        ),
                        (
                            (line[i].x + (n as f64) - 3.0).floor() as f32,
                            (line[i].y + (m as f64) - 3.0).floor() as f32,
                        ),
                        cliffcolor,
                    )
                }
            }
        }
    }
    Ok(())
}

/// Heightmap and formline steepness of a tile. Both renders of a tile use the same values, so
/// they are read once.
struct CurveInputs {
    hmap: HeightMap,
    steepness: Vec2D<f64>,
}

fn load_curve_inputs(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<CurveInputs, Box<dyn Error>> {
    let &Config {
        scalefactor,
        mut formlinesteepness,
        formline,
        ..
    } = config;
    formlinesteepness *= scalefactor;

    let heightmap_in = tmpfolder.join("xyz2.hmap");
    let mut reader = fs.open(heightmap_in)?;
    let hmap = HeightMap::from_bytes(&mut reader)?;

    let xyz = &hmap.grid;
    let mut steepness = Vec2D::new(xyz.width(), xyz.height(), 0f64);

    if formline > 0.0 {
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
                steepness[(i, j)] = val;
            }
        }
    }

    Ok(CurveInputs { hmap, steepness })
}

pub fn draw_curves(
    fs: &impl FileSystem,
    config: &Config,
    canvas: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    tmpfolder: &Path,
    nodepressions: bool,
    draw_image: bool,
) -> Result<(), Box<dyn Error>> {
    let inputs = load_curve_inputs(fs, config, tmpfolder)?;
    draw_curves_with(
        fs,
        config,
        canvas,
        tmpfolder,
        nodepressions,
        draw_image,
        &inputs,
    )
}

fn draw_curves_with(
    fs: &impl FileSystem,
    config: &Config,
    canvas: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    tmpfolder: &Path,
    nodepressions: bool,
    draw_image: bool,
    inputs: &CurveInputs,
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

    let hmap = &inputs.hmap;
    let xyz = &hmap.grid;
    let x0 = hmap.minx();
    let y0 = hmap.maxy();
    let steepness = &inputs.steepness;

    if formline > 0.0 {
        xstart = hmap.xoffset;
        ystart = hmap.yoffset;
        size = hmap.scale;
    }

    // read the binary file

    let input_dxf = BinaryDxf::from_reader(&mut fs.open(tmpfolder.join("out2.dxf.bin"))?)
        .expect("Unable to read out2.dxf.bin");
    let bounds = input_dxf.bounds().clone();
    let Geometry::Polylines3(input_lines) = input_dxf.take_geometry().swap_remove(0) else {
        return Err(anyhow::anyhow!("out2.dxf.bin does not contain polylines").into());
    };

    let should_generate_formlines = formline == 2.0 && !nodepressions;
    let mut formlines = Polylines::<Point2, Classification>::new();

    let mut last_curve_drawn = false;
    let mut should_draw_next_slope_line = true;
    let next_slopeline_starts = input_lines
        .iter()
        .map(|(line, (layer, _))| {
            if *layer == Classification::SlopeLine {
                line.first().map(|point| {
                    (
                        (point.x - x0) * PX_PER_M / scalefactor,
                        (y0 - point.y) * PX_PER_M / scalefactor,
                    )
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    for (ii, (mut line, (layer, _height))) in input_lines.into_iter().enumerate() {
        if layer == Classification::SlopeLine && (!last_curve_drawn || !should_draw_next_slope_line)
        {
            should_draw_next_slope_line = true;
            continue;
        }
        last_curve_drawn = false;

        // flip and scale the line points
        for p in line.iter_mut() {
            p.x = (p.x - x0) * PX_PER_M / scalefactor;
            p.y = (y0 - p.y) * PX_PER_M / scalefactor;
        }

        // TEMP: split x and y values
        let x = line.iter().map(|p| p.x).collect::<Vec<_>>();
        let y = line.iter().map(|p| p.y).collect::<Vec<_>>();

        // The slope line is part of symbol 101, so it carries depression contour color
        // weight — it just belongs to a depression, hence the nodepressions gate below.
        let color = if layer.is_contour() && layer != Classification::SlopeLine {
            CONTOUR_BROWN_RGBA
        } else {
            Rgba([
                config.depressions_color.0,
                config.depressions_color.1,
                config.depressions_color.2,
                255,
            ]) // Default purple
        };

        if !nodepressions || layer.is_contour() {
            let mut curvew = 2.0;
            if layer.is_index() {
                curvew = 3.0;
            }
            if formline > 0.0 {
                if formline == 1.0 {
                    curvew = 2.5
                }
                if layer.is_intermed() {
                    curvew = INTERMEDIATE_CONTOUR_WIDTH
                }
                if layer.is_index() {
                    curvew = 3.5
                }
            }

            let mut smallringtest = false;
            let mut help = vec![false; x.len()];
            let mut help2 = vec![false; x.len()];
            let mut help3 = vec![false; x.len()];
            if curvew == INTERMEDIATE_CONTOUR_WIDTH {
                for i in 0..x.len() {
                    help[i] = false;
                    help2[i] = true;
                    help3[i] = false;
                    let xx =
                        (((x[i] / PX_PER_M * scalefactor + x0) - xstart) / size).floor() as usize;
                    let yy =
                        (((-y[i] / PX_PER_M * scalefactor + y0) - ystart) / size).floor() as usize;

                    // make sure indices are within bounds for the grid lookups
                    if xx >= xyz.width() - 1 || yy >= xyz.height() - 1 || xx < 1 || yy < 1 {
                        continue;
                    }

                    if curvew != INTERMEDIATE_CONTOUR_WIDTH
                        || formline == 0.0
                        || steepness[(xx, yy)] < formlinesteepness
                        || steepness[(xx, yy + 1)] < formlinesteepness
                        || steepness[(xx + 1, yy)] < formlinesteepness
                        || steepness[(xx + 1, yy + 1)] < formlinesteepness
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
                if x.first() == x.last()
                    && y.first() == y.last()
                    && x.len() < 122
                    && help2.iter().any(|v| *v)
                {
                    smallringtest = true;
                }
                if smallringtest {
                    help2.iter_mut().for_each(|h| *h = true);
                }

                smallringtest = false;
                if x.first() == x.last() && y.first() == y.last() && x.len() < 60 {
                    smallringtest = help2.iter().any(|v| *v);
                }

                // TODO: check if closed ring smaller than ISOM, draw dotknoll then if confidg allow it

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

            let f_label = if layer.is_depression() && label_depressions {
                Classification::FormlineDepression
            } else {
                Classification::Formline
            };

            let mut formiline_points = Vec::new();

            if layer == Classification::SmallDepression {
                curvew = 3.0;
            }

            // Check if next symbol is a slopeline and get its location
            let next_slopeline_start = if layer.is_depression() {
                next_slopeline_starts.get(ii + 1).copied().flatten()
            } else {
                None
            };

            for i in 1..x.len() {
                if !(curvew != INTERMEDIATE_CONTOUR_WIDTH
                    || formline == 0.0
                    || help2[i]
                    || smallringtest)
                    && should_draw_next_slope_line
                    && let Some((next_x, next_y)) = next_slopeline_start
                    && x[i] == next_x
                    && y[i] == next_y
                {
                    should_draw_next_slope_line = false;
                }
                if curvew != INTERMEDIATE_CONTOUR_WIDTH
                    || formline == 0.0
                    || help2[i]
                    || smallringtest
                {
                    if should_generate_formlines && curvew == INTERMEDIATE_CONTOUR_WIDTH {
                        formiline_points.push(Point2::new(
                            x[i] / PX_PER_M * scalefactor + x0,
                            -y[i] / PX_PER_M * scalefactor + y0,
                        ));
                    }

                    if draw_image {
                        if curvew == INTERMEDIATE_CONTOUR_WIDTH && formline == 2.0 {
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
                                        last_curve_drawn = true;
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
                                        last_curve_drawn = true;
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
                                    last_curve_drawn = true;
                                }
                                n += 1.0;
                            }
                        }
                    }
                } else if !formiline_points.is_empty() {
                    // line ended, append and start new one
                    formlines.push(formiline_points, f_label);
                    formiline_points = Vec::new();
                }
            }

            if !formiline_points.is_empty() {
                formlines.push(formiline_points, f_label);
            }
        }
    }

    if should_generate_formlines {
        let out_formlines = BinaryDxf::new(bounds, vec![formlines.into()]);
        out_formlines
            .to_writer(&mut fs.create(tmpfolder.join("formlines.dxf.bin"))?)
            .expect("Could not write formlines.dxf.bin");

        if config.output_dxf {
            out_formlines.to_dxf(&mut fs.create(tmpfolder.join("formlines.dxf"))?)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                (x * 7 + y * 13) as u8,
                (x * 3) as u8 ^ (y as u8),
                (x + y * 5) as u8,
                (x * y) as u8,
            ])
        })
    }

    #[test]
    fn resize_nearest_matches_image_crate() {
        let src = test_image(37, 23);
        for (nw, nh) in [(37, 23), (91, 57), (17, 11), (100, 1), (1, 50), (255, 199)] {
            let expected =
                image::imageops::resize(&src, nw, nh, image::imageops::FilterType::Nearest);
            assert_eq!(resize_nearest(&src, 37, 23, nw, nh), expected, "{nw}x{nh}");
        }
    }

    #[test]
    fn resize_nearest_uses_only_the_cropped_part() {
        let src = test_image(40, 30);
        let cropped = image::imageops::crop(&mut src.clone(), 0, 0, 20, 10).to_image();
        for (nw, nh) in [(60, 45), (20, 10), (7, 33)] {
            let expected =
                image::imageops::resize(&cropped, nw, nh, image::imageops::FilterType::Nearest);
            assert_eq!(resize_nearest(&src, 20, 10, nw, nh), expected, "{nw}x{nh}");
        }
    }
}
