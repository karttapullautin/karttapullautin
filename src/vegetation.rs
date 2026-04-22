use image::buffer::ConvertBuffer;
use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut};
use imageproc::filter::median_filter;
use imageproc::rect::Rect;
use log::info;
use std::error::Error;
use std::f32::consts::SQRT_2;
use std::io::Write;
use std::path::Path;

use crate::config::{Config, Zone};
use crate::io::bytes::FromToBytes;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::io::xyz::XyzInternalReader;
use crate::vec2d::Vec2D;

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
            if vegethin == 0 || ((i + 1) as u32) % vegethin == 0 {
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
            if vegethin == 0 || ((i + 1) as u32) % vegethin == 0 {
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

    // render yellow as multiple small squares
    let ye2 = Rgba([255, 219, 166, 255]);
    // let mut imgye2 = RgbaImage::from_pixel(img_width, img_height, Rgba([255, 255, 255, 0]));
    let palette_ye = [(0, 0, 0), (ye2[0], ye2[1], ye2[2])];
    let mut imgye2 = GrayImage::from_pixel(img_width, img_height, Luma([0]));
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
            if ghit2 as f64 / (highhit2 as f64 + ghit2 as f64 + 0.01) > yellowthreshold {
                draw_filled_rect_mut(
                    &mut imgye2,
                    Rect::at(x as i32 * 3 + 2, (h_3 as i32 - y as i32) * 3 - 3).of_size(3, 3),
                    Luma::from([1]), // ye2,
                );
            }
        }
    }

    // render green gradients
    let greens = (0..greenshades.len())
        .map(|i| {
            Rgb([
                (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8,
                (254.0 - (74.0 / (greenshades.len() - 1) as f64) * i as f64) as u8,
                (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8,
            ])
        })
        .collect::<Vec<_>>();

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

    // let mut imggr1 = RgbImage::from_pixel(img_width, img_height, Rgb([255, 255, 255]));
    let mut imggr1 = GrayImage::from_pixel(img_width, img_height, Luma([0]));
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
                    draw_filled_rect_mut(
                        &mut imggr1,
                        Rect::at(
                            ((x as f64 - 0.5) * block) as i32 - addition,
                            (((h_block as f64 - y as f64) - 0.5) * block) as i32 - addition,
                        )
                        .of_size(
                            (block as i32 + addition) as u32,
                            (block as i32 + addition) as u32,
                        ),
                        Luma::from([greenshade as u8]),
                        // greens[greenshade - 1],
                    );
                }
            }
        }
    }

    let proceed_yellows: bool = config.proceed_yellows;
    let med: u32 = config.med;
    let med2 = config.med2;
    let medyellow = config.medyellow;

    if med > 0 {
        imggr1 = median_filter(&imggr1, med / 2, med / 2);
    }
    if med2 > 0 {
        imggr1 = median_filter(&imggr1, med2 / 2, med2 / 2);
    }
    if proceed_yellows {
        if med > 0 {
            imgye2 = median_filter(&imgye2, med / 2, med / 2);
        }
        if med2 > 0 {
            imgye2 = median_filter(&imgye2, med2 / 2, med2 / 2);
        }
    } else if medyellow > 0 {
        imgye2 = median_filter(&imgye2, medyellow / 2, medyellow / 2);
    }

    // convert to full image
    let imgye2 = imgye2.expand_palette(&palette_ye, Some(0));

    imgye2
        .write_to(
            &mut fs
                .create(tmpfolder.join("yellow.png"))
                .expect("error saving png"),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    // let writer = &mut fs
    //     .create(tmpfolder.join("greens.png"))
    //     .expect("error saving png");
    // let mut encoder = png::Encoder::new(writer, img_width, img_height);
    // encoder.set_color(png::ColorType::Indexed);
    //
    // let mut palette = vec![255, 255, 255];
    // palette.extend(greens.iter().flat_map(|c| c.0.iter().cloned()));
    // encoder.set_palette(palette);
    // encoder.set_depth(png::BitDepth::Eight);
    //
    // let mut w = encoder.write_header().expect("Failed to write PNG header");
    // w.write_image_data(imggr1.as_raw())
    //     .expect("Failed to write PNG data");

    let mut palette = vec![(255, 255, 255)];
    palette.extend(greens.iter().map(|c| (c[0], c[1], c[2])));

    let imggr1 = imggr1.expand_palette(&palette, None);
    let imggr1: RgbImage = imggr1.convert();

    imggr1
        .write_to(
            &mut fs
                .create(tmpfolder.join("greens.png"))
                .expect("error saving png"),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    let mut img = DynamicImage::ImageRgb8(imggr1);
    image::imageops::overlay(&mut img, &DynamicImage::ImageRgba8(imgye2), 0, 0);

    img.write_to(
        &mut fs
            .create(tmpfolder.join("vegetation.png"))
            .expect("error saving png"),
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
                &mut fs
                    .create(tmpfolder.join("greens_bit.png"))
                    .expect("error saving png"),
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
                &mut fs
                    .create(tmpfolder.join("yellow_bit.png"))
                    .expect("error saving png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

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

    let mut imgwater = RgbImage::from_pixel(img_width, img_height, Rgb([255, 255, 255]));
    let black = Rgb([0, 0, 0]);
    let blue = Rgb([29, 190, 255]);
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
            &mut fs
                .create(tmpfolder.join("blueblack.png"))
                .expect("error saving png"),
            image::ImageFormat::Png,
        )
        .expect("could not save output png");

    drop(imgwater); // explicitly drop imgwater to free memory

    let scalefactor = config.scalefactor;

    let underg = Rgba([64, 121, 0, 255]);
    let tmpfactor = (600.0 / 254.0 / scalefactor) as f32;

    let bf32 = block as f32;
    let hf32 = h_block as f32;
    let ww = w_block as f32 * bf32;
    let hh = hf32 * bf32;
    let mut x = 0.0_f32;

    let mut imgug = RgbaImage::from_pixel(
        (w_block as f64 * block * 600.0 / 254.0 / scalefactor) as u32,
        (h_block as f64 * block * 600.0 / 254.0 / scalefactor) as u32,
        Rgba([255, 255, 255, 0]),
    );
    let mut img_ug_bit = GrayImage::from_pixel(
        (w_block as f64 * block * 600.0 / 254.0 / scalefactor) as u32,
        (h_block as f64 * block * 600.0 / 254.0 / scalefactor) as u32,
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
            let xx = (x / bf32 / step) as usize;
            let yy = (y / bf32 / step) as usize;

            let ug_entry = &ug[(xx, yy)];
            let value = ug_entry.ug as f64 / (ug_entry.ug as f64 + ug_entry.ugg as f64 + 0.01);
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
            &mut fs
                .create(tmpfolder.join("undergrowth.png"))
                .expect("error saving png"),
            image::ImageFormat::Png,
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
