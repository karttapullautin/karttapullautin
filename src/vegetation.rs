use image::{DynamicImage, GrayAlphaImage, GrayImage, Luma, LumaA};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut};
use imageproc::filter::median_filter;
use imageproc::rect::Rect;
use log::info;
use rustc_hash::FxHashMap;
use std::error::Error;
use std::f32::consts::SQRT_2;
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::{Config, Zone};
use crate::constants::{PX_PER_M, RATIO_EPSILON};
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::io::xyz::XyzInternalReader;
use crate::palette::{Palette, PaletteColorEnum, PalettedImage};
use crate::vec2d::Vec2D;

/// Writes `water.txt`: for the 3 m cells that contain points of the water class, one line `x y water total` (cell indices counted
/// from the top-left corner of `vegetation.pgw`, the number of water points and of all points in the cell). The vector tiles use it to
/// leave water out of the vegetation (pullautin shows water as open land).
pub fn watercells(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    const CELL: f64 = 3.0;

    let mut pgw = fs.open(tmpfolder.join("vegetation.pgw"))?.lines();
    let x0: f64 = pgw
        .nth(4)
        .ok_or("vegetation.pgw has no x origin")??
        .trim()
        .parse()?;
    let y0: f64 = pgw
        .next()
        .ok_or("vegetation.pgw has no y origin")??
        .trim()
        .parse()?;

    // (water points, all points) per cell
    let mut cells: FxHashMap<(i64, i64), (u32, u32)> = FxHashMap::default();
    let mut reader = XyzInternalReader::new(fs.open(tmpfolder.join("xyztemp.xyz.bin"))?)?;
    while let Some(chunk) = reader.next_chunk()? {
        for r in chunk {
            let cell = (((r.x - x0) / CELL) as i64, ((y0 - r.y) / CELL) as i64);
            let counts = cells.entry(cell).or_insert((0, 0));
            counts.1 += 1;
            if r.classification == config.water_class {
                counts.0 += 1;
            }
        }
    }

    let mut wet: Vec<_> = cells.into_iter().filter(|(_, (w, _))| *w > 0).collect();
    wet.sort_unstable_by_key(|(cell, _)| *cell);
    let mut out = std::io::BufWriter::new(fs.create(tmpfolder.join("water.txt"))?);
    for ((cx, cy), (water, total)) in wet {
        writeln!(out, "{cx} {cy} {water} {total}")?;
    }
    out.flush()?;
    Ok(())
}

pub fn makevege(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("Generating vegetation...");

    let heightmap_in = tmpfolder.join("xyz2.hmap");
    let mut reader = fs.open(heightmap_in)?;
    let hmap = HeightMap::from_bytes(&mut reader)?;

    // in world coordinates
    let size = hmap.scale;
    let xyz = &hmap.grid;

    let palette = Palette::new(config);

    let thresholds = &config.thresholds;
    let block = config.greendetectsize;

    let &Config {
        vege_bitmode,
        yellowheight,
        yellowthreshold,
        greenground,
        pointvolumefactor,
        pointvolumeexponent,
        greenhigh,
        topweight,
        vegezoffset: zoffset,
        uglimit,
        uglimit2,
        addition,
        firstandlastreturnasground,
        firstandlastfactor,
        lastfactor,
        yellowfirstlast,
        vegethin,
        ..
    } = config;
    let greenshades = &config.greenshades;

    let xyz_file_in = tmpfolder.join("xyztemp.xyz.bin");

    let xmin = hmap.minx();
    let ymin = hmap.miny();
    let xmax = hmap.maxx();
    let ymax = hmap.maxy();
    // xmax/ymax are always slightly superior to the max x/y values within the hmap
    // this mean (xmax - xmin).ceil() > (x - xmin).floor() is always true
    // for more detail why, check the xyz2heightmap function and the heightmap.rs file

    // here we overlay two other grids on top of the heightmap, but with the same origin
    let w_block = ((xmax - xmin) / block).ceil() as usize;
    let h_block = ((ymax - ymin) / block).ceil() as usize;

    let w_3 = ((xmax - xmin) / 3.0).ceil() as usize;
    let h_3 = ((ymax - ymin) / 3.0).ceil() as usize;

    let mut top = Vec2D::new(w_block, h_block, 0.0); // block
    let mut yhit = Vec2D::new(w_3, h_3, 0_u32); // 3.0
    let mut noyhit = Vec2D::new(w_3, h_3, 0_u32); // 3.0

    let mut i = 0;
    let mut reader = XyzInternalReader::new(fs.open(&xyz_file_in)?)?;
    while let Some(chunk) = reader.next_chunk()? {
        for r in chunk {
            if vegethin == 0 || ((i + 1) as u32).is_multiple_of(vegethin) {
                let x: f64 = r.x;
                let y: f64 = r.y;
                let h: f64 = r.z as f64;
                let r3 = r.classification;
                let r4 = r.number_of_returns;
                let r5 = r.return_number;

                let xx = ((x - xmin) / block) as usize;
                let yy = ((y - ymin) / block) as usize;
                let t = &mut top[(xx, yy)];
                if h > *t {
                    *t = h;
                }
                let xx = ((x - xmin) / 3.0) as usize;
                let yy = ((y - ymin) / 3.0) as usize;

                if r3 == 2
                    || h < yellowheight
                        + xyz[(((x - xmin) / size) as usize, ((y - ymin) / size) as usize)]
                {
                    yhit[(xx, yy)] += 1;
                } else if r4 == 1 && r5 == 1 {
                    noyhit[(xx, yy)] += yellowfirstlast;
                } else {
                    noyhit[(xx, yy)] += 1;
                }
            }

            i += 1;
        }
    }
    // rebind the variables to be non-mut for the rest of the function
    let (top, yhit, noyhit) = (top, yhit, noyhit);

    let mut firsthit = Vec2D::new(w_block, h_block, 0_u32); // block
    let mut ghit = Vec2D::new(w_block, h_block, 0_u32); // block
    let mut greenhit = Vec2D::new(w_block, h_block, 0_f32); // block
    let mut highit = Vec2D::new(w_block, h_block, 0_u32); // block

    let step: f32 = 6.0;

    let w_block_step = ((xmax - xmin) / (block * step as f64)).ceil() as usize;
    let h_block_step = ((ymax - ymin) / (block * step as f64)).ceil() as usize;

    #[derive(Default, Clone)]
    struct UggItem {
        ugg: f32,
        ug: u32,
    }
    let mut ug = Vec2D::new(w_block_step, h_block_step, UggItem::default()); // block / step

    let mut i = 0;
    let mut reader = XyzInternalReader::new(fs.open(&xyz_file_in)?)?;
    while let Some(chunk) = reader.next_chunk()? {
        for r in chunk {
            if vegethin == 0 || ((i + 1) as u32).is_multiple_of(vegethin) {
                let x: f64 = r.x;
                let y: f64 = r.y;
                let h: f64 = r.z as f64 - zoffset;
                let r3 = r.classification;
                let r4 = r.number_of_returns;
                let r5 = r.return_number;

                if r5 == 1 {
                    let xx = ((x - xmin) / block) as usize;
                    let yy = ((y - ymin) / block) as usize;
                    firsthit[(xx, yy)] += 1;
                }

                // linear interpolation of the height at the point based on the surrpoinding cells in the heightmap
                let thelele = {
                    let xx = ((x - xmin) / size) as usize;
                    let yy = ((y - ymin) / size) as usize;

                    let a = xyz[(xx, yy)];

                    // if we are on the edge, simply extend the values
                    let (b, c, d) = if xx < xyz.width() - 1 && yy < xyz.height() - 1 {
                        // inside, take all values
                        (xyz[(xx + 1, yy)], xyz[(xx, yy + 1)], xyz[(xx + 1, yy + 1)])
                    } else if xx < xyz.width() - 1 {
                        // on bottom edge, extend downwards
                        (xyz[(xx + 1, yy)], a, a)
                    } else if yy < xyz.height() - 1 {
                        // on right edge, extend to the right
                        (a, xyz[(xx, yy + 1)], a)
                    } else {
                        // in corner, use this height for all
                        (a, a, a)
                    };

                    let distx = (x - xmin) / size - xx as f64;
                    let disty = (y - ymin) / size - yy as f64;

                    // linear interpolation of the elevation at the point
                    let ab = a * (1.0 - distx) + b * distx;
                    let cd = c * (1.0 - distx) + d * distx;
                    ab * (1.0 - disty) + cd * disty
                };

                let xx = ((x - xmin) / block / (step as f64)) as usize;
                let yy = ((y - ymin) / block / (step as f64)) as usize;
                let hh = h - thelele;
                let ug_entry = &mut ug[(xx, yy)];
                if hh <= 1.2 {
                    if r3 == 2 {
                        ug_entry.ugg += 1.0;
                    } else if hh > 0.25 {
                        ug_entry.ug += 1;
                    } else {
                        ug_entry.ugg += 1.0;
                    }
                } else {
                    ug_entry.ugg += 0.05;
                }

                let xx = ((x - xmin) / block) as usize;
                let yy = ((y - ymin) / block) as usize;
                if r3 == 2 || greenground >= hh {
                    if r4 == 1 && r5 == 1 {
                        ghit[(xx, yy)] += firstandlastreturnasground;
                    } else {
                        ghit[(xx, yy)] += 1;
                    }
                } else {
                    let mut last = 1.0;
                    if r4 == r5 {
                        last = lastfactor;
                        if hh < 5.0 {
                            last = firstandlastfactor;
                        }
                    }

                    // NOTE: the use of top here means that we cannot combine the two processing loops into one
                    let top_val = top[(xx, yy)];
                    for &Zone {
                        low,
                        high,
                        roof,
                        factor,
                    } in config.zones.iter()
                    {
                        if hh >= low && hh < high && top_val - thelele < roof {
                            greenhit[(xx, yy)] += (factor * last) as f32;
                            break;
                        }
                    }

                    if greenhigh < hh {
                        highit[(xx, yy)] += 1;
                    }
                }
            }

            i += 1;
        }
    }
    // rebind the variables to be non-mut for the rest of the function
    let (firsthit, ug, ghit, greenhit, highit) = (firsthit, ug, ghit, greenhit, highit);

    let img_width = (w_block as f64 * block) as u32;
    let img_height = (h_block as f64 * block) as u32;

    // The images are georeferenced by their top-left corner (xmin, ymax), see the pgw files written below and
    // render.rs, which draws everything else relative to that corner. The grids above start at (xmin, ymin) and the
    // image height is rounded up to whole blocks, so rows must be counted down from ymax, not up from the bottom of
    // the image: the two differ by up to a block whenever the height is not a multiple of the block size.
    let extent_y = ymax - ymin;

    // render yellow as multiple small squares
    let mut imgye2 = PalettedImage::new(
        img_width,
        img_height,
        PaletteColorEnum::BackgroundWhite.to_color(),
    );
    for x in 0..(w_3 - 2) {
        for y in 0..(h_3 - 2) {
            let mut ghit2 = 0;
            let mut highhit2 = 0;

            // sum in a 2x2 area
            for i in x..x + 2 {
                for j in y..y + 2 {
                    ghit2 += yhit[(i, j)];
                    highhit2 += noyhit[(i, j)];
                }
            }
            if ghit2 as f64 / (highhit2 as f64 + ghit2 as f64 + RATIO_EPSILON) > yellowthreshold {
                // the 2x2 cells cover the 6 m window [3x, 3x + 6) x [3y, 3y + 6): draw the square around its centre
                imgye2.draw_filled_rect(
                    Rect::at(
                        x as i32 * 3 + 2,
                        (extent_y - y as f64 * 3.0).round() as i32 - 4,
                    )
                    .of_size(3, 3),
                    PaletteColorEnum::Yellow2.to_color(),
                );
            }
        }
    }

    // compute global average firsthit
    let aveg = {
        let mut aveg = 0;
        let mut avecount = 0;

        for x in 0..w_block {
            for y in 0..h_block {
                if ghit[(x, y)] > 1 {
                    aveg += firsthit[(x, y)];
                    avecount += 1;
                }
            }
        }
        aveg as f64 / avecount as f64
    };

    let mut imggr1 = PalettedImage::new(
        img_width,
        img_height,
        PaletteColorEnum::BackgroundWhite.to_color(),
    );
    for x in 0..w_block {
        for y in 0..h_block {
            let roof = top[(x, y)]
                - xyz[(
                    (x as f64 * block / size) as usize,
                    (y as f64 * block / size) as usize,
                )];

            // find lowest firsthit in a 5x5 area
            let mut firsthit2 = firsthit[(x, y)];
            for i in x.saturating_sub(2)..(x + 3).min(w_block) {
                for j in y.saturating_sub(2)..(y + 3).min(h_block) {
                    let value = firsthit[(i, j)];
                    if value < firsthit2 {
                        firsthit2 = value;
                    }
                }
            }

            let greenhit2 = greenhit[(x, y)] as f64;
            let highit2 = highit[(x, y)];
            let ghit2 = ghit[(x, y)];

            let mut greenlimit = 9999.0;
            for &(v0, v1, v2) in thresholds.iter() {
                if roof >= v0 && roof < v1 {
                    greenlimit = v2;
                    break;
                }
            }

            let thevalue = greenhit2 / (ghit2 as f64 + greenhit2 + 1.0)
                * (1.0 - topweight
                    + topweight * highit2 as f64
                        / (ghit2 as f64 + greenhit2 + highit2 as f64 + 1.0))
                * (1.0 - pointvolumefactor * firsthit2 as f64 / (aveg + 0.00001))
                    .powf(pointvolumeexponent);
            if thevalue > 0.0 {
                let mut greenshade = 0;
                for (i, &shade) in greenshades.iter().enumerate() {
                    if thevalue > greenlimit * shade {
                        greenshade = i + 1;
                    }
                }
                if greenshade > 0 {
                    // cell (x, y) holds the points of [x, x + 1) x [y, y + 1) blocks: draw it there
                    imggr1.draw_filled_rect(
                        Rect::at(
                            (x as f64 * block).floor() as i32 - addition,
                            (extent_y - (y + 1) as f64 * block).round() as i32 - addition,
                        )
                        .of_size(
                            (block as i32 + addition) as u32,
                            (block as i32 + addition) as u32,
                        ),
                        PaletteColorEnum::GreenShade(greenshade as u8 - 1).to_color(),
                    );
                }
            }
        }
    }

    // drop all used resources to free memory early
    drop((top, firsthit, ghit, greenhit, highit, yhit, noyhit));

    let proceed_yellows: bool = config.proceed_yellows;
    let med: u32 = config.med;
    let med2 = config.med2;
    let medyellow = config.medyellow;

    if med > 1 {
        imggr1 = imggr1.median_filter(med / 2, med / 2);
    }
    if med2 > 1 {
        imggr1 = imggr1.median_filter(med2 / 2, med2 / 2);
    }
    if proceed_yellows {
        if med > 1 {
            imgye2 = imgye2.median_filter(med / 2, med / 2);
        }
        if med2 > 1 {
            imgye2 = imgye2.median_filter(med2 / 2, med2 / 2);
        }
    } else if medyellow > 1 {
        imgye2 = imgye2.median_filter(medyellow / 2, medyellow / 2);
    }

    imgye2
        .write_to(
            &mut fs
                .create(tmpfolder.join("yellow.png"))
                .expect("error saving png"),
            &palette,
        )
        .expect("could not save output png");

    imggr1
        .write_to(
            &mut fs
                .create(tmpfolder.join("greens.png"))
                .expect("error saving png"),
            &palette,
        )
        .expect("could not save output png");

    if vege_bitmode {
        // create a new Luma image initialized with Zeros
        let mut g_img = GrayImage::new(imggr1.width(), imggr1.height());

        // modify pixels of the new image based on the green shades
        for (pixel, out) in imggr1.pixels().zip(g_img.pixels_mut()) {
            // if pixel is background, just skip (already initialized to 0)
            if *pixel == PaletteColorEnum::BackgroundWhite.to_color() {
                continue;
            }

            // else, find the corresponding green shade and set the output pixel
            for idx in 0..greenshades.len() {
                if *pixel == PaletteColorEnum::GreenShade(idx as u8).to_color() {
                    // index starts at 2 for the first green tone
                    *out = Luma([idx as u8 + 2]);
                }
            }
        }

        g_img
            .write_to(
                &mut fs
                    .create(tmpfolder.join("greens_bit.png"))
                    .expect("error saving png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

        let mut y_img = GrayAlphaImage::new(imgye2.width(), imgye2.height());

        for (pixel, out) in imgye2.pixels().zip(y_img.pixels_mut()) {
            if *pixel == PaletteColorEnum::Yellow2.to_color() {
                *out = LumaA([1, 255]);
            }
        }

        y_img
            .write_to(
                &mut fs
                    .create(tmpfolder.join("yellow_bit.png"))
                    .expect("error saving png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

        // overlay the two bit images (yellow on top of green) and save as vegetation bit image
        let mut img_bit = DynamicImage::ImageLuma8(g_img);
        let img_bit2 = DynamicImage::ImageLumaA8(y_img);
        image::imageops::overlay(&mut img_bit, &img_bit2, 0, 0);

        img_bit
            .write_to(
                &mut fs
                    .create(tmpfolder.join("vegetation_bit.png"))
                    .expect("error saving png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");
    }

    // overlay yellow on top of green and save as total vegetation image
    imggr1.overlay(&imgye2, 0, 0);

    imggr1
        .write_to(
            &mut fs
                .create(tmpfolder.join("vegetation.png"))
                .expect("error saving png"),
            &palette,
        )
        .expect("could not save output png");

    // drop images to free memory
    drop(imggr1);
    drop(imgye2);

    let mut imgwater = PalettedImage::new(
        img_width,
        img_height,
        PaletteColorEnum::BackgroundWhite.to_color(),
    );
    let buildings = config.buildings;
    let water = config.water;
    if buildings > 0 || water > 0 {
        let mut reader = XyzInternalReader::new(fs.open(&xyz_file_in)?)?;
        while let Some(chunk) = reader.next_chunk()? {
            for r in chunk {
                let (x, y) = (r.x, r.y);
                let c: u8 = r.classification;

                if c == buildings {
                    draw_filled_rect_mut(
                        &mut imgwater,
                        Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                        PaletteColorEnum::Black.to_color(),
                    );
                }
                if c == water {
                    draw_filled_rect_mut(
                        &mut imgwater,
                        Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                        PaletteColorEnum::Blue.to_color(),
                    );
                }
            }
        }
    }

    for (x, y, hh) in hmap.iter() {
        if hh < config.waterele {
            draw_filled_rect_mut(
                &mut imgwater,
                Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                PaletteColorEnum::Blue.to_color(),
            );
        }
    }

    imgwater
        .write_to(
            &mut fs
                .create(tmpfolder.join("blueblack.png"))
                .expect("error saving png"),
            &palette,
        )
        .expect("could not save output png");

    drop(imgwater); // explicitly drop imgwater to free memory

    let scalefactor = config.scalefactor;

    // factor to convert from coordinates to pixels
    let tmpfactor = (PX_PER_M / scalefactor) as f32;

    let bf32 = block as f32;
    let hf32 = h_block as f32;
    // rows of the undergrowth images are counted down from ymax as well
    let top_f32 = extent_y as f32;
    let ww = w_block as f32 * bf32;
    let hh = hf32 * bf32;
    let mut x = 0.0_f32;

    let mut imgug = PalettedImage::new(
        (w_block as f64 * block * PX_PER_M / scalefactor) as u32,
        (h_block as f64 * block * PX_PER_M / scalefactor) as u32,
        PaletteColorEnum::Transparent.to_color(),
    );
    let mut img_ug_bit = GrayImage::new(img_width, img_height);
    loop {
        if x >= ww {
            break;
        }
        let mut y = 0.0_f32;
        loop {
            if y >= hh {
                break;
            }
            let xx = (x / bf32 / step) as usize;
            let yy = (y / bf32 / step) as usize;

            let ug_entry = &ug[(xx, yy)];
            let value =
                ug_entry.ug as f64 / (ug_entry.ug as f64 + ug_entry.ugg as f64 + RATIO_EPSILON);
            if value > uglimit {
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x + bf32 * 3.0),
                        tmpfactor * (top_f32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x + bf32 * 3.0),
                        tmpfactor * (top_f32 - y + bf32 * 3.0),
                    ),
                    PaletteColorEnum::Undergrowth.to_color(),
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x + bf32 * 3.0) + 1.0,
                        tmpfactor * (top_f32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x + bf32 * 3.0) + 1.0,
                        tmpfactor * (top_f32 - y + bf32 * 3.0),
                    ),
                    PaletteColorEnum::Undergrowth.to_color(),
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x - bf32 * 3.0),
                        tmpfactor * (top_f32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x - bf32 * 3.0),
                        tmpfactor * (top_f32 - y + bf32 * 3.0),
                    ),
                    PaletteColorEnum::Undergrowth.to_color(),
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x - bf32 * 3.0) + 1.0,
                        tmpfactor * (top_f32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x - bf32 * 3.0) + 1.0,
                        tmpfactor * (top_f32 - y + bf32 * 3.0),
                    ),
                    PaletteColorEnum::Undergrowth.to_color(),
                );

                if vege_bitmode {
                    draw_undergrowth_bit(&mut img_ug_bit, x, top_f32 - y, bf32, tmpfactor, 0x01)
                }
            }
            if value > uglimit2 {
                draw_line_segment_mut(
                    &mut imgug,
                    (tmpfactor * x, tmpfactor * (top_f32 - y - bf32 * 3.0)),
                    (tmpfactor * x, tmpfactor * (top_f32 - y + bf32 * 3.0)),
                    PaletteColorEnum::Undergrowth.to_color(),
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (tmpfactor * x + 1.0, tmpfactor * (top_f32 - y - bf32 * 3.0)),
                    (tmpfactor * x + 1.0, tmpfactor * (top_f32 - y + bf32 * 3.0)),
                    PaletteColorEnum::Undergrowth.to_color(),
                );

                if vege_bitmode {
                    draw_undergrowth_bit(&mut img_ug_bit, x, top_f32 - y, bf32, tmpfactor, 0x02)
                }
            }

            y += bf32 * step;
        }
        x += bf32 * step;
    }
    imgug
        .write_to(
            &mut fs
                .create(tmpfolder.join("undergrowth.png"))
                .expect("error saving png"),
            &palette,
        )
        .expect("could not save output png");

    let img_ug_bit_b = median_filter(&img_ug_bit, (bf32 * step) as u32, (bf32 * step) as u32);

    img_ug_bit_b
        .write_to(
            &mut fs
                .create(tmpfolder.join("undergrowth_bit.png"))
                .expect("error saving png"),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    let mut writer = fs
        .create(tmpfolder.join("undergrowth.pgw"))
        .expect("cannot create pgw file");
    write!(
        &mut writer,
        "{}\r\n0.0\r\n0.0\r\n{}\r\n{}\r\n{}\r\n",
        1.0 / tmpfactor,
        -1.0 / tmpfactor,
        xmin,
        ymax,
    )
    .expect("Cannot write pgw file");

    let mut writer = fs
        .create(tmpfolder.join("vegetation.pgw"))
        .expect("cannot create pgw file");
    write!(
        &mut writer,
        "1.0\r\n0.0\r\n0.0\r\n-1.0\r\n{xmin}\r\n{ymax}\r\n"
    )
    .expect("Cannot write pgw file");

    info!("Done");
    Ok(())
}

fn draw_undergrowth_bit(
    image: &mut GrayImage,
    x: f32,
    y: f32,
    block: f32,
    tmpfactor: f32,
    category: u8,
) {
    // Convert the original integer render-pixel radius to metres to keep its ground footprint.
    let radius = (block * 9.0 * SQRT_2) as i32;
    draw_filled_circle_mut(
        image,
        (x as i32, y as i32),
        (radius as f32 / tmpfactor) as i32,
        Luma([category]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::fs::memory::MemoryFileSystem;
    use crate::io::xyz::{XyzInternalWriter, XyzRecord};
    use std::sync::OnceLock;

    struct UndergrowthOutput {
        category: u8,
        scalefactor: f64,
        image: GrayImage,
        dimensions: (u32, u32),
    }

    fn undergrowth_outputs() -> &'static Vec<UndergrowthOutput> {
        static OUTPUTS: OnceLock<Vec<UndergrowthOutput>> = OnceLock::new();
        OUTPUTS.get_or_init(|| {
            let mut config = Config::load_or_create_default().unwrap();
            config.vege_bitmode = true;
            config.greendetectsize = 3.0;
            config.vegezoffset = 0.0;
            config.vegethin = 0;
            config.uglimit = 0.35;
            config.uglimit2 = 0.56;
            let mut outputs = Vec::new();
            for (scalefactor, category) in [(1.0, 0x01), (1.0, 0x02), (4.0, 0x01), (4.0, 0x02)] {
                config.scalefactor = scalefactor;
                let fs = MemoryFileSystem::new();
                let folder = Path::new("vegetation-test");
                fs.create_dir_all(folder).unwrap();
                HeightMap {
                    xoffset: 0.0,
                    yoffset: 0.0,
                    scale: 3.0,
                    grid: Vec2D::new(60, 72, 0.0),
                }
                .to_file(&fs, folder.join("xyz2.hmap"))
                .unwrap();
                let mut writer =
                    XyzInternalWriter::new(fs.create(folder.join("xyztemp.xyz.bin")).unwrap());
                for (x, y) in [(19.0, 19.0), (163.0, 181.0)] {
                    let point = XyzRecord {
                        x,
                        y,
                        z: 0.6,
                        classification: 1,
                        number_of_returns: 1,
                        return_number: 1,
                        ..Default::default()
                    };
                    writer.write_records(&[point]).unwrap();
                    if category == 0x01 {
                        writer
                            .write_records(&[XyzRecord {
                                z: 0.0,
                                classification: 2,
                                ..point
                            }])
                            .unwrap();
                    }
                }
                drop(writer.finish().unwrap());
                makevege(&fs, &config, folder).unwrap();
                let sibling = fs
                    .read_image_png(folder.join("vegetation_bit.png"))
                    .unwrap();
                outputs.push(UndergrowthOutput {
                    category,
                    scalefactor,
                    image: fs
                        .read_image_png(folder.join("undergrowth_bit.png"))
                        .unwrap()
                        .to_luma8(),
                    dimensions: (sibling.width(), sibling.height()),
                });
            }
            outputs
        })
    }

    #[test]
    fn undergrowth_bit_marks_survive_metre_export() {
        assert_metre_export(0x01);
    }

    #[test]
    fn undergrowth_bit_dense_marks_survive_metre_export() {
        assert_metre_export(0x02);
    }

    fn assert_metre_export(category: u8) {
        for output in undergrowth_outputs()
            .iter()
            .filter(|output| output.category == category)
        {
            let (width, height) = output.dimensions;
            let mut exported = GrayImage::new(width, height);
            image::imageops::overlay(&mut exported, &output.image, 0, 0);
            for (x, y) in [(18, 198), (162, 36)] {
                assert_eq!(
                    exported.get_pixel(x, y).0[0],
                    category,
                    "A: category {category:#04x} lost at ({x}, {y})"
                );
            }
            assert!(exported.pixels().all(|p| [0, category].contains(&p.0[0])));
        }
    }

    #[test]
    fn undergrowth_bit_light_footprint_tracks_production_factor() {
        assert_production_footprint(0x01, 1.0, 24, 0);
    }

    #[test]
    fn undergrowth_bit_dense_footprint_tracks_production_factor() {
        assert_production_footprint(0x02, 1.0, 24, 0);
    }

    #[test]
    fn undergrowth_bit_light_footprint_tracks_coarse_production_factor() {
        assert_production_footprint(0x01, 4.0, 50, 0x01);
    }

    #[test]
    fn undergrowth_bit_dense_footprint_tracks_coarse_production_factor() {
        assert_production_footprint(0x02, 4.0, 50, 0x02);
    }

    fn assert_production_footprint(category: u8, scalefactor: f64, offset: i32, expected: u8) {
        let output = undergrowth_outputs()
            .iter()
            .find(|output| output.category == category && output.scalefactor == scalefactor)
            .unwrap();
        // Probe outside the default footprint and inside the coarse footprint after filtering.
        for (x, y, direction) in [(18, 198, 1), (162, 36, -1)] {
            assert_eq!(
                output.image.get_pixel((x + direction * offset) as u32, y).0[0],
                expected,
                "C: production footprint category {category:#04x}, scale {scalefactor}, ({x}, {y})"
            );
        }
    }

    #[test]
    fn undergrowth_bit_dimensions_match_siblings() {
        for output in undergrowth_outputs() {
            assert_eq!(
                output.image.dimensions(),
                output.dimensions,
                "B: bitmask grid mismatch"
            );
        }
    }

    #[test]
    fn undergrowth_bit_circles_keep_ground_footprint_at_opposite_edges() {
        assert_circle_footprint(1.0, 16);
    }

    #[test]
    fn undergrowth_bit_circles_keep_ground_footprint_with_coarse_render_pixels() {
        assert_circle_footprint(4.0, 64);
    }

    fn assert_circle_footprint(scalefactor: f64, radius: i32) {
        let factor = (PX_PER_M / scalefactor) as f32;
        for category in [0x01, 0x02] {
            for (x, y, direction) in [(18, 198, 1), (162, 36, -1)] {
                let mut image = GrayImage::new(180, 216);
                draw_undergrowth_bit(&mut image, x as f32, y as f32, 3.0, factor, category);
                assert_eq!(
                    image.get_pixel(x as u32, y as u32).0[0],
                    category,
                    "A: category {category:#04x}, scale {scalefactor}, ({x}, {y})"
                );
                assert_eq!(
                    image.get_pixel((x + direction * radius) as u32, y as u32).0[0],
                    category,
                    "radius too small: category {category:#04x}, scale {scalefactor}"
                );
                assert_eq!(
                    image
                        .get_pixel((x + direction * (radius + 1)) as u32, y as u32)
                        .0[0],
                    0,
                    "radius too large: category {category:#04x}, scale {scalefactor}"
                );
                assert!(image.pixels().all(|p| [0, category].contains(&p.0[0])));
            }
        }
    }

    // --- alignment of the rasters with the points they are computed from ---
    //
    // The scenes are blocks of points with known edges on a heightmap whose origin is not zero and whose height is
    // 0, 1 and 2 m short of a multiple of the 3 m block, so that an image anchored to the wrong edge shows up.

    const X0: f64 = 1000.0;
    const Y0: f64 = 5000.0;
    const WIDTH_M: usize = 102;

    struct Scene {
        yellow: GrayImage,
        green: GrayImage,
        undergrowth_bit: GrayImage,
        /// first and last row of undergrowth.png that carry stripes, in metres from the top of the image
        undergrowth_rows_m: (f64, f64),
        /// top-left corner of the images, as written to vegetation.pgw
        corner: (f64, f64),
    }

    fn point(x: f64, y: f64, z: f32, classification: u8, returns: (u8, u8)) -> XyzRecord {
        XyzRecord {
            x: X0 + x,
            y: Y0 + y,
            z,
            classification,
            number_of_returns: returns.0,
            return_number: returns.1,
            ..Default::default()
        }
    }

    /// Runs `makevege` on a flat heightmap of 2 m cells, `WIDTH_M` wide and `height_m` high, with one lattice of
    /// points per metre given by `at(x, y)` (the centre of each square metre).
    fn scene(height_m: usize, at: impl Fn(f64, f64) -> Vec<XyzRecord>) -> Scene {
        // every setting the scenes depend on is fixed here, whatever the pullauta.ini of the working directory says
        let mut config = Config::load_or_create_default().unwrap();
        config.vege_bitmode = true;
        config.greendetectsize = 3.0;
        config.vegezoffset = 0.0;
        config.vegethin = 0;
        config.scalefactor = 1.0;
        config.addition = 0;
        config.med = 0;
        config.med2 = 0;
        config.medyellow = 0;
        config.proceed_yellows = false;
        config.uglimit = 0.35;
        config.uglimit2 = 0.56;
        config.yellowheight = 0.9;
        config.yellowthreshold = 0.9;
        config.yellowfirstlast = 1;
        config.greenground = 0.9;
        config.greenhigh = 2.0;
        config.topweight = 0.8;
        config.pointvolumefactor = 0.1;
        config.pointvolumeexponent = 1.0;
        config.firstandlastreturnasground = 3;
        config.firstandlastfactor = 1.0;
        config.lastfactor = 1.0;
        config.zones = vec![Zone {
            low: 1.0,
            high: 2.65,
            roof: 99.0,
            factor: 1.0,
        }];
        config.thresholds = vec![(0.2, 99.0, 0.1)];
        config.greenshades = vec![0.2, 0.35, 0.5, 0.7, 1.3, 2.6, 4.0];
        config.buildings = 0;
        config.water = 0;
        config.waterele = -999.0;
        let fs = MemoryFileSystem::new();
        let folder = Path::new("vegetation-alignment-test");
        fs.create_dir_all(folder).unwrap();
        HeightMap {
            xoffset: X0,
            yoffset: Y0,
            scale: 2.0,
            grid: Vec2D::new(WIDTH_M / 2, height_m / 2, 0.0),
        }
        .to_file(&fs, folder.join("xyz2.hmap"))
        .unwrap();
        let mut writer = XyzInternalWriter::new(fs.create(folder.join("xyztemp.xyz.bin")).unwrap());
        for i in 0..WIDTH_M {
            for j in 0..height_m {
                writer
                    .write_records(&at(i as f64 + 0.5, j as f64 + 0.5))
                    .unwrap();
            }
        }
        drop(writer.finish().unwrap());
        makevege(&fs, &config, folder).unwrap();

        let pgw: Vec<f64> = fs
            .open(folder.join("vegetation.pgw"))
            .unwrap()
            .lines()
            .map(|line| line.unwrap().trim().parse().unwrap())
            .collect();
        assert_eq!(pgw[..4], [1.0, 0.0, 0.0, -1.0]);
        let mask = |name: &str| {
            let mut image = fs.read_image_png(folder.join(name)).unwrap().to_luma8();
            // everything that is not the white background is drawn
            image
                .pixels_mut()
                .for_each(|p| p.0[0] = u8::from(p.0[0] != 255));
            image
        };
        let stripes = fs
            .read_image_png(folder.join("undergrowth.png"))
            .unwrap()
            .to_rgba8();
        let striped: Vec<u32> = (0..stripes.height())
            .filter(|&row| (0..stripes.width()).any(|col| stripes.get_pixel(col, row).0[3] != 0))
            .collect();
        // undergrowth.png has its own resolution and world file
        let ug_pgw: Vec<f64> = fs
            .open(folder.join("undergrowth.pgw"))
            .unwrap()
            .lines()
            .map(|line| line.unwrap().trim().parse().unwrap())
            .collect();
        assert_eq!((ug_pgw[4], ug_pgw[5]), (pgw[4], pgw[5]));
        let to_m = |row: Option<&u32>| row.map_or(f64::NAN, |&row| row as f64 * -ug_pgw[3]);
        Scene {
            undergrowth_rows_m: (to_m(striped.first()), to_m(striped.last())),
            yellow: mask("yellow.png"),
            green: mask("greens.png"),
            undergrowth_bit: fs
                .read_image_png(folder.join("undergrowth_bit.png"))
                .unwrap()
                .to_luma8(),
            corner: (pgw[4], pgw[5]),
        }
    }

    /// World bounding box (min x, min y, max x, max y) of the drawn pixels, relative to the scene origin. The
    /// corner in the pgw file is the top-left corner of the top-left pixel, as render.rs and `watercells` use it.
    fn drawn_extent(image: &GrayImage, corner: (f64, f64)) -> (f64, f64, f64, f64) {
        let mut extent = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (col, row, p) in image.enumerate_pixels() {
            if p.0[0] != 0 {
                let (x, y) = (corner.0 - X0 + col as f64, corner.1 - Y0 - row as f64);
                extent = (
                    extent.0.min(x),
                    extent.1.min(y - 1.0),
                    extent.2.max(x + 1.0),
                    extent.3.max(y),
                );
            }
        }
        assert!(extent.0 < extent.2, "nothing was drawn");
        extent
    }

    fn centre(extent: (f64, f64, f64, f64)) -> (f64, f64) {
        ((extent.0 + extent.2) / 2.0, (extent.1 + extent.3) / 2.0)
    }

    fn inside(x: f64, y: f64, block: (f64, f64, f64, f64)) -> bool {
        x >= block.0 && x < block.2 && y >= block.1 && y < block.3
    }

    #[test]
    fn yellow_is_drawn_where_the_open_ground_is() {
        // open ground in a block of whole 3 m cells, forest everywhere else
        let open = (30.0, 30.0, 60.0, 60.0);
        for height_m in [102, 104, 106] {
            let s = scene(height_m, |x, y| {
                if inside(x, y, open) {
                    vec![point(x, y, 0.0, 2, (1, 1))]
                } else {
                    vec![point(x, y, 10.0, 1, (2, 1))]
                }
            });
            assert_eq!(s.corner, (X0, Y0 + height_m as f64));
            let drawn = drawn_extent(&s.yellow, s.corner);
            // a 6 m window is yellow when it is open: the windows fully inside the block, i.e. the block less 1.5 m
            // on every side, drawn as 3 m squares on a 1 m raster (so half a metre of rounding is left)
            let (cx, cy) = centre(drawn);
            assert!(
                (cx - 45.0).abs() <= 0.5 && (cy - 45.0).abs() <= 0.5,
                "height {height_m}: yellow {drawn:?} is not centred on the open block {open:?}"
            );
            assert_eq!(
                (drawn.2 - drawn.0, drawn.3 - drawn.1),
                (27.0, 27.0),
                "height {height_m}: size of the yellow area"
            );
        }
    }

    #[test]
    fn green_is_drawn_where_the_undergrowth_is() {
        // returns at 2 m above the ground in a block of whole 3 m cells, bare ground everywhere else
        let thicket = (30.0, 36.0, 60.0, 66.0);
        for height_m in [102, 104, 106] {
            let s = scene(height_m, |x, y| {
                let mut points = vec![point(x, y, 0.0, 2, (2, 2))];
                if inside(x, y, thicket) {
                    points.push(point(x, y, 2.0, 1, (2, 1)));
                    points.push(point(x, y, 2.0, 1, (2, 1)));
                }
                points
            });
            assert_eq!(
                drawn_extent(&s.green, s.corner),
                thicket,
                "height {height_m}: green against the thicket"
            );
        }
    }

    #[test]
    fn undergrowth_is_drawn_from_the_top_of_the_heightmap() {
        for height_m in [102, 104, 106] {
            // low vegetation in the 18 m undergrowth cell [18, 36) x [18, 36), marked around its corner (18, 18)
            let s = scene(height_m, |x, y| {
                if inside(x, y, (18.0, 18.0, 36.0, 36.0)) {
                    vec![point(x, y, 0.6, 1, (1, 1))]
                } else {
                    vec![point(x, y, 0.0, 2, (1, 1))]
                }
            });
            let rows: Vec<u32> = (0..s.undergrowth_bit.height())
                .filter(|&row| s.undergrowth_bit.get_pixel(18, row).0[0] != 0)
                .collect();
            assert!(!rows.is_empty(), "height {height_m}: no undergrowth mark");
            let middle = (rows[0] + rows[rows.len() - 1]) as f64 / 2.0;
            assert_eq!(
                s.corner.1 - Y0 - middle,
                18.0,
                "height {height_m}: undergrowth mark at rows {}..={}",
                rows[0],
                rows[rows.len() - 1]
            );
            // the stripes of undergrowth.png are drawn 9 m up and down from the same point
            let (first, last) = s.undergrowth_rows_m;
            let stripes = s.corner.1 - Y0 - (first + last) / 2.0;
            assert!(
                (stripes - 18.0).abs() <= 0.5,
                "height {height_m}: undergrowth stripes around {stripes} m, rows {first}..{last} m"
            );
        }
    }
}
