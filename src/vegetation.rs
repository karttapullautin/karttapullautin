use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut};
use imageproc::filter::median_filter;
use imageproc::rect::Rect;
use log::info;
use rustc_hash::FxHashMap as HashMap;
use std::error::Error;
use std::f32::consts::SQRT_2;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use crate::config::{Config, Zone};
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::io::xyz::XyzInternalReader;

pub fn makevege(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("Generating vegetation...");

    let heightmap_in = tmpfolder.join("xyz2.hmap");
    let mut reader = BufReader::new(fs.open(heightmap_in)?);
    let hmap = HeightMap::from_bytes(&mut reader)?;

    // in world coordinates
    let xstart = hmap.xoffset;
    let ystart = hmap.yoffset;
    let size = hmap.scale;
    let xyz = &hmap.grid;

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
        greentone,
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

    let xmin = xstart;
    let ymin = ystart;
    let mut xmax: f64 = f64::MIN;
    let mut ymax: f64 = f64::MIN;

    let mut top: HashMap<(u64, u64), f64> = HashMap::default();
    let mut yhit: HashMap<(u64, u64), u64> = HashMap::default();
    let mut noyhit: HashMap<(u64, u64), u64> = HashMap::default();

    let mut i = 0;
    let mut reader = XyzInternalReader::new(BufReader::new(fs.open(&xyz_file_in)?))?;
    while let Some(r) = reader.next()? {
        if vegethin == 0 || ((i + 1) as u32) % vegethin == 0 {
            let x: f64 = r.x;
            let y: f64 = r.y;
            let h: f64 = r.z;
            let r3 = r.classification;
            let r4 = r.number_of_returns;
            let r5 = r.return_number;

            if xmax < x {
                xmax = x;
            }
            if ymax < y {
                ymax = y;
            }
            if x > xmin && y > ymin {
                let xx = ((x - xmin) / block).floor() as u64;
                let yy = ((y - ymin) / block).floor() as u64;
                if h > *top.get(&(xx, yy)).unwrap_or(&0.0) {
                    top.insert((xx, yy), h);
                }
                let xx = ((x - xmin) / 3.0).floor() as u64;
                let yy = ((y - ymin) / 3.0).floor() as u64;

                if r3 == 2
                    || h < yellowheight
                        + xyz[(
                            ((x - xmin) / size).floor() as usize,
                            ((y - ymin) / size).floor() as usize,
                        )]
                {
                    *yhit.entry((xx, yy)).or_insert(0) += 1;
                } else if r4 == 1 && r5 == 1 {
                    *noyhit.entry((xx, yy)).or_insert(0) += yellowfirstlast;
                } else {
                    *noyhit.entry((xx, yy)).or_insert(0) += 1;
                }
            }
        }

        i += 1;
    }
    // rebind the variables to be non-mut for the rest of the function
    let (yhit, noyhit) = (yhit, noyhit);

    let mut firsthit: HashMap<(u64, u64), u64> = HashMap::default();
    let mut ugg: HashMap<(u64, u64), f64> = HashMap::default();
    let mut ug: HashMap<(u64, u64), u64> = HashMap::default();
    let mut ghit: HashMap<(u64, u64), u64> = HashMap::default();
    let mut greenhit: HashMap<(u64, u64), f64> = HashMap::default();
    let mut highit: HashMap<(u64, u64), u64> = HashMap::default();
    let step: f32 = 6.0;

    let mut i = 0;
    let mut reader = XyzInternalReader::new(BufReader::new(fs.open(&xyz_file_in)?))?;
    while let Some(r) = reader.next()? {
        if vegethin == 0 || ((i + 1) as u32) % vegethin == 0 {
            let x: f64 = r.x;
            let y: f64 = r.y;
            let h: f64 = r.z - zoffset;
            let r3 = r.classification;
            let r4 = r.number_of_returns;
            let r5 = r.return_number;

            if x > xmin && y > ymin {
                if r5 == 1 {
                    let xx = ((x - xmin) / block + 0.5).floor() as u64;
                    let yy = ((y - ymin) / block + 0.5).floor() as u64;
                    *firsthit.entry((xx, yy)).or_insert(0) += 1;
                }

                let xx = ((x - xmin) / size).floor() as usize;
                let yy = ((y - ymin) / size).floor() as usize;
                let a = xyz[(xx, yy)];
                let b = xyz[(xx + 1, yy)];
                let c = xyz[(xx, yy + 1)];
                let d = xyz[(xx + 1, yy + 1)];

                let distx = (x - xmin) / size - xx as f64;
                let disty = (y - ymin) / size - yy as f64;

                let ab = a * (1.0 - distx) + b * distx;
                let cd = c * (1.0 - distx) + d * distx;
                let thelele = ab * (1.0 - disty) + cd * disty;
                let xx = ((x - xmin) / block / (step as f64) + 0.5).floor() as u64;
                let yy = (((y - ymin) / block / (step as f64)).floor() + 0.5).floor() as u64;
                let hh = h - thelele;
                if hh <= 1.2 {
                    if r3 == 2 {
                        *ugg.entry((xx, yy)).or_insert(0.0) += 1.0;
                    } else if hh > 0.25 {
                        *ug.entry((xx, yy)).or_insert(0) += 1;
                    } else {
                        *ugg.entry((xx, yy)).or_insert(0.0) += 1.0;
                    }
                } else {
                    *ugg.entry((xx, yy)).or_insert(0.0) += 0.05;
                }

                let xx = ((x - xmin) / block + 0.5).floor() as u64;
                let yy = ((y - ymin) / block + 0.5).floor() as u64;
                let yyy = ((y - ymin) / block).floor() as u64; // necessary due to bug in perl version
                if r3 == 2 || greenground >= hh {
                    if r4 == 1 && r5 == 1 {
                        *ghit.entry((xx, yyy)).or_insert(0) += firstandlastreturnasground;
                    } else {
                        *ghit.entry((xx, yyy)).or_insert(0) += 1;
                    }
                } else {
                    let mut last = 1.0;
                    if r4 == r5 {
                        last = lastfactor;
                        if hh < 5.0 {
                            last = firstandlastfactor;
                        }
                    }

                    let top_val = *top.get(&(xx, yy)).unwrap_or(&0.0);
                    for &Zone {
                        low,
                        high,
                        roof,
                        factor,
                    } in config.zones.iter()
                    {
                        if hh >= low && hh < high && top_val - thelele < roof {
                            *greenhit.entry((xx, yy)).or_insert(0.0) += factor * last;
                            break;
                        }
                    }

                    if greenhigh < hh {
                        *highit.entry((xx, yy)).or_insert(0) += 1;
                    }
                }
            }
        }

        i += 1;
    }
    // rebind the variables to be non-mut for the rest of the function
    let (firsthit, ugg, ug, ghit, greenhit, highit) = (firsthit, ugg, ug, ghit, greenhit, highit);

    let w = (xmax - xmin).floor() / block;
    let h = (ymax - ymin).floor() / block;
    let wy = (xmax - xmin).floor() / 3.0;
    let hy = (ymax - ymin).floor() / 3.0;

    let scalefactor = config.scalefactor;

    let img_width = (w * block) as u32;
    let img_height = (h * block) as u32;

    let greens = (0..greenshades.len())
        .map(|i| {
            Rgb([
                (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8,
                (254.0 - (74.0 / (greenshades.len() - 1) as f64) * i as f64) as u8,
                (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8,
            ])
        })
        .collect::<Vec<_>>();

    let mut aveg = 0;
    let mut avecount = 0;

    for x in 1..(h as usize) {
        for y in 1..(h as usize) {
            let xx = x as u64;
            let yy = y as u64;
            if *ghit.get(&(xx, yy)).unwrap_or(&0) > 1 {
                aveg += *firsthit.get(&(xx, yy)).unwrap_or(&0);
                avecount += 1;
            }
        }
    }
    let aveg = aveg as f64 / avecount as f64;
    let ye2 = Rgba([255, 219, 166, 255]);
    let mut imgye2 = RgbaImage::from_pixel(img_width, img_height, Rgba([255, 255, 255, 0]));
    for x in 4..(wy as usize - 3) {
        for y in 4..(hy as usize - 3) {
            let mut ghit2 = 0;
            let mut highhit2 = 0;

            for i in x..x + 2 {
                for j in y..y + 2 {
                    ghit2 += *yhit.get(&(i as u64, j as u64)).unwrap_or(&0);
                    highhit2 += *noyhit.get(&(i as u64, j as u64)).unwrap_or(&0);
                }
            }
            if ghit2 as f64 / (highhit2 as f64 + ghit2 as f64 + 0.01) > yellowthreshold {
                draw_filled_rect_mut(
                    &mut imgye2,
                    Rect::at(x as i32 * 3 + 2, (hy as i32 - y as i32) * 3 - 3).of_size(3, 3),
                    ye2,
                );
            }
        }
    }

    let mut imggr1 = RgbImage::from_pixel(img_width, img_height, Rgb([255, 255, 255]));
    for x in 2..w as usize {
        for y in 2..h as usize {
            let roof = *top.get(&(x as u64, y as u64)).unwrap_or(&0.0)
                - xyz[(
                    (x as f64 * block / size).floor() as usize,
                    (y as f64 * block / size).floor() as usize,
                )];

            let mut firsthit2 = *firsthit.get(&(x as u64, y as u64)).unwrap_or(&0);
            for i in (x - 2)..x + 3_usize {
                for j in (y - 2)..y + 3_usize {
                    let value = *firsthit.get(&(i as u64, j as u64)).unwrap_or(&0);
                    if value < firsthit2 {
                        firsthit2 = value;
                    }
                }
            }

            let greenhit2 = *greenhit.get(&(x as u64, y as u64)).unwrap_or(&0.0);
            let highit2 = *highit.get(&(x as u64, y as u64)).unwrap_or(&0);
            let ghit2 = *ghit.get(&(x as u64, y as u64)).unwrap_or(&0);

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
                    draw_filled_rect_mut(
                        &mut imggr1,
                        Rect::at(
                            ((x as f64 - 0.5) * block) as i32 - addition,
                            (((h - y as f64) - 0.5) * block) as i32 - addition,
                        )
                        .of_size(
                            (block as i32 + addition) as u32,
                            (block as i32 + addition) as u32,
                        ),
                        greens[greenshade - 1],
                    );
                }
            }
        }
    }

    let proceed_yellows: bool = config.proceed_yellows;
    let med: u32 = config.med;
    let med2 = config.med2;

    if med > 0 {
        imggr1 = median_filter(&imggr1, med / 2, med / 2);
        if proceed_yellows {
            imgye2 = median_filter(&imgye2, med / 2, med / 2);
        }
    }
    if med2 > 0 {
        imggr1 = median_filter(&imggr1, med2 / 2, med2 / 2);
        if proceed_yellows {
            imgye2 = median_filter(&imgye2, med / 2, med / 2);
        }
    }

    imgye2
        .write_to(
            &mut BufWriter::new(
                fs.create(tmpfolder.join("yellow.png"))
                    .expect("error saving png"),
            ),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    imggr1
        .write_to(
            &mut BufWriter::new(
                fs.create(tmpfolder.join("greens.png"))
                    .expect("error saving png"),
            ),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    let mut img = DynamicImage::ImageRgb8(imggr1);
    image::imageops::overlay(&mut img, &DynamicImage::ImageRgba8(imgye2), 0, 0);

    img.write_to(
        &mut BufWriter::new(
            fs.create(tmpfolder.join("vegetation.png"))
                .expect("error saving png"),
        ),
        image::ImageFormat::Png,
    )
    .expect("could not save output png");

    // drop img to free memory
    drop(img);

    if vege_bitmode {
        let g_img = fs
            .read_image_png(tmpfolder.join("greens.png"))
            .expect("Opening image failed");
        let mut g_img = g_img.to_rgb8();
        for pixel in g_img.pixels_mut() {
            let mut found = false;
            for (idx, color) in greens.iter().enumerate() {
                let c = idx as u8 + 2;
                if pixel[0] == color[0] && pixel[1] == color[1] && pixel[2] == color[2] {
                    *pixel = Rgb([c, c, c]);
                    found = true;
                }
            }
            if !found {
                *pixel = Rgb([0, 0, 0]);
            }
        }
        let g_img = DynamicImage::ImageRgb8(g_img).to_luma8();

        g_img
            .write_to(
                &mut BufWriter::new(
                    fs.create(tmpfolder.join("greens_bit.png"))
                        .expect("error saving png"),
                ),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

        let y_img = fs
            .read_image_png(tmpfolder.join("yellow.png"))
            .expect("Opening image failed");
        let mut y_img = y_img.to_rgba8();
        for pixel in y_img.pixels_mut() {
            if pixel[0] == ye2[0] && pixel[1] == ye2[1] && pixel[2] == ye2[2] && pixel[3] == ye2[3]
            {
                *pixel = Rgba([1, 1, 1, 255]);
            } else {
                *pixel = Rgba([0, 0, 0, 0]);
            }
        }
        let y_img = DynamicImage::ImageRgba8(y_img).to_luma_alpha8();

        y_img
            .write_to(
                &mut BufWriter::new(
                    fs.create(tmpfolder.join("yellow_bit.png"))
                        .expect("error saving png"),
                ),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

        let mut img_bit = DynamicImage::ImageLuma8(g_img);
        let img_bit2 = DynamicImage::ImageLumaA8(y_img);
        image::imageops::overlay(&mut img_bit, &img_bit2, 0, 0);

        img_bit
            .write_to(
                &mut BufWriter::new(
                    fs.create(tmpfolder.join("vegetation_bit.png"))
                        .expect("error saving png"),
                ),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");
    }

    let mut imgwater = RgbImage::from_pixel(img_width, img_height, Rgb([255, 255, 255]));
    let black = Rgb([0, 0, 0]);
    let blue = Rgb([29, 190, 255]);
    let buildings = config.buildings;
    let water = config.water;
    if buildings > 0 || water > 0 {
        let mut reader = XyzInternalReader::new(BufReader::new(fs.open(&xyz_file_in)?))?;
        while let Some(r) = reader.next()? {
            let (x, y) = (r.x, r.y);
            let c: u8 = r.classification;

            if c == buildings {
                draw_filled_rect_mut(
                    &mut imgwater,
                    Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                    black,
                );
            }
            if c == water {
                draw_filled_rect_mut(
                    &mut imgwater,
                    Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                    blue,
                );
            }
        }
    }

    for (x, y, hh) in hmap.iter() {
        if hh < config.waterele {
            draw_filled_rect_mut(
                &mut imgwater,
                Rect::at((x - xmin) as i32 - 1, (ymax - y) as i32 - 1).of_size(3, 3),
                blue,
            );
        }
    }

    imgwater
        .write_to(
            &mut BufWriter::new(
                fs.create(tmpfolder.join("blueblack.png"))
                    .expect("error saving png"),
            ),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    drop(imgwater); // explicitly drop imgwater to free memory

    let underg = Rgba([64, 121, 0, 255]);
    let tmpfactor = (600.0 / 254.0 / scalefactor) as f32;

    let bf32 = block as f32;
    let hf32 = h as f32;
    let ww = w as f32 * bf32;
    let hh = hf32 * bf32;
    let mut x = 0.0_f32;

    let mut imgug = RgbaImage::from_pixel(
        (w * block * 600.0 / 254.0 / scalefactor) as u32,
        (h * block * 600.0 / 254.0 / scalefactor) as u32,
        Rgba([255, 255, 255, 0]),
    );
    let mut img_ug_bit = GrayImage::from_pixel(
        (w * block * 600.0 / 254.0 / scalefactor) as u32,
        (h * block * 600.0 / 254.0 / scalefactor) as u32,
        Luma([0x00]),
    );
    loop {
        if x >= ww {
            break;
        }
        let mut y = 0.0_f32;
        loop {
            if y >= hh {
                break;
            }
            let xx = ((x / bf32 / step).floor()) as u64;
            let yy = ((y / bf32 / step).floor()) as u64;
            let value = *ug.get(&(xx, yy)).unwrap_or(&0) as f64
                / (*ug.get(&(xx, yy)).unwrap_or(&0) as f64
                    + { *ugg.get(&(xx, yy)).unwrap_or(&0.0) }
                    + 0.01);
            if value > uglimit {
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x + bf32 * 3.0),
                        tmpfactor * (hf32 * bf32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x + bf32 * 3.0),
                        tmpfactor * (hf32 * bf32 - y + bf32 * 3.0),
                    ),
                    underg,
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x + bf32 * 3.0) + 1.0,
                        tmpfactor * (hf32 * bf32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x + bf32 * 3.0) + 1.0,
                        tmpfactor * (hf32 * bf32 - y + bf32 * 3.0),
                    ),
                    underg,
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x - bf32 * 3.0),
                        tmpfactor * (hf32 * bf32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x - bf32 * 3.0),
                        tmpfactor * (hf32 * bf32 - y + bf32 * 3.0),
                    ),
                    underg,
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * (x - bf32 * 3.0) + 1.0,
                        tmpfactor * (hf32 * bf32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * (x - bf32 * 3.0) + 1.0,
                        tmpfactor * (hf32 * bf32 - y + bf32 * 3.0),
                    ),
                    underg,
                );

                if vege_bitmode {
                    draw_filled_circle_mut(
                        &mut img_ug_bit,
                        (
                            (tmpfactor * (x)) as i32,
                            (tmpfactor * (hf32 * bf32 - y)) as i32,
                        ),
                        (bf32 * 9.0 * SQRT_2) as i32,
                        Luma([0x01]),
                    )
                }
            }
            if value > uglimit2 {
                draw_line_segment_mut(
                    &mut imgug,
                    (tmpfactor * x, tmpfactor * (hf32 * bf32 - y - bf32 * 3.0)),
                    (tmpfactor * x, tmpfactor * (hf32 * bf32 - y + bf32 * 3.0)),
                    underg,
                );
                draw_line_segment_mut(
                    &mut imgug,
                    (
                        tmpfactor * x + 1.0,
                        tmpfactor * (hf32 * bf32 - y - bf32 * 3.0),
                    ),
                    (
                        tmpfactor * x + 1.0,
                        tmpfactor * (hf32 * bf32 - y + bf32 * 3.0),
                    ),
                    underg,
                );

                if vege_bitmode {
                    draw_filled_circle_mut(
                        &mut img_ug_bit,
                        (
                            (tmpfactor * (x)) as i32,
                            (tmpfactor * (hf32 * bf32 - y)) as i32,
                        ),
                        (bf32 * 9.0 * SQRT_2) as i32,
                        Luma([0x02]),
                    )
                }
            }

            y += bf32 * step;
        }
        x += bf32 * step;
    }
    imgug
        .write_to(
            &mut BufWriter::new(
                fs.create(tmpfolder.join("undergrowth.png"))
                    .expect("error saving png"),
            ),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    let img_ug_bit_b = median_filter(&img_ug_bit, (bf32 * step) as u32, (bf32 * step) as u32);

    img_ug_bit_b
        .write_to(
            &mut BufWriter::new(
                fs.create(tmpfolder.join("undergrowth_bit.png"))
                    .expect("error saving png"),
            ),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    let mut writer = BufWriter::new(
        fs.create(tmpfolder.join("undergrowth.pgw"))
            .expect("cannot create pgw file"),
    );
    write!(
        &mut writer,
        "{}\r\n0.0\r\n0.0\r\n{}\r\n{}\r\n{}\r\n",
        1.0 / tmpfactor,
        -1.0 / tmpfactor,
        xmin,
        ymax,
    )
    .expect("Cannot write pgw file");

    let mut writer = BufWriter::new(
        fs.create(tmpfolder.join("vegetation.pgw"))
            .expect("cannot create pgw file"),
    );
    write!(
        &mut writer,
        "1.0\r\n0.0\r\n0.0\r\n-1.0\r\n{}\r\n{}\r\n",
        xmin, ymax
    )
    .expect("Cannot write pgw file");

    info!("Done");
    Ok(())
}
