use std::{
    error::Error,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use log::{debug, info};

use crate::{
    config::Config,
    io::fs::FileSystem,
    shapefile::{
        canvas::Canvas,
        mapping::{Mapping, Operator},
    },
};
use shapefile::dbase::{FieldValue, Record};
use shapefile::{Polygon, Polyline, Shape, ShapeType};

pub fn render(
    fs: &impl FileSystem,
    config: &Config,
    tmpfolder: &Path,
    batch: bool,
) -> Result<(), Box<dyn Error>> {
    let low_file = tmpfolder.join("low.png");
    if fs.exists(&low_file) {
        fs.remove_file(low_file).unwrap();
    }

    let high_file = tmpfolder.join("high.png");
    if fs.exists(&high_file) {
        fs.remove_file(high_file).unwrap();
    }

    let scalefactor = config.scalefactor;

    let vectorconf = &config.vectorconf;
    let mtkskip = &config.mtkskiplayers;

    let mut vectorconf_mappings: Vec<Mapping> = vec![];
    if !vectorconf.is_empty() {
        let vectorconf_lines = fs
            .read_to_string(vectorconf)
            .expect("Can not read input file");

        // parse all the lines in the vectorconf file into a list of mappings
        vectorconf_mappings = vectorconf_lines
            .lines()
            .map(Mapping::from_str)
            .collect::<Result<Vec<_>, _>>()?;
    }

    let input = tmpfolder.join("vegetation.pgw");
    if !fs.exists(&input) {
        info!("Could not find vegetation file");
        return Ok(());
    }

    let data = fs.read_to_string(input).expect("Can not read input file");
    let d: Vec<&str> = data.split('\n').collect();

    let x0 = d[4].trim().parse::<f64>().unwrap();
    let y0 = d[5].trim().parse::<f64>().unwrap();
    // let resvege = d[0].trim().parse::<f64>().unwrap();

    let mut img_reader = image::ImageReader::new(
        fs.open(tmpfolder.join("vegetation.png"))
            .expect("Opening vegetation image failed"),
    );
    img_reader.set_format(image::ImageFormat::Png);
    img_reader.no_limits();
    let img = img_reader.decode().unwrap();
    let w = img.width() as f64;
    let h = img.height() as f64;

    let outw = w * 600.0 / 254.0 / scalefactor;
    let outh = h * 600.0 / 254.0 / scalefactor;

    // TODO: only allocate the canvas that are actually used... in a lazy way
    let (width, height) = (outw as u32, outh as u32);
    let mut imgbrown = Canvas::new(width, height);
    let mut imgbrowntop = Canvas::new(width, height);
    let mut imgblack = Canvas::new(width, height);
    let mut imgblacktop = Canvas::new(width, height);
    let mut imgyellow = Canvas::new(width, height);
    let mut imgolive = Canvas::new(width, height);
    let mut imgparkings = Canvas::new(width, height);
    let mut imgblue = Canvas::new(width, height);
    let mut imgmarsh = Canvas::new(width, height);
    let mut imgtempblack = Canvas::new(width, height);
    let mut imgtempblacktop = Canvas::new(width, height);
    let mut imgblue2 = Canvas::new(width, height);

    let white = (255, 255, 255);
    let black = (0, 0, 0);
    let brown = (255, 150, 80);

    let purple = config.buildingcolor;
    let yellow = (255, 184, 83);
    let blue = (29, 190, 255);
    let marsh = (0, 10, 220);
    let olive = (194, 176, 33);

    let shapetmpfolder = if batch {
        PathBuf::from("temp_shapefiles".to_string())
    } else {
        tmpfolder.to_path_buf()
    };

    let mut shp_files: Vec<PathBuf> = Vec::new();

    for path in fs.list(&shapetmpfolder).unwrap() {
        if let Some(extension) = path.extension() {
            if extension == "shp" {
                shp_files.push(path);
            }
        }
    }

    info!("Processing shapefiles: {shp_files:?}");
    let mut total_elapsed = Duration::ZERO;

    for shp_file in shp_files.iter() {
        let file = shp_file.as_path().file_name().unwrap().to_str().unwrap();
        let mut file = shapetmpfolder.join(file);

        // drawshape comes here
        let mut reader = fs.read_shapefile(file.clone())?;
        let bbox = reader.header().bbox;
        let minx = (600.0 / 254.0 / scalefactor * (bbox.min.x - x0)).floor();
        let maxy = (600.0 / 254.0 / scalefactor * (y0 - bbox.min.y)).floor();
        let maxx = (600.0 / 254.0 / scalefactor * (bbox.max.x - x0)).floor();
        let miny = (600.0 / 254.0 / scalefactor * (y0 - bbox.max.y)).floor();
        log::debug!("Bounding box: {bbox:?}");
        if minx > outw || maxx < 0.0 || miny > outh || maxy < 0.0 {
            info!("Skipping shapefile {}, out of bounds.", file.display());
            continue;
        }

        info!("Processing shapefile: {}", file.display());
        let start = std::time::Instant::now();
        for shape_record in reader.iter_shapes_and_records() {
            let (shape, record) = shape_record
                .unwrap_or_else(|_err: shapefile::Error| (Shape::NullShape, Record::default()));

            let bbox = match shape {
                Shape::Polygon(ref p) => p.bbox(),
                Shape::Polyline(ref p) => p.bbox(),
                _ => continue, // we don't care about other types
            };

            let minx = (600.0 / 254.0 / scalefactor * (bbox.min.x - x0)).floor();
            let maxy = (600.0 / 254.0 / scalefactor * (y0 - bbox.min.y)).floor();
            let maxx = (600.0 / 254.0 / scalefactor * (bbox.max.x - x0)).floor();
            let miny = (600.0 / 254.0 / scalefactor * (y0 - bbox.max.y)).floor();
            if minx > outw || maxx < 0.0 || miny > outh || maxy < 0.0 {
                continue;
            }

            let mut area = false;
            let mut roadedge = 0.0;
            let mut edgeimage = "black";
            let mut image = "";
            let mut thickness = 1.0;
            let mut color: Option<(u8, u8, u8)> = None;
            let mut dashedline = false;
            let mut border = 0.0;

            if vectorconf.is_empty() {
                // MML shape file
                let mut luokka = String::new();
                if let Some(fv) = record.get("LUOKKA") {
                    if let FieldValue::Numeric(Some(f_luokka)) = fv {
                        luokka = format!("{f_luokka}");
                    }
                    if let FieldValue::Character(Some(c_luokka)) = fv {
                        luokka = c_luokka.to_string();
                    }
                }
                let mut versuh = 0.0;
                if let Some(FieldValue::Numeric(Some(f_versuh))) = record.get("VERSUH") {
                    versuh = *f_versuh;
                }
                // water streams
                if ["36311", "36312"].contains(&luokka.as_str()) {
                    thickness = 4.0;
                    color = Some(marsh);
                    image = "blue";
                }

                // pathes
                if luokka == "12316" && versuh != -11.0 {
                    thickness = 12.0;
                    dashedline = true;
                    image = "black";
                    color = Some(black);
                    if versuh > 0.0 {
                        image = "blacktop";
                    }
                }

                // large pathes
                if (luokka == "12141" || luokka == "12314") && versuh != -11.0 {
                    thickness = 12.0;
                    image = "black";
                    color = Some(black);
                    if versuh > 0.0 {
                        image = "blacktop";
                    }
                }

                // roads
                if ["12111", "12112", "12121", "12122", "12131", "12132"].contains(&luokka.as_str())
                    && versuh != -11.0
                {
                    imgbrown.set_line_width(20.0);
                    imgbrowntop.set_line_width(20.0);
                    thickness = 20.0;
                    color = Some(brown);
                    image = "brown";
                    roadedge = 26.0;
                    imgblack.set_line_width(26.0);
                    if versuh > 0.0 {
                        edgeimage = "blacktop";
                        imgbrown.set_line_width(14.0);
                        imgbrowntop.set_line_width(14.0);
                        thickness = 14.0;
                    }
                }

                // railroads
                if ["14110", "14111", "14112", "14121", "14131"].contains(&luokka.as_str())
                    && versuh != -11.0
                {
                    image = "black";
                    color = Some(white);
                    thickness = 3.0;
                    roadedge = 18.0;
                    if versuh > 0.0 {
                        image = "blacktop";
                        edgeimage = "blacktop";
                    }
                }

                if luokka == "12312" && versuh != -11.0 {
                    dashedline = true;
                    thickness = 6.0;
                    image = "black";
                    color = Some(black);
                    if versuh > 0.0 {
                        image = "blacktop";
                    }
                }

                if luokka == "12313" && versuh != -11.0 {
                    dashedline = true;
                    thickness = 5.0;
                    image = "black";
                    color = Some(black);
                    if versuh > 0.0 {
                        image = "blacktop";
                    }
                }

                // power line
                if ["22300", "22311", "22312", "44500"].contains(&luokka.as_str()) {
                    imgblacktop.set_line_width(5.0);
                    thickness = 5.0;
                    color = Some(black);
                    image = "blacktop";
                }

                // fence
                if ["44211", "44213"].contains(&luokka.as_str()) {
                    imgblacktop.set_line_width(7.0);
                    thickness = 7.0;
                    color = Some(black);
                    image = "blacktop";
                }

                // Next are polygons

                // fields
                if luokka == "32611" {
                    area = true;
                    color = Some(yellow);
                    border = 3.0;
                    image = "yellow";
                }

                // lake
                if [
                    "36200", "36211", "36313", "38700", "44300", "45111", "54112",
                ]
                .contains(&luokka.as_str())
                {
                    area = true;
                    color = Some(blue);
                    border = 5.0;
                    image = "blue";
                }

                // impassable marsh
                if ["35421", "38300"].contains(&luokka.as_str()) {
                    area = true;
                    color = Some(marsh);
                    border = 3.0;
                    image = "marsh";
                }

                // regular marsh
                if ["35400", "35411"].contains(&luokka.as_str()) {
                    area = true;
                    color = Some(marsh);
                    border = 0.0;
                    image = "marsh";
                }

                // marshy
                if ["35300", "35412", "35422"].contains(&luokka.as_str()) {
                    area = true;
                    color = Some(marsh);
                    border = 0.0;
                    image = "marsh";
                }

                // marshy
                if [
                    "42210", "42211", "42212", "42220", "42221", "42222", "42230", "42231",
                    "42232", "42240", "42241", "42242", "42270", "42250", "42251", "42252",
                    "42260", "42261", "42262",
                ]
                .contains(&luokka.as_str())
                {
                    area = true;
                    color = Some(purple);
                    border = 0.0;
                    image = "black";
                }

                // settlement
                if [
                    "32000", "40200", "62100", "32410", "32411", "32412", "32413", "32414",
                    "32415", "32416", "32417", "32418",
                ]
                .contains(&luokka.as_str())
                {
                    area = true;
                    color = Some(olive);
                    border = 0.0;
                    image = "olive";
                }

                // airport runway, car parkings
                if ["32411", "32412", "32415", "32417", "32421"].contains(&luokka.as_str()) {
                    area = true;
                    color = Some(brown);
                    border = 0.0;
                    image = "parkings";
                }

                if mtkskip.contains(&luokka) {
                    color = None;
                }
            } else {
                // configuration based drawing, iterate over all the rules and find the one that matches
                for mapping in vectorconf_mappings.iter() {
                    // if the color is already set we have a match, skip the rest of the mappings
                    if color.is_some() {
                        break;
                    }

                    // check if the record matches the conditions
                    let mut is_ok = true;
                    for keyval in &mapping.conditions {
                        let mut r = String::from("");
                        if let Some(FieldValue::Character(Some(record_str))) =
                            record.get(&keyval.key)
                        {
                            r = record_str.trim().to_string();
                        }
                        if keyval.operator == Operator::Equal {
                            if r != keyval.value {
                                is_ok = false;
                            }
                        } else if r == keyval.value {
                            is_ok = false;
                        }
                    }

                    // no match? continue to the next mapping
                    if !is_ok {
                        continue;
                    }

                    let isom = &mapping.isom;

                    if isom == "306" {
                        imgblue.set_line_width(5.0);
                        thickness = 4.0;
                        color = Some(marsh);
                        image = "blue";
                    }

                    // small path
                    if isom == "505" {
                        dashedline = true;
                        thickness = 12.0;
                        color = Some(black);
                        image = "black";
                    }

                    // small path top
                    if isom == "505T" {
                        dashedline = true;
                        thickness = 12.0;
                        color = Some(black);
                        image = "blacktop";
                    }

                    // large path
                    if isom == "504" {
                        imgblack.set_line_width(12.0);
                        thickness = 12.0;
                        color = Some(black);
                        image = "black";
                    }

                    // large path top
                    if isom == "504T" {
                        imgblack.set_line_width(12.0);
                        thickness = 12.0;
                        color = Some(black);
                        image = "blacktop";
                    }

                    // road
                    if isom == "503" {
                        imgbrown.set_line_width(20.0);
                        imgbrowntop.set_line_width(20.0);
                        color = Some(brown);
                        image = "brown";
                        roadedge = 26.0;
                        thickness = 20.0;
                        imgblack.set_line_width(26.0);
                    }

                    // road, bridges
                    if isom == "503T" {
                        edgeimage = "blacktop";
                        imgbrown.set_line_width(14.0);
                        imgbrowntop.set_line_width(14.0);
                        color = Some(brown);
                        image = "brown";
                        roadedge = 26.0;
                        thickness = 14.0;
                        imgblack.set_line_width(26.0);
                    }

                    // railroads
                    if isom == "515" {
                        color = Some(white);
                        image = "black";
                        roadedge = 18.0;
                        thickness = 3.0;
                    }

                    // railroads top
                    if isom == "515T" {
                        color = Some(white);
                        image = "blacktop";
                        edgeimage = "blacktop";
                        roadedge = 18.0;
                        thickness = 3.0;
                    }

                    // small path
                    if isom == "507" {
                        dashedline = true;
                        color = Some(black);
                        image = "black";
                        thickness = 6.0;
                        imgblack.set_line_width(6.0);
                    }

                    // small path top
                    if isom == "507T" {
                        dashedline = true;
                        color = Some(black);
                        image = "blacktop";
                        thickness = 6.0;
                        imgblack.set_line_width(6.0);
                    }

                    // powerline
                    if isom == "516" {
                        color = Some(black);
                        image = "blacktop";
                        thickness = 5.0;
                        imgblacktop.set_line_width(5.0);
                    }

                    // fence
                    if isom == "524" {
                        color = Some(black);
                        image = "black";
                        thickness = 7.0;
                        imgblacktop.set_line_width(7.0);
                    }

                    // blackline
                    if isom == "414" {
                        color = Some(black);
                        image = "black";
                        thickness = 4.0;
                    }

                    // areas

                    // fields
                    if isom == "401" {
                        area = true;
                        color = Some(yellow);
                        border = 3.0;
                        image = "yellow";
                    }
                    // lakes
                    if isom == "301" {
                        area = true;
                        color = Some(blue);
                        border = 5.0;
                        image = "blue";
                    }
                    // marshes
                    if isom == "310" {
                        area = true;
                        color = Some(marsh);
                        image = "marsh";
                    }
                    // buildings
                    if isom == "526" {
                        area = true;
                        color = Some(purple);
                        image = "black";
                    }
                    // settlements
                    if isom == "527" {
                        area = true;
                        color = Some(olive);
                        image = "olive";
                    }
                    // car parkings border
                    if isom == "529.1" || isom == "301.1" {
                        thickness = 2.0;
                        color = Some(black);
                        image = "black";
                    }
                    // car park area
                    if isom == "529" {
                        area = true;
                        color = Some(brown);
                        image = "parkings";
                    }
                    // car park top
                    if isom == "529T" {
                        area = true;
                        color = Some(brown);
                        image = "brown";
                    }
                }
            }

            // if there was a match, do the drawing!
            if let Some(color) = color {
                let shapetype = shape.shapetype();
                if !area && shapetype == ShapeType::Polyline {
                    let polyline = Polyline::try_from(shape).unwrap();
                    let mut poly: Vec<(f32, f32)> = vec![];
                    for points in polyline.parts().iter() {
                        for point in points.iter() {
                            let x = point.x;
                            let y = point.y;
                            poly.push((
                                (600.0 / 254.0 / scalefactor * (x - x0)).floor() as f32,
                                (600.0 / 254.0 / scalefactor * (y0 - y)).floor() as f32,
                            ));
                        }
                    }
                    if roadedge > 0.0 {
                        if edgeimage == "blacktop" {
                            imgblacktop.unset_stroke_cap();
                            imgblacktop.set_line_width(roadedge);
                            imgblacktop.set_color(black);
                            imgblacktop.draw_polyline(&poly);
                            imgblacktop.set_line_width(thickness);
                        } else {
                            imgblack.set_color(black);
                            imgblack.set_stroke_cap_round();
                            imgblack.set_line_width(roadedge);
                            imgblack.draw_polyline(&poly);
                            imgblack.set_line_width(thickness);
                            imgblack.unset_stroke_cap();
                        }
                    }

                    if !dashedline {
                        if image == "blacktop" {
                            imgblacktop.set_line_width(thickness);
                            imgblacktop.set_color(color);
                            if thickness >= 9.0 {
                                imgblacktop.set_stroke_cap_round();
                            }
                            imgblacktop.draw_polyline(&poly);
                            imgblacktop.unset_stroke_cap();
                        }
                        if image == "black" {
                            imgblack.set_line_width(thickness);
                            imgblack.set_color(color);
                            if thickness >= 9.0 {
                                imgblack.set_stroke_cap_round();
                            } else {
                                imgblack.unset_stroke_cap();
                            }
                            imgblack.draw_polyline(&poly);
                        }
                    } else {
                        if image == "blacktop" {
                            let interval_on = 1.0 + thickness * 8.0;
                            imgtempblacktop.set_dash(interval_on, thickness * 1.6);
                            if thickness >= 9.0 {
                                imgtempblacktop.set_stroke_cap_round();
                            }
                            imgtempblacktop.set_color(color);
                            imgtempblacktop.set_line_width(thickness);
                            imgtempblacktop.draw_polyline(&poly);
                            imgtempblacktop.unset_dash();
                            imgtempblacktop.unset_stroke_cap();
                        }
                        if image == "black" {
                            let interval_on = 1.0 + thickness * 8.0;
                            imgtempblack.set_dash(interval_on, thickness * 1.6);
                            if thickness >= 9.0 {
                                imgtempblack.set_stroke_cap_round();
                            }
                            imgtempblack.set_color(color);
                            imgtempblack.set_line_width(thickness);
                            imgtempblack.draw_polyline(&poly);
                            imgtempblack.unset_dash();
                            imgtempblack.unset_stroke_cap();
                        }
                    }

                    if image == "blue" {
                        imgblue.set_color(color);
                        imgblue.set_line_width(thickness);
                        imgblue.draw_polyline(&poly)
                    }

                    if image == "brown" {
                        if edgeimage == "blacktop" {
                            imgbrowntop.set_line_width(thickness);
                            imgbrowntop.set_color(brown);
                            imgbrowntop.draw_polyline(&poly);
                        } else {
                            imgbrown.set_stroke_cap_round();
                            imgbrown.set_line_width(thickness);
                            imgbrown.set_color(brown);
                            imgbrown.draw_polyline(&poly);
                            imgbrown.unset_stroke_cap();
                        }
                    }
                } else if area && shapetype == ShapeType::Polygon {
                    let polygon = Polygon::try_from(shape).unwrap();
                    let mut polys: Vec<Vec<(f32, f32)>> = vec![];
                    for ring in polygon.rings().iter() {
                        let mut poly: Vec<(f32, f32)> = vec![];
                        let mut polyborder: Vec<(f32, f32)> = vec![];
                        for point in ring.points().iter() {
                            let x = point.x;
                            let y = point.y;
                            poly.push((
                                (600.0 / 254.0 / scalefactor * (x - x0)).floor() as f32,
                                (600.0 / 254.0 / scalefactor * (y0 - y)).floor() as f32,
                            ));
                            polyborder.push((
                                (600.0 / 254.0 / scalefactor * (x - x0)).floor() as f32,
                                (600.0 / 254.0 / scalefactor * (y0 - y)).floor() as f32,
                            ));
                        }
                        polys.push(poly);
                        if border > 0.0 {
                            imgblack.set_color(black);
                            imgblack.set_line_width(border);
                            imgblack.draw_closed_polyline(&polyborder);
                        }
                    }

                    if image == "black" {
                        imgblack.set_color(color);
                        imgblack.draw_filled_polygon(&polys)
                    }
                    if image == "blue" {
                        imgblue.set_color(color);
                        imgblue.draw_filled_polygon(&polys)
                    }
                    if image == "yellow" {
                        imgyellow.set_color(color);
                        imgyellow.draw_filled_polygon(&polys)
                    }
                    if image == "olive" {
                        imgolive.set_color(color);
                        imgolive.draw_filled_polygon(&polys)
                    }
                    if image == "parkings" {
                        imgparkings.set_color(color);
                        imgparkings.draw_filled_polygon(&polys)
                    }
                    if image == "marsh" {
                        imgmarsh.set_color(color);
                        imgmarsh.draw_filled_polygon(&polys)
                    }
                    if image == "brown" {
                        imgbrown.set_color(color);
                        imgbrown.draw_filled_polygon(&polys)
                    }
                }
            }
        }

        let elapsed = start.elapsed();
        debug!("Time elapsed in drawing shapes: {elapsed:.2?}");
        total_elapsed += elapsed;

        // remove the shapefile and all associated files
        if !batch {
            fs.remove_file(&file).unwrap();
            for ext in ["dbf", "sbx", "prj", "shx", "sbn", "cpg", "qmd"].iter() {
                file.set_extension(ext);
                if fs.exists(&file) {
                    println!("Removing file: {file:?}");
                    fs.remove_file(&file).unwrap();
                }
            }
        }
    }
    info!("Total time elapsed in drawing shapes: {total_elapsed:.2?}",);
    imgblue2.overlay(&mut imgblue, 0.0, 0.0);
    imgblue2.overlay(&mut imgblue, 1.0, 0.0);
    imgblue2.overlay(&mut imgblue, 0.0, 1.0);
    imgblue.overlay(&mut imgblue2, 0.0, 0.0);

    let mut i = 0.0_f32;
    imgmarsh.set_transparent_color();
    while i < ((h * 600.0 / 254.0 / scalefactor + 500.0) as f32) {
        i += 14.0;
        let wd = (w * 600.0 / 254.0 / scalefactor + 2.0) as f32;
        imgmarsh.draw_filled_polygon(&[vec![
            (-1.0, i),
            (wd, i),
            (wd, i + 10.0),
            (-1.0, i + 10.0),
            (-1.0, i),
        ]])
    }
    imgblacktop.overlay(&mut imgtempblacktop, 0.0, 0.0);
    imgblack.overlay(&mut imgtempblack, 0.0, 0.0);

    imgolive.overlay(&mut imgyellow, 0.0, 0.0);
    imgolive.overlay(&mut imgparkings, 0.0, 0.0);
    imgolive.overlay(&mut imgmarsh, 0.0, 0.0);

    imgblue.overlay(&mut imgblack, 0.0, 0.0);
    imgblue.overlay(&mut imgbrown, 0.0, 0.0);
    imgblue.overlay(&mut imgblacktop, 0.0, 0.0);
    imgblue.overlay(&mut imgbrowntop, 0.0, 0.0);

    let low_file = tmpfolder.join("low.png");
    if fs.exists(&low_file) {
        let mut low = Canvas::load_from(fs, &low_file).expect("could not load low.png");
        imgolive.overlay(&mut low, 0.0, 0.0);
    }

    let high_file = tmpfolder.join("high.png");
    if fs.exists(&high_file) {
        let mut high = Canvas::load_from(fs, &high_file).expect("could not load high.png");
        imgblue.overlay(&mut high, 0.0, 0.0);
    }
    imgblue
        .save_as(fs, &high_file)
        .expect("could not save high.png");
    imgolive
        .save_as(fs, &low_file)
        .expect("could not save low.png");
    Ok(())
}
