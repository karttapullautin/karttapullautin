use image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};
use imageproc::drawing::draw_filled_rect_mut;
use imageproc::filter::median_filter;
use imageproc::rect::Rect;
use log::info;
use rustc_hash::FxHashMap as HashMap;
use std::{error::Error, path::Path};

use crate::io::{bytes::FromToBytes, fs::FileSystem, heightmap::HeightMap, xyz::XyzInternalReader};

pub fn blocks(fs: &impl FileSystem, tmpfolder: &Path) -> Result<(), Box<dyn Error>> {
    info!("Identifying blocks...");

    let heightmap_in = tmpfolder.join("xyz2.hmap");
    let hmap = HeightMap::from_bytes(&mut fs.open(heightmap_in)?)?;

    let xstartxyz = hmap.xoffset;
    let ystartxyz = hmap.yoffset;
    let size = hmap.scale;

    let xmax = hmap.grid.width() - 1;
    let ymax = hmap.grid.height() - 1;

    // Temporarily convert to HashMap for not having to go through all the logic below.
    let mut xyz: HashMap<(u64, u64), f64> = HashMap::default();
    for (x, y, h) in hmap.grid.iter() {
        xyz.insert((x as u64, y as u64), h);
    }

    let mut img = RgbImage::from_pixel(xmax as u32 * 2, ymax as u32 * 2, Rgb([255, 255, 255]));
    let mut img2 = RgbaImage::from_pixel(xmax as u32 * 2, ymax as u32 * 2, Rgba([0, 0, 0, 0]));

    let black = Rgb([0, 0, 0]);
    let white = Rgba([255, 255, 255, 255]);

    let xyz_file_in = tmpfolder.join("xyztemp.xyz.bin");
    let mut reader = XyzInternalReader::new(fs.open(&xyz_file_in)?).unwrap();
    while let Some(chunk) = reader.next_chunk().unwrap() {
        for r in chunk {
            let (x, y, h) = (r.x, r.y, r.z as f64);
            let r3 = r.classification;
            let r4 = r.number_of_returns;
            let r5 = r.return_number;

            let xx = ((x - xstartxyz) / size).floor() as u64;
            let yy = ((y - ystartxyz) / size).floor() as u64;
            if r3 != 2
                && r3 != 9
                && r4 == 1
                && r5 == 1
                && h - *xyz.get(&(xx, yy)).unwrap_or(&0.0) > 2.0
            {
                draw_filled_rect_mut(
                    &mut img,
                    Rect::at(
                        (x - xstartxyz - 1.0) as i32,
                        (ystartxyz + 2.0 * ymax as f64 - y - 1.0) as i32,
                    )
                    .of_size(3, 3),
                    black,
                );
            } else {
                draw_filled_rect_mut(
                    &mut img2,
                    Rect::at(
                        (x - xstartxyz - 1.0) as i32,
                        (ystartxyz + 2.0 * ymax as f64 - y - 1.0) as i32,
                    )
                    .of_size(3, 3),
                    white,
                );
            }
        }
    }

    img2.write_to(
        &mut fs
            .create(tmpfolder.join("blocks2.png"))
            .expect("error saving png"),
        image::ImageFormat::Png,
    )
    .expect("error saving png");

    let mut img = DynamicImage::ImageRgb8(img);

    image::imageops::overlay(&mut img, &DynamicImage::ImageRgba8(img2), 0, 0);

    let filter_size = 2;
    img = image::DynamicImage::ImageRgb8(median_filter(&img.to_rgb8(), filter_size, filter_size));

    img.write_to(
        &mut fs
            .create(tmpfolder.join("blocks.png"))
            .expect("error saving png"),
        image::ImageFormat::Png,
    )
    .expect("error saving png");
    info!("Done");
    Ok(())
}
