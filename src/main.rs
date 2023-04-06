use std::env;
use regex::Regex;
use std::path::Path;
extern crate ini;
use ini::Ini;
use std::{thread, time};
use std::error::Error;
use std::fs::File;
use std::fs;
use std::io::{self, BufRead};
use image::{RgbImage, RgbaImage, Rgb, Rgba};
use std::process::Command;
use std::io::{BufWriter, Write};
use std::fs::OpenOptions;
use std::collections::HashMap;
use rand::prelude::*;
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut};
use imageproc::rect::Rect;
use imageproc::filter::median_filter;


fn main() {
    let mut thread: String = String::new();
    if Path::new("pullauta.ini").exists() {
        // nothing
    } else {
        // TODO: create the ini file
    }

    let conf = Ini::load_from_file("pullauta.ini").unwrap();
    
    let int_re = Regex::new(r"^[1-9]\d*$").unwrap();

    let mut args: Vec<String> = env::args().collect();
    
    args.remove(0); // program name
    
    if args.len() > 0 && int_re.is_match(&args[0]) {
        thread = args.remove(0);
    }

    let mut command: String = String::new();
    if args.len() > 0{
        command = args.remove(0);
    }

    let accepted_files_re = Regex::new(r"\.(las|laz|xyz)$").unwrap();
    if command == "" || accepted_files_re.is_match(&command.to_lowercase())  {
        println!("This is rusty karttapulatin... There is no warranty. Use it at your own risk!\n");
    }
    
    // println!("Hello thread {}, command {}!", thread, command);

    let batch: bool = conf.general_section().get("batch").unwrap() == "1";

    let tmpfolder = format!("temp{}", thread);
    fs::create_dir_all(&tmpfolder).expect("Could not create tmp folder");

    if command == "" && Path::new(&format!("{}/vegetation.png", tmpfolder)).exists() && !batch {
        println!("Rendering png map with depressions");
        // TODO: run `pullauta render $pnorthlinesangle $pnorthlineswidth`
        println!("Rendering png map without depressions");
        // TODO: run `pullauta render $pnorthlinesangle $pnorthlineswidth  nodepressions`
        println!("\nAll done!");
        return();
    }

    if command == "" && !batch {
        println!("USAGE:\npullauta [parameter 1] [parameter 2] [parameter 3] ... [parameter n]\nSee readme.txt for more details");
    }

    if command == "groundfix" {
        println!("Not implemented");
        return();
    }

    if command == "profile" {
        println!("Not implemented");
        return();
    }
    
    if command == "ground" {
        println!("Not implemented");
        return();
    }

    if command == "ground2" {
        println!("Not implemented");
        return();
    }

    if command == "blocks" {
        println!("Not implemented");
        return();
    }

    if command == "dxfmerge" || command == "merge" {
        println!("Not implemented");
        return();
    }

    if command == "pngmerge" || command == "pngmergedepr" {
        println!("Not implemented");
        return();
    }

    if command == "pngmergevege" {
        println!("Not implemented");
        return();
    }

    if command == "makevegenew" {
        makevegenew(&thread).unwrap();
    }
    if command == "pngmergevege" {
        println!("Not implemented");
        return();
    }

    if command == "xyz2contours" {
        println!("{}", args[1]);
        let cinterval: f64 = args[0].parse::<f64>().unwrap();
        let xyzfilein = args[1].clone();
        let xyzfileout = args[2].clone();
        let dxffile = args[3].clone();
        let mut ground: bool = false;
        if args.len() > 4 && args[4] == "ground" {
            ground = true;
        }
        xyz2contours(&thread, cinterval, &xyzfilein, &xyzfileout, &dxffile, ground).unwrap();
        return();
    }

    if command == "makecliffs" {
        makecliffs(&thread).unwrap();
        return();
    }

    fn batch_process(thread: &String) {
        let _out = Command::new("perl")
                    .arg("pullauta")
                    .arg("startthread")
                    .arg(thread)
                    .output();
        // let _thread_number = thread.parse::<u64>().unwrap_or(0);
        // println!("Not implemented further");
        return();
    }

    let proc: u64 = conf.general_section().get("processes").unwrap().parse::<u64>().unwrap();
    if command == "" && batch && proc > 1 {
        let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity((proc + 1) as usize);
        for i in 0..proc {
            let handle = thread::spawn(move || {
                println!("Starting thread {}", i + 1);
                batch_process(&format!("{}", i + 1));     
            });
            thread::sleep(time::Duration::from_millis(100));
            handles.push(handle);
        }
        for handle in handles {
            handle.join().unwrap();
        }
        return();
    }

    if (command == "" && batch && proc < 2 ) || (command == "startthread" && batch) {
        thread = String::from("0");
        if args.len() > 0 {
            thread = args[0].clone();
        }
        if thread == "0" {
            thread = String::from("");
        }
        batch_process(&thread)
    }

    if accepted_files_re.is_match(&command) {
        println!("Preparing input file");

        let mut skiplaz2txt: bool = false;
        if Regex::new(r".xyz$").unwrap().is_match(&command.to_lowercase()) {
            println!(".xyz input file");
            if let Ok(lines) = read_lines(&command) {
                let mut i: u32 = 0;
                for line in lines {
                    if  i == 2 {
                        let ip = line.unwrap_or(String::new());
                        let parts = ip.split(" ");
                        let r = parts.collect::<Vec<&str>>();                     
                        if r.len() == 7 {
                            skiplaz2txt = true;
                            break;
                        }
                    }
                    i += 1;
                }
            }
        }

        if !skiplaz2txt {
            let out = Command::new("las2txt")
                    .arg("-version")
                    .output()
                    .expect("las2txt command failed to start");
            if out.status.success() {
                println!("Not implemented further");
                return();
            } else {
                println!("Can't find las2txt. It is needed if input file is not xyz file with xyzc data. Make sure it is in path or copy it to the same folder as pullautin");
                return();
            }
        } else {
            fs::copy(&command, format!("{}/xyztemp.xyz", tmpfolder)).expect("Could not copy file to tmpfolder");
        }
        println!("Done");
        println!("Knoll detection part 1");
        let scalefactor: f64 = conf.general_section().get("scalefactor").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
        xyz2contours(&thread, scalefactor * 0.3, "xyztemp.xyz", "xyz_03.xyz", "contours03.dxf", true).expect("countour generation failed");
        /*
        fs::copy(format!("{}/xyz_03.xyz", tmpfolder), format!("{}/xyz2.xyz", tmpfolder)).expect("Could not copy file");
        
        let basemapcontours: f64 = conf.general_section().get("basemapinterval").unwrap_or("0").parse::<f64>().unwrap_or(0.0);

        if basemapcontours != 0.0 {
            println!("Basemap contours");
            xyz2contours(&thread, basemapcontours, "xyz2.xyz", "", "basemap.dxf", false).expect("countour generation failed");
        }
        
        if conf.general_section().get("skipknolldetection").unwrap_or("0") == "1" {
            println!("Knoll detection part 2");
            // TODO: Run `pulauta knolldetector`
        }
    
        println!("Contour generation part 1");
        // TODO: Run `pulauta xyzknolls`
    
        if conf.general_section().get("skipknolldetection").unwrap_or("0") == "1" {
            // contours 2.5
            println!("Contour generation part 2");
            xyz2contours(&thread, 2.5 * scalefactor, "xyz_knolls.xyz", "", "out.dxf", false).expect("countour generation failed");
        } else {
            xyz2contours(&thread, basemapcontours, "xyztemp.xyz", "", "out.dxf", true).expect("countour generation failed");
        }
        */
        println!("Not implemented further");
    }
}

fn makecliffs(thread: &String ) -> Result<(), Box<dyn Error>>  {
    println!("Running makecliffs");
    let conf = Ini::load_from_file("pullauta.ini").unwrap();

    let c1_limit: f64 = conf.general_section().get("cliff1").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    let c2_limit: f64 = conf.general_section().get("cliff2").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    // let c3_limit: f64 = conf.general_section().get("cliff3").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    
    let cliff_thin: f64 = conf.general_section().get("cliffthin").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    
    let steep_factor: f64 = conf.general_section().get("cliffsteepfactor").unwrap_or("0.33").parse::<f64>().unwrap_or(0.33);
    
    let flat_place: f64 = conf.general_section().get("cliffflatplace").unwrap_or("6.6").parse::<f64>().unwrap_or(6.6);
    
    let mut no_small_ciffs: f64 = conf.general_section().get("cliffnosmallciffs").unwrap_or("0").parse::<f64>().unwrap_or(0.0);

    if no_small_ciffs == 0.0 {
        no_small_ciffs = 6.0;
    } else {
        no_small_ciffs -= flat_place;
    }

    let mut xmin: f64 = std::f64::MAX;
    let mut xmax: f64 = std::f64::MIN; 

    let mut ymin: f64 = std::f64::MAX;
    let mut ymax: f64 = std::f64::MIN;
    
    let mut hmin: f64 = std::f64::MAX; 
    let mut hmax: f64 = std::f64::MIN;
    
    let tmpfolder = format!("temp{}", thread);

    let xyz_file_in = format!("{}/xyztemp.xyz", tmpfolder);
    
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            let h: f64 = r[2].parse::<f64>().unwrap();
            if xmin > x {
                xmin = x;
            }
            
            if xmax < x {
                xmax = x;
            }
            
            if ymin > y {
                ymin = y;
            }
            
            if ymax < y {
                ymax = y;
            }
            
            if hmin > h {
                hmin = h;
            } 
            
            if hmax < h {
                hmax = h;
            }
        }
    }

    let xyz_file_in = format!("{}/xyz2.xyz", tmpfolder);
    let mut size: f64 = f64::NAN;
    let mut xstart: f64 = f64::NAN;
    let mut ystart: f64 = f64::NAN;
    let mut sxmax: usize = usize::MIN;
    let mut symax: usize = usize::MIN;
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for (i, line) in lines.enumerate() {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            if i == 0 {
                xstart = x;
                ystart = y;
            } else if i == 1 {
                size = y - ystart;
            } else {
                break;
            }
        }
    }
    let mut xyz = vec![vec![f64::NAN; ((ymax - ystart) / size).ceil() as usize + 1]; ((xmax - xstart) / size).ceil() as usize + 1];
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            let h: f64 = r[2].parse::<f64>().unwrap();

            let xx = ((x - xstart) / size).floor() as usize;
            let yy = ((y - ystart) / size).floor() as usize;

            xyz[xx][yy] = h;

            if sxmax < xx {
                sxmax = xx;
            }
            if symax < yy {
                symax = yy;
            }
        }
    }
    let mut steepness = vec![vec![f64::NAN; symax+1]; sxmax+1];
    for i in 3..sxmax-4 {
        for j in 3..symax-4 {
            let mut low: f64 = f64::MAX;
            let mut high: f64 = f64::MIN;
            for ii in i-3..i+4 {
                for jj in j-3..j+4 {
                    if xyz[ii][jj] < low { 
                        low  = xyz[ii][jj];
                    }
                    if xyz[ii][jj] > high {
                        high = xyz[ii][jj];
                    }
                }
            }
            steepness[i][j] = high - low;
        }
    }

    let mut img = RgbImage::from_pixel((xmax - xmin).floor() as u32, (ymax - ymin).floor() as u32, Rgb([255, 255, 255]));
    

    xmin = (xmin / 3.0 ).floor() * 3.0;
    ymin = (ymin / 3.0 ).floor() * 3.0;
    
    let mut list_alt = vec![vec![Vec::<String>::new(); (((ymax - ymin) / 3.0).ceil() + 1.0) as usize]; (((xmax - xmin) / 3.0).ceil() + 1.0) as usize];
    
    let xyz_file_in = format!("{}/xyztemp.xyz", tmpfolder);

    let mut rng = rand::thread_rng();
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            if cliff_thin > rng.gen() {
                let ip = line.unwrap_or(String::new());
                let parts = ip.split(" ");
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap();
                if r[3] == "2" {
                    let val_tmp = format!("{:.1} {:.1} {:.2}", x, y, h);
                    list_alt[((x - xmin).floor() / 3.0) as usize][((y - ymin ).floor() / 3.0) as usize].push(
                        val_tmp
                    );
                }
            }
        }
    }
    let w = ((xmax - xmin).floor() / 3.0) as usize;
    let h = ((ymax - ymin).floor() / 3.0) as usize;

    let f2 = File::create(&format!("{}/c2g.dxf", tmpfolder)).expect("Unable to create file");
    let mut f2 = BufWriter::new(f2);

    f2.write(format!("  0
SECTION
  2
HEADER
  9
$EXTMIN
 10
{}
 20
{}
  9
$EXTMAX
 10
{}
 20
{}
  0
ENDSEC
  0
SECTION
  2
ENTITIES
  0
", xmin, ymin, xmax, ymax).as_bytes()).expect("Cannot write dxf file");

    let f3 = File::create(&format!("{}/c3g.dxf", tmpfolder)).expect("Unable to create file");
    let mut f3 = BufWriter::new(f3);

    f3.write(format!("  0
SECTION
  2
HEADER
  9
$EXTMIN
 10
{}
 20
{}
  9
$EXTMAX
 10
{}
 20
{}
  0
ENDSEC
  0
SECTION
  2
ENTITIES
  0
", xmin, ymin, xmax, ymax).as_bytes()).expect("Cannot write dxf file");

    for x in 0..w+1 {
        for y in 0..h+1 {
            if list_alt[x][y].len() != 0 {
                let mut t = Vec::<&String>::new();
                if x >= 1 {
                    if y >= 1 {
                        t.extend(&list_alt[x - 1][y - 1]);
                    }
                    t.extend(&list_alt[x - 1][y]);
                    if y < h {
                        t.extend(&list_alt[x - 1][y + 1]);
                    }
                }
                if y >= 1 {
                    t.extend(&list_alt[x][y - 1]);
                }
                t.extend(&list_alt[x][y]);
                if y < h {
                    t.extend(&list_alt[x][y + 1]);
                }
                if x < h {
                    if y >= 1 {
                        t.extend(&list_alt[x + 1][y - 1]);
                    }
                    t.extend(&list_alt[x + 1][y]);
                    if y < h {
                        t.extend(&list_alt[x + 1][y + 1] );
                    }
                }
                let mut d = Vec::<&String>::new();
                d.extend(&list_alt[x][y]);

                if d.len() > 31 {
                    let b = ((d.len() - 1) as f64 / 30.0).floor() as usize;
                    let mut i: usize = 0;
                    while i < d.len() {
                        let mut e = i + b;
                        if e > d.len() {
                            e = d.len();
                        }
                        let _: Vec<_> = d.drain(i..e).collect();
                        i += 1;
                    }
                }
                if t.len() > 301 {
                    let b = ((t.len() - 1) as f64 / 300.0).floor() as usize;
                    let mut i: usize = 0;
                    while i < t.len() {
                        let mut e = i + b;
                        if e > t.len() {
                            e = t.len();
                        }
                        let _: Vec<_> = t.drain(i..e).collect();
                        i += 1;
                    }
                }
                let mut temp_max: f64 = f64::MIN;
                let mut temp_min: f64 = f64::MAX;
                for rec in t.iter() {
                    if t.len() > 0 {
                        let parts = rec.split(" ");
                        let r = parts.collect::<Vec<&str>>();
                        let h0: f64 = r[2].parse::<f64>().unwrap();
                        if temp_max < h0 {
                            temp_max = h0;
                        }
                        if
                            //true || // FIXME: bug in original script
                            temp_min > h0
                        {
                            temp_min = h0;
                        }
                    }
                }
                if temp_max - temp_min < c1_limit * 0.999 { 
                    d.clear();
                }

                for rec in d.iter() {
                    if d.len() > 0 {
                        let parts = rec.split(" ");
                        let r = parts.collect::<Vec<&str>>();
                        let x0: f64 = r[0].parse::<f64>().unwrap();
                        let y0: f64 = r[1].parse::<f64>().unwrap();
                        let h0: f64 = r[2].parse::<f64>().unwrap();

                        let cliff_length = 1.47;
                        let mut steep = steepness[((x0 - xstart) / size + 0.5).floor() as usize][((y0 - ystart) / size + 0.5).floor() as usize] - flat_place;
                        if steep < 0.0 { steep = 0.0;}
                        if steep > 17.0 { steep = 17.0;}
                        let bonus = (c2_limit-c1_limit)*(1.0-(no_small_ciffs-steep)/no_small_ciffs);
                        let limit = c1_limit + bonus;
                        let mut bonus = c2_limit * steep_factor * (steep - no_small_ciffs);
                        if bonus < 0.0 {
                            bonus = 0.0;
                        }
                        let limit2 = c2_limit + bonus;
                        for rec2 in t.iter() {
                            let partst = rec2.split(" ");
                            let rt = partst.collect::<Vec<&str>>();
                            let xt: f64 = rt[0].parse::<f64>().unwrap();
                            let yt: f64 = rt[1].parse::<f64>().unwrap();
                            let ht: f64 = rt[2].parse::<f64>().unwrap();
                            let temp = h0 - ht;
                            let dist = ((x0 - xt).powi(2) + (y0 - yt).powi(2)).sqrt();
                            if dist > 0.0 {
                                if steep < no_small_ciffs && temp > limit && temp > (limit + (dist - limit) * 0.85) {
                                    let p = img.get_pixel(((x0 + xt) / 2.0 - xmin + 0.5).floor() as u32, ((y0 + yt) / 2.0 - ymin + 0.5).floor() as u32);
                                    if p[0] == 255 {
                                        img.put_pixel(((x0 + xt) / 2.0 - xmin + 0.5).floor() as u32, ((y0 + yt) / 2.0 - ymin + 0.5).floor() as u32, Rgb([0, 0, 0]));
                                        f2.write("POLYLINE
 66
1
  8
cliff2
  0
".as_bytes()).expect("Cannot write dxf file");
                                        f2.write(
                                            format!(
                                                "VERTEX
  8
cliff2
 10
{}
 20
{}
  0
VERTEX
  8
cliff2
 10
{}
 20
{}
  0
SEQEND
  0
",
                                                (x0 + xt) / 2.0 + cliff_length * (y0 - yt) / dist,
                                                (y0 + yt) / 2.0 - cliff_length * (x0 - xt) / dist,
                                                (x0 + xt) / 2.0 - cliff_length * (y0 - yt) / dist,
                                                (y0 + yt) / 2.0 + cliff_length * (x0 - xt) / dist
                                            ).as_bytes()
                                        ).expect("Cannot write dxf file");
                                    }
                                }
                                if temp > limit2 && (temp > limit2 + (dist - limit2) * 0.85) {
                                    f3.write("POLYLINE
 66
1
  8
cliff3
  0
".as_bytes()).expect("Cannot write dxf file");
                                    f3.write(
                                        format!(
                                            "VERTEX
  8
cliff3
 10
{}
 20
{}
  0
VERTEX
  8
cliff3
 10
{}
 20
{}
  0
SEQEND
  0
",
                                            (x0 + xt) / 2.0 + cliff_length * (y0 - yt) / dist,
                                            (y0 + yt) / 2.0 - cliff_length * (x0 - xt) / dist,
                                            (x0 + xt) / 2.0 - cliff_length * (y0 - yt) / dist,
                                            (y0 + yt) / 2.0 + cliff_length * (x0 - xt) / dist
                                        ).as_bytes()
                                    ).expect("Cannot write dxf file");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    f2.write("ENDSEC
  0
".as_bytes()).expect("Cannot write dxf file");
    
    let c2_limit = 2.6 * 2.75;
    let xyz_file_in = format!("{}/xyz2.xyz", tmpfolder);

    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            if cliff_thin > rng.gen() {
                let ip = line.unwrap_or(String::new());
                let parts = ip.split(" ");
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap();
                let val_tmp = format!("{:.1} {:.1} {:.2}", x, y, h);
                list_alt[((x - xmin).floor() / 3.0) as usize][((y - ymin ).floor() / 3.0) as usize].push(
                    val_tmp
                );
            }
        }
    }

    for x in 0..w+1 {
        for y in 0..h+1 {
            if list_alt[x][y].len() != 0 {
                let mut t = Vec::<&String>::new();
                if x >= 1 {
                    if y >= 1 {
                        t.extend(&list_alt[x - 1][y - 1]);
                    }
                    t.extend(&list_alt[x - 1][y]);
                    if y < h {
                        t.extend(&list_alt[x - 1][y + 1]);
                    }
                }
                if y >= 1 {
                    t.extend(&list_alt[x][y - 1]);
                }
                t.extend(&list_alt[x][y]);
                if y < h {
                    t.extend(&list_alt[x][y + 1]);
                }
                if x < h {
                    if y >= 1 {
                        t.extend(&list_alt[x + 1][y - 1]);
                    }
                    t.extend(&list_alt[x + 1][y]);
                    if y < h {
                        t.extend(&list_alt[x + 1][y + 1] );
                    }
                }
                let mut d = Vec::<&String>::new();
                d.extend(&list_alt[x][y]);

                if d.len() > 31 {
                    let b = ((d.len() - 1) as f64 / 30.0).floor() as usize;
                    let mut i: usize = 0;
                    while i < d.len() {
                        let mut e = i + b;
                        if e > d.len() {
                            e = d.len();
                        }
                        let _: Vec<_> = d.drain(i..e).collect();
                        i += 1;
                    }
                }
                if t.len() > 301 {
                    let b = ((t.len() - 1) as f64 / 300.0).floor() as usize;
                    let mut i: usize = 0;
                    while i < t.len() {
                        let mut e = i + b;
                        if e > t.len() {
                            e = t.len();
                        }
                        let _: Vec<_> = t.drain(i..e).collect();
                        i += 1;
                    }
                }

                let mut temp_max: f64 = f64::MIN;
                let mut temp_min: f64 = f64::MAX;
                for rec in t.iter() {
                    if t.len() > 0 {
                        let parts = rec.split(" ");
                        let r = parts.collect::<Vec<&str>>();
                        let h0: f64 = r[2].parse::<f64>().unwrap();
                        if temp_max < h0 {
                            temp_max = h0;
                        }
                        if
                            //true || // FIXME: bug in original script
                            temp_min > h0
                        {
                            temp_min = h0;
                        }
                    }
                }

                if temp_max - temp_min < c2_limit * 0.999 { 
                    d.clear();
                }

                for rec in d.iter() {
                    if d.len() > 0 {
                        let parts = rec.split(" ");
                        let r = parts.collect::<Vec<&str>>();
                        let x0: f64 = r[0].parse::<f64>().unwrap();
                        let y0: f64 = r[1].parse::<f64>().unwrap();
                        let h0: f64 = r[2].parse::<f64>().unwrap();
                        let cliff_length = 1.47;
                        let limit = c2_limit;
                        for rec2 in t.iter() {
                            let partst = rec2.split(" ");
                            let rt = partst.collect::<Vec<&str>>();
                            let xt: f64 = rt[0].parse::<f64>().unwrap();
                            let yt: f64 = rt[1].parse::<f64>().unwrap();
                            let ht: f64 = rt[2].parse::<f64>().unwrap();
                            let temp = h0 - ht;
                            let dist = ((x0 - xt).powi(2) + (y0 - yt).powi(2)).sqrt();
                            if dist > 0.0 {
                                if temp > limit && temp > (limit + (dist -limit) * 0.85) {
                                    f3.write("POLYLINE
 66
1
  8
cliff4
  0
".as_bytes()).expect("Cannot write dxf file");
                                    f3.write(
                                        format!(
                                            "VERTEX
  8
cliff4
 10
{}
 20
{}
  0
VERTEX
  8
cliff4
 10
{}
 20
{}
  0
SEQEND
  0
",
                                            (x0 + xt) / 2.0 + cliff_length * (y0 - yt) / dist,
                                            (y0 + yt) / 2.0 - cliff_length * (x0 - xt) / dist,
                                            (x0 + xt) / 2.0 - cliff_length * (y0 - yt) / dist,
                                            (y0 + yt) / 2.0 + cliff_length * (x0 - xt) / dist
                                        ).as_bytes()
                                    ).expect("Cannot write dxf file");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    f3.write("ENDSEC
  0
".as_bytes()).expect("Cannot write dxf file");
    img.save(format!("{}/c2.png", tmpfolder)).expect("could not save output png");
    println!("Done");
    Ok(())
}

fn xyz2contours(thread: &String, cinterval: f64, xyzfilein: &str, xyzfileout: &str, dxffile: &str, ground: bool) -> Result<(), Box<dyn Error>> {
    println!("Running xyz2contours {} {} {} {} {} {}", thread, cinterval, xyzfilein, xyzfileout, dxffile, ground);

    let conf = Ini::load_from_file("pullauta.ini").unwrap();
    let scalefactor: f64 = conf.general_section().get("scalefactor").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    let water_class = conf.general_section().get("waterclass").unwrap_or("9");

    let tmpfolder = format!("temp{}", thread);

    let mut xmin: f64 = std::f64::MAX;
    let mut xmax: f64 = std::f64::MIN; 

    let mut ymin: f64 = std::f64::MAX;
    let mut ymax: f64 = std::f64::MIN;
    
    let mut hmin: f64 = std::f64::MAX; 
    let mut hmax: f64 = std::f64::MIN;
    
    let xyz_file_in = format!("{}/{}", tmpfolder, xyzfilein);
    
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            if (r.len() > 3 && (r[3] == "2" || r[3] == water_class)) || !ground {
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap();
                if xmin > x {
                    xmin = x;
                }
                
                if xmax < x {
                    xmax = x;
                }
                
                if ymin > y {
                    ymin = y;
                }
                
                if ymax < y {
                    ymax = y;
                }
                
                if hmin > h {
                    hmin = h;
                } 
                
                if hmax < h {
                    hmax = h;
                }
            }
        }
    }

    xmin = (xmin / 2.0 / scalefactor).floor() * 2.0 * scalefactor;
    ymin = (ymin / 2.0 / scalefactor).floor() * 2.0 * scalefactor;

    let w: usize = ((xmax - xmin).ceil() / 2.0 / scalefactor) as usize;
    let h: usize = ((ymax - ymin).ceil() / 2.0 / scalefactor) as usize;

    let mut list_alt = vec![vec![Vec::new(); h + 2]; w + 2];
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            if (r.len() > 3 && (r[3] == "2" || r[3] == water_class)) || !ground {
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap();

                list_alt[((x - xmin).floor() / 2.0 / scalefactor) as usize][((y - ymin).floor() / 2.0 / scalefactor) as usize].push(h);
            }
        }
    }
    let mut avg_alt = vec![vec![f64::NAN; h + 2]; w + 2];

    for x in 0..w+1 {
        for y in 0..h+1 {
            if !list_alt[x][y].is_empty() {
                avg_alt[x][y] = average(&list_alt[x][y]);
            }
        }
    }

    for x in 0..w+1 {
        for y in 0..h+1 {
            if avg_alt[x][y].is_nan() {
                // interpolate altitude of pixel
                // Todo: optimize to first clasify area then assign values
                let mut i1 = x;
                let mut i2 = x;
                let mut j1 = y;
                let mut j2 = y;

                while i1 > 0 && avg_alt[i1][y].is_nan() {
                    i1 -= 1;
                }

                while i2 < w && avg_alt[i2][y].is_nan() {
                    i2 += 1;
                }

                while  j1 > 0 && avg_alt[x][j1].is_nan(){
                    j1 -= 1;
                }

                while j2 < h && avg_alt[x][j2].is_nan() {
                    j2 += 1;
                }

                let mut val1 = f64::NAN;
                let mut val2 = f64::NAN;
                
                if !avg_alt[i1][y].is_nan() && !avg_alt[i2][y].is_nan() {
                    val1 = ((i2 - x) as f64 * avg_alt[i1][y] + (x - i1) as f64 * avg_alt[i2][y]) / ((i2 - i1) as f64);
                }

                if !avg_alt[x][j1].is_nan() && !avg_alt[x][j2].is_nan() {
                    val2 = ((j2 - y) as f64 * avg_alt[x][j1] + (y - j1) as f64 * avg_alt[x][j2]) / ((j2 - j1) as f64);
                }

                if !val1.is_nan() && !val2.is_nan() {
                    avg_alt[x][y] = (val1 + val2) / 2.0;
                } else if !val1.is_nan() {
                    avg_alt[x][y] = val1;
                } else if !val2.is_nan() {
                    avg_alt[x][y] = val2;
                }
                
            }
        }
    }

    for x in 0..w+1 {
        for y in 0..h+1 {
            if avg_alt[x][y].is_nan() {
                // second round of interpolation of altitude of pixel
                let mut val: f64 = 0.0;
                let mut c = 0;
                for i in 0..3 {
                    let ii: i32 = i as i32 - 1;
                    for j in 0..3 {
                        let jj: i32 = j as i32 - 1;
                        if y as i32 + jj >= 0 && x as i32 + ii >= 0 {
                            let x_idx = (x as i32 + ii) as usize;
                            let y_idx = (y as i32 + jj) as usize;
                            if x_idx <= w && y_idx <= h && !avg_alt[x_idx][y_idx].is_nan() {
                                c += 1;
                                val += avg_alt[x_idx][y_idx];
                            }
                        }
                    }
                }
                if c > 0 {
                    avg_alt[x][y] = val / c as f64;
                }
            }
        }
    }

    for x in 0..w+1 {
        for y in 1..h+1 {
            if avg_alt[x][y].is_nan() { 
                avg_alt[x][y] = avg_alt[x][y - 1]; 
            }
        }
        for yy in 1..h+1 {
            let y = h - yy;
            if avg_alt[x][y].is_nan() {
                avg_alt[x][y] = avg_alt[x][y + 1];
            }
        }
    }

    xmin += 1.0;
    ymin += 1.0;

    for x in 0..w+1 {
        for y in 0..h+1 {
            let mut ele = avg_alt[x][y];
            let temp: f64 = (ele / cinterval + 0.5).floor() * cinterval;
            if (ele - temp).abs() < 0.02 {
                if ele - temp < 0.0 {
                    ele = temp - 0.02;
                }
                else {
                    ele = temp + 0.02;
                }
                avg_alt[x][y] = ele;
            }
        }
    }
    
    if xyzfileout != "" && xyzfileout != "null" {
        let xyz_file_out = format!("{}/{}", tmpfolder, xyzfileout);
        let f = File::create(&xyz_file_out).expect("Unable to create file");
        let mut f = BufWriter::new(f);
        for x in 0..w+1 {
            for y in 0..h+1 {
                let ele = avg_alt[x][y];
                let xx = x as f64 * 2.0 * scalefactor + xmin as f64;
                let yy = y as f64 * 2.0 * scalefactor + ymin as f64;
                f.write(
                    format!(
                        "{} {} {}\n",
                        xx,
                        yy,
                        ele
                    ).as_bytes()
                ).expect("Cannot write to output file");
            }
        }
    }

    let v = cinterval;

    let mut progress: f64 = 0.0;
    let mut progprev: f64 = 0.0;
    let total: f64 = (hmax - hmin) / v;
    let mut level: f64 = (hmin / v).floor() * v;
    let polyline_out = format!("{}/temp_polylines.txt", tmpfolder);

    let f = File::create(&polyline_out).expect("Unable to create file");
    let mut f = BufWriter::new(f);
    f.write(b"").expect("Unable to create file");

    loop {
        if level >= hmax {
            break
        }
        progress += 1.0;
        if (progress / total * 18.0).floor() > progprev {
            progprev = (progress / total * 18.0).floor();
            println!("Generating temp polylines: {}%", (progress / total * 100.0).floor() as u32);
        }
        let mut obj = Vec::<String>::new();
        let mut curves: HashMap<String, String> = HashMap::new();
        
        for i in 1..((xmax - xmin).ceil() / 2.0 / scalefactor) as usize {
            for j in 2..((ymax - ymin).ceil() / 2.0 / scalefactor) as usize {
                let mut a = avg_alt[i][j];
                let mut b = avg_alt[i][j + 1];
                let mut c = avg_alt[i + 1][j];
                let mut d = avg_alt[i + 1][j + 1];
                
                if (a < level && b < level && c < level && d < level)
                || (a > level && b > level && c > level && d > level) {
                    // skip
                } else {
                    let temp: f64 = (a / v + 0.5).floor() * v;
                    if  (a - temp).abs() < 0.05 {
                        if a - temp < 0.0 {
                            a = temp - 0.05;
                        } else {
                            a = temp + 0.05;
                        }
                    }

                    let temp: f64 = (b / v + 0.5).floor() * v;
                    if  (b - temp).abs() < 0.05 {
                        if b - temp < 0.0 {
                            b = temp - 0.05;
                        } else {
                            b = temp + 0.05;
                        }
                    }

                    let temp: f64 = (c / v + 0.5).floor() * v;
                    if  (c - temp).abs() < 0.05 {
                        if c - temp < 0.0 {
                            c = temp - 0.05;
                        } else {
                            c = temp + 0.05;
                        }
                    }

                    let temp: f64 = (d / v + 0.5).floor() * v;
                    if  (d - temp).abs() < 0.05 {
                        if d - temp < 0.0 {
                            d = temp - 0.05;
                        } else {
                            d = temp + 0.05;
                        }
                    }

                    if a < b {
                        if level < b && level > a {
                            let x1: f64 = i as f64;
                            let y1: f64 = j as f64 + (level - a) / (b - a);
                            if level > c {
                                let x2: f64 = i as f64 + (b - level) / (b - c);
                                let y2: f64 = j as f64 + (level - c) / (b - c);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            } else if level < c {
                                let x2: f64 = i as f64 + (level - a) / (c - a);
                                let y2: f64 = j as f64;
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    } else if b < a {
                        if level < a && level > b {
                            let x1: f64 = i as f64;
                            let y1: f64 = j as f64 + (a - level) / (a - b);
                            if level < c {
                                let x2: f64 = i as f64 + (level - b) / (c - b);
                                let y2: f64 = j as f64 + (c - level) / (c - b);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            } else if level > c {
                                let x2: f64 = i as f64 + (a - level) / (a - c);
                                let y2: f64 = j as f64;
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    }

                    if a < c {
                        if level < c && level > a {
                            let x1: f64 = i as f64 + (level - a) / (c - a);
                            let y1: f64 = j as f64;
                            if level > b {
                                let x2: f64 = i as f64 + (level - b) / (c - b);
                                let y2: f64 = j as f64 + (c - level) / (c - b);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    } else if a > c {
                        if level < a && level > c {
                            let x1: f64 = i as f64 + (a - level) / (a - c);
                            let y1: f64 = j as f64;
                            if level < b {
                                let x2: f64 = i as f64 + (b - level) / (b - c);
                                let y2: f64 = j as f64 + (level - c) / (b - c);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    }

                    if c < d {
                        if level < d && level > c {
                            let x1: f64 = i as f64 + 1.0;
                            let y1: f64 = j as f64 + (level - c) / (d - c);
                            if level < b {
                                let x2: f64 = i as f64 + (b - level) / (b - c);
                                let y2: f64 = j as f64 + (level - c) / (b - c);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            } else if level > b {
                                let x2: f64 = i as f64 + (level - b) / (d - b);
                                let y2: f64 = j as f64 + 1.0;
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    } else if c > d {
                        if level < c && level > d {
                            let x1: f64 = i as f64 + 1.0;
                            let y1: f64 = j as f64 + (c - level) / (c - d);
                            if level > b {
                                let x2: f64 = i as f64 + (level - b) / (c - b);
                                let y2: f64 = j as f64 + (c - level) / (c - b);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            } else if level < b {
                                let x2: f64 = i as f64 + (b - level) / (b - d);
                                let y2: f64 = j as f64 + 1.0;
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    }

                    if d < b {
                        if level < b && level > d {
                            let x1: f64 = i as f64 + (b - level) / (b - d);
                            let y1: f64 = j as f64 + 1.0;
                            if level > c {
                                let x2: f64 = i as f64 + (b - level) / (b - c);
                                let y2: f64 = j as f64 + (level - c) / (b - c);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    } else if b < d {
                        if level < d && level > b {
                            let x1: f64 = i as f64 + (level - b) / (d - b);
                            let y1: f64 = j as f64 + 1.0;
                            if level < c {
                                let x2: f64 = i as f64 + (level - b) / (c - b);
                                let y2: f64 = j as f64 + (c - level) / (c - b);
                                check_obj_in(&mut obj, &mut curves, x1, x2, y1, y2);
                            }
                        }
                    }
                }
            }
        }

        let f = OpenOptions::new().append(true).open(&polyline_out).expect("Unable to create file");
        let mut f = BufWriter::new(f);

        for k in obj.iter() {
            if curves.contains_key(k) {
                let separator = "_".to_string();
                let parts = k.split(&separator);
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                f.write(format!("{},{};", x, y).as_bytes()).expect("Cannot write to output file");
                let mut res = format!("{}_{}", x, y);

                let parts = curves.get(&k.clone()).unwrap().split(&separator);
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                f.write(format!("{},{};", x, y).as_bytes()).expect("Cannot write to output file");
                curves.remove(&k.clone());
                
                let mut head = format!("{}_{}", x, y);
                if curves.get(&format!("{}_1", head)).unwrap_or(&String::new()) == &res {
                    curves.remove(&format!("{}_1", head));
                }
                if curves.get(&format!("{}_2", head)).unwrap_or(&String::new()) == &res {
                    curves.remove(&format!("{}_2", head));
                }
                loop {
                    if curves.contains_key(&format!("{}_1", head))
                    && curves.get(&format!("{}_1", head)).unwrap() != &res {
                        res = head.clone();

                        let parts = curves.get(&format!("{}_1", head)).unwrap().split(&separator);
                        let r = parts.collect::<Vec<&str>>();
                        let x: f64 = r[0].parse::<f64>().unwrap();
                        let y: f64 = r[1].parse::<f64>().unwrap();
                        f.write(format!("{},{};", x, y).as_bytes()).expect("Cannot write to output file");
                        curves.remove(&format!("{}_1", head));

                        head = format!("{}_{}", x, y);
                        if curves.get(&format!("{}_1", head)).unwrap_or(&String::new()) == &res {
                            curves.remove(&format!("{}_1", head));
                        }
                        if curves.get(&format!("{}_2", head)).unwrap_or(&String::new()) == &res {
                            curves.remove(&format!("{}_2", head));
                        }
                    } else {
                        if curves.contains_key(&format!("{}_2", head))
                        && curves.get(&format!("{}_2", head)).unwrap() != &res {
                            res = head.clone();

                            let parts = curves.get(&format!("{}_2", head)).unwrap().split(&separator);
                            let r = parts.collect::<Vec<&str>>();
                            let x: f64 = r[0].parse::<f64>().unwrap();
                            let y: f64 = r[1].parse::<f64>().unwrap();
                            f.write(format!("{},{};", x, y).as_bytes()).expect("Cannot write to output file");
                            curves.remove(&format!("{}_2", head));

                            head = format!("{}_{}", x, y);
                            if curves.get(&format!("{}_1", head)).unwrap_or(&String::new()) == &res {
                                curves.remove(&format!("{}_1", head));
                            }
                            if curves.get(&format!("{}_2", head)).unwrap_or(&String::new()) == &res {
                                curves.remove(&format!("{}_2", head));
                            }
                        } else {
                            f.write("\n".as_bytes()).expect("Cannot write to output file");
                            break
                        }
                    }
                }
            }
        }
        f.flush().expect("Cannot flush");
        level += v;
    }
    let f = File::create(&format!("{}/{}", tmpfolder, dxffile)).expect("Unable to create file");
    let mut f = BufWriter::new(f);

    f.write(format!("  0
SECTION
  2
HEADER
  9
$EXTMIN
 10
{}
 20
{}
  9
$EXTMAX
 10
{}
 20
{}
  0
ENDSEC
  0
SECTION
  2
ENTITIES
  0
", xmin, ymin, xmax, ymax).as_bytes()).expect("Cannot write dxf file");

    if let Ok(lines) = read_lines(&polyline_out) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(";");
            let r = parts.collect::<Vec<&str>>();   
            f.write("POLYLINE
 66
1
  8
cont
  0
".as_bytes()).expect("Cannot write dxf file");
            for (i, d) in r.iter().enumerate() {
                if d != &"" {
                    let ii = i + 1;
                    let ldata = r.len() - 2;
                    if ii > 5 && ii < ldata - 5 && ldata > 12 && ii % 2 == 0 {
                        continue;
                    }
                    let xy_raw = d.split(",");
                    let xy = xy_raw.collect::<Vec<&str>>();
                    let x: f64 = xy[0].parse::<f64>().unwrap() * 2.0 * scalefactor + xmin;
                    let y: f64 = xy[1].parse::<f64>().unwrap() * 2.0 * scalefactor + ymin;
                    f.write(format!("VERTEX
  8
cont
 10
{}
 20
{}
  0
", x, y).as_bytes()).expect("Cannot write dxf file");
                }
            }
            f.write("SEQEND
  0
".as_bytes()).expect("Cannot write dxf file");
        }
        f.write("ENDSEC
  0
EOF
".as_bytes()).expect("Cannot write dxf file");
        println!("Done");
    }
    Ok(())
}


fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where P: AsRef<Path>, {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn average(numbers: &Vec<f64>) -> f64 {
    let mut sum = 0.0;
    for n in numbers {
        sum += n;
    }
    sum / numbers.len() as f64 
}

fn check_obj_in (obj: &mut Vec<String>, curves: &mut HashMap<String, String>, x1: f64, x2: f64, y1: f64, y2: f64) {
    let x1 = (x1 * 100.0).floor() / 100.0;
    let x2 = (x2 * 100.0).floor() / 100.0;
    let y1 = (y1 * 100.0).floor() / 100.0;
    let y2 = (y2 * 100.0).floor() / 100.0;
    if x1 == x2 && y1 == y2 {

    } else {
        let mut key: String = format!("{}_{}_1", x1, y1);
        if !curves.contains_key(&key) {
            curves.insert(key.clone(), format!("{}_{}", x2, y2));
            obj.push(key.clone());
        } else {
            key = format!("{}_{}_2", x1, y1);
            curves.insert(key.clone(), format!("{}_{}", x2, y2));
            obj.push(key.clone());
        }
        key = format!("{}_{}_1", x2, y2);
        if !curves.contains_key(&key) {
            curves.insert(key.clone(), format!("{}_{}", x1, y1));
            obj.push(key.clone());
        } else {
            key = format!("{}_{}_2", x2, y2);
            curves.insert(key.clone(), format!("{}_{}", x1, y1));
            obj.push(key.clone());
        }
    }
}

fn makevegenew(thread: &String) -> Result<(), Box<dyn Error>> {
    println!("Running makevege");

    let tmpfolder = format!("temp{}", thread);

    let xyz_file_in = format!("{}/xyz2.xyz", tmpfolder);
    
    let mut xstart: f64 = 0.0;
    let mut ystart: f64 = 0.0;
    let mut size: f64 = 0.0;

    if let Ok(lines) = read_lines(&xyz_file_in) {
        for (i, line) in lines.enumerate() {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            if i == 0 {
                xstart = x;
                ystart = y;
            } else if i == 1 {
                size = y - ystart;
            } else {
                break;
            }
        }
    }

    let mut xmax: f64 = 0.0;
    let mut ymax: f64 = 0.0;

    let conf = Ini::load_from_file("pullauta.ini").unwrap();
    let block: f64 = conf.general_section().get("greendetectsize").unwrap_or("3").parse::<f64>().unwrap_or(3.0);
    
    let mut xyz: HashMap<(u64, u64), f64> = HashMap::new();
    let mut top: HashMap<(u64, u64), f64> = HashMap::new();
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            let h: f64 = r[2].parse::<f64>().unwrap();
            xyz.insert((((x - xstart) / size).floor() as u64,((y - ystart) / size).floor() as u64), h);
            if xmax < ((x - xstart) / size).floor() {
                xmax = ((x - xstart) / size).floor();
            }
            if ymax < ((y - ystart) / size).floor() {
                ymax = ((y - ystart) / size).floor();
            }

            if top.contains_key(&(((x - xstart) / block).floor() as u64,((y - ystart) / block).floor() as u64))
                && h > *top.get(&(((x - xstart) / block).floor() as u64,((y - ystart) / block).floor() as u64)).unwrap()
            {
                top.insert((((x - xstart) / block).floor() as u64,((y - ystart) / block).floor() as u64), h);
            }
        }
    }

    let mut zones = vec![];
    let mut i: u32 = 1;
    loop {
        let last_zone = conf.general_section().get(format!("zone{}", i)).unwrap_or("");
        if last_zone == "" {
            break
        }
        zones.push(last_zone);
        i += 1;
    }

    let mut thresholds = vec![];
    let mut i: u32 = 1;
    loop {
        let last_threshold = conf.general_section().get(format!("thresold{}", i)).unwrap_or("");
        if last_threshold == "" {
            break
        }
        thresholds.push(last_threshold);
        i += 1;
    }

    let greenshades = conf.general_section().get("greenshades").unwrap_or("").split("|").collect::<Vec<&str>>();
    // kayak
    let yellowheight: f64 = conf.general_section().get("yellowheight").unwrap_or("0.9").parse::<f64>().unwrap_or(0.9);
    let yellowthreshold: f64 = conf.general_section().get("yellowthreshold").unwrap_or("0.9").parse::<f64>().unwrap_or(0.9);
    let greenground: f64 = conf.general_section().get("greenground").unwrap_or("0.9").parse::<f64>().unwrap_or(0.9);
    let pointvolumefactor: f64 = conf.general_section().get("pointvolumefactor").unwrap_or("0.1").parse::<f64>().unwrap_or(0.1);
    let pointvolumeexponent: f64 = conf.general_section().get("pointvolumeexponent").unwrap_or("1").parse::<f64>().unwrap_or(1.0);
    let greenhigh: f64 = conf.general_section().get("greenhigh").unwrap_or("2").parse::<f64>().unwrap_or(2.0);
    let topweight: f64 = conf.general_section().get("topweight").unwrap_or("0.8").parse::<f64>().unwrap_or(0.8);
    let greentone: f64 = conf.general_section().get("lightgreentone").unwrap_or("200").parse::<f64>().unwrap_or(200.0);
    let zoffset: f64 = conf.general_section().get("vegezoffset").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let uglimit: f64 = conf.general_section().get("undergrowth").unwrap_or("0.35").parse::<f64>().unwrap_or(0.35);
    let uglimit2: f64 = conf.general_section().get("undergrowth2").unwrap_or("0.56").parse::<f64>().unwrap_or(0.56);
    let addition: i32 = conf.general_section().get("greendotsize").unwrap_or("0").parse::<i32>().unwrap_or(0);
    let firstandlastreturnasground = conf.general_section().get("firstandlastreturnasground").unwrap_or("").parse::<u64>().unwrap_or(1);
    let firstandlastfactor = conf.general_section().get("firstandlastreturnfactor").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let lastfactor = conf.general_section().get("lastreturnfactor").unwrap_or("0").parse::<f64>().unwrap_or(0.0);

    let yellowfirstlast = conf.general_section().get("yellowfirstlast").unwrap_or("").parse::<u64>().unwrap_or(1);
    let vegethin: u32 = conf.general_section().get("vegethin").unwrap_or("0").parse::<u32>().unwrap_or(0);
    
    let xyz_file_in = format!("{}/xyztemp.xyz", tmpfolder);
    
    let xmin = xstart;
    let ymin = ystart;
    let mut xmax: f64 = f64::MIN;
    let mut ymax: f64 = f64::MIN;

    let mut hits: HashMap<(u64, u64), u64> = HashMap::new();
    let mut yhit: HashMap<(u64, u64), u64> = HashMap::new();
    let mut noyhit: HashMap<(u64, u64), u64> = HashMap::new();

    if let Ok(lines) = read_lines(&xyz_file_in) {
        for (i, line) in lines.enumerate() {
            if vegethin == 0 || i as u32 + 1 % vegethin == 0 {
                let ip = line.unwrap_or(String::new());
                let parts = ip.split(" ");
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap();
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
                    if hits.contains_key(&(xx, yy)) {
                        *hits.get_mut(&(xx, yy)).unwrap() += 1;
                    } else {
                        hits.insert((xx, yy), 1);
                    }
                    if r[3] == "2" || h < yellowheight + *xyz.get(&(((x - xmin) / size).floor() as u64, ((y - ymin) / size).floor() as u64)).unwrap_or(&0.0) {
                        if yhit.contains_key(&(xx, yy)) {
                            *yhit.get_mut(&(xx, yy)).unwrap() += 1;
                        } else {
                            yhit.insert((xx, yy), 1);
                        }
                    } else {
                        if r[4] == "1" && r[5] == "1" {
                            if noyhit.contains_key(&(xx, yy)) {
                                *noyhit.get_mut(&(xx, yy)).unwrap() += yellowfirstlast;
                            } else {
                                noyhit.insert((xx, yy), yellowfirstlast);
                            }
                        } else {
                            if noyhit.contains_key(&(xx, yy)) {
                                *noyhit.get_mut(&(xx, yy)).unwrap() += 1;
                            } else {
                                noyhit.insert((xx, yy), 1);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut firsthit: HashMap<(u64, u64), u64> = HashMap::new();
    let mut ugg: HashMap<(u64, u64), u64> = HashMap::new();
    let mut ug: HashMap<(u64, u64), u64> = HashMap::new();
    let mut ghit: HashMap<(u64, u64), u64> = HashMap::new();
    let mut greenhit: HashMap<(u64, u64), f64> = HashMap::new();
    let mut highit: HashMap<(u64, u64), u64> = HashMap::new();
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for (i, line) in lines.enumerate() {
            if vegethin == 0 || i as u32 + 1 % vegethin == 0 {
                let ip = line.unwrap_or(String::new());
                let parts = ip.split(" ");
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let h: f64 = r[2].parse::<f64>().unwrap() - zoffset;
                if x > xmin && y > ymin {
                    
                    if r[5] == "1" {
                        let xx = ((x - xmin) / block + 0.5).floor() as u64;
                        let yy = ((y - ymin) / block + 0.5).floor() as u64;
                        if firsthit.contains_key(&(xx, yy)) {
                            *firsthit.get_mut(&(xx, yy)).unwrap() += 1;
                        } else {
                            firsthit.insert((xx, yy), 1);
                        }
                    }

                    let xx = ((x - xmin) / size).floor() as u64;
                    let yy = ((y - ymin) / size).floor() as u64;
                    let a = *xyz.get(&(xx, yy)).unwrap_or(&0.0);
                    let b = *xyz.get(&(xx + 1, yy)).unwrap_or(&0.0);
                    let c = *xyz.get(&(xx, yy + 1)).unwrap_or(&0.0);
                    let d = *xyz.get(&(xx + 1, yy + 1)).unwrap_or(&0.0);

                    let distx = (x - xmin) / size - xx as f64;
                    let disty = (y - ymin) / size - yy as f64;
                    
                    let ab = a * (1.0 - distx) + b * distx;
                    let cd = c * (1.0 - distx) + d * distx;
                    let thelele = ab * (1.0 - disty) + cd * disty;

                    let xx = ((x - xmin) / block / 6.0 + 0.5).floor() as u64;
                    let yy = ((y - ymin) / block / 6.0 + 0.5).floor() as u64;
                    let hh = h - thelele;
                    if hh <= 1.2 {
                        if r[3] == "2" {
                            if ugg.contains_key(&(xx, yy)) {
                                *ugg.get_mut(&(xx, yy)).unwrap() += 1;
                            } else {
                                ugg.insert((xx, yy), 1);
                            }
                        } else {
                            if hh > 0.25 {
                                if ug.contains_key(&(xx, yy)) {
                                    *ug.get_mut(&(xx, yy)).unwrap() += 1;
                                } else {
                                    ug.insert((xx, yy), 1);
                                }
                            } else {
                                if ugg.contains_key(&(xx, yy)) {
                                    *ugg.get_mut(&(xx, yy)).unwrap() += 1;
                                } else {
                                    ugg.insert((xx, yy), 1);
                                }
                            }
                        }
                    } else {
                        if ugg.contains_key(&(xx, yy)) {
                            *ugg.get_mut(&(xx, yy)).unwrap() += 1;
                        } else {
                            ugg.insert((xx, yy), 1);
                        }
                    }

                    let xx = ((x - xmin) / block + 0.5).floor() as u64;
                    let yy = ((y - ymin) / block + 0.5).floor() as u64;
                    let yyy = ((y - ymin) / block).floor() as u64; // necessary due to bug in perl version
                    if r[3] == "2" || greenground >= hh {
                        if r[4] == "1" && r[5] == "1" {
                            if ghit.contains_key(&(xx, yyy)) {
                                *ghit.get_mut(&(xx, yyy)).unwrap() += firstandlastreturnasground;
                            } else {
                                ghit.insert((xx, yyy), firstandlastreturnasground);
                            }
                        } else {
                            if ghit.contains_key(&(xx, yyy)) {
                                *ghit.get_mut(&(xx, yyy)).unwrap() += 1;
                            } else {
                                ghit.insert((xx, yyy), 1);
                            }
                        }
                    } else {
                        let mut last = 1.0;
                        if r[4] == r[5] {
                            last = lastfactor;
                            if hh < 5.0 {
                                last = firstandlastfactor;
                            }
                        }
                        for zone in zones.iter() {
                            let parts = zone.split("|");
                            let v = parts.collect::<Vec<&str>>();
                            let low: f64 = v[0].parse::<f64>().unwrap();
                            let high: f64 = v[1].parse::<f64>().unwrap();
                            let roof: f64 = v[2].parse::<f64>().unwrap();
                            let factor: f64 = v[3].parse::<f64>().unwrap(); 
                            if hh >= low && hh < high && *top.get(&(xx, yy)).unwrap_or(&0.0) - thelele < roof {
                                let offset = factor * last as f64; 
                                if greenhit.contains_key(&(xx, yy)) {
                                    *greenhit.get_mut(&(xx, yy)).unwrap() += offset;
                                } else {
                                    greenhit.insert((xx, yy), offset);
                                }
                                break;
                            } 
                        }

                        if greenhigh < hh {
                            if highit.contains_key(&(xx, yy)) {
                                *highit.get_mut(&(xx, yy)).unwrap() += 1;
                            } else {
                                highit.insert((xx, yy), 1);
                            }
                        }
                    }
                }
            }
        }
    }


    let w = (xmax - xmin).floor() / block;
    let h = (ymax - ymin).floor() / block;
    let wy = (xmax - xmin).floor() / 3.0;
    let hy = (ymax - ymin).floor() / 3.0;

    let scalefactor: f64 = conf.general_section().get("scalefactor").unwrap_or("1").parse::<f64>().unwrap_or(1.0);

    let mut imgug = RgbaImage::from_pixel(
        (w * block * 600.0 / 254.0 / scalefactor) as u32,
        (h * block * 600.0 / 254.0 / scalefactor) as u32,
        Rgba([255, 255, 255, 0])
    );
    let mut imggr1 = RgbImage::from_pixel((w * block) as u32, (h * block) as u32, Rgb([255, 255, 255]));
    let mut imggr1b = RgbImage::from_pixel((w * block) as u32, (h * block) as u32, Rgb([255, 255, 255]));
    let mut imgye2 = RgbaImage::from_pixel((w * block) as u32, (h * block) as u32, Rgba([255, 255, 255, 0]));
    let mut imgwater = RgbImage::from_pixel((w * block) as u32, (h * block) as u32, Rgb([255, 255, 255]));
    
    let mut greens = Vec::new();
    for i in 0..greenshades.len() {
        greens.push(Rgb([
            (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8,
            (254.0 - (74.0 / (greenshades.len() - 1) as f64) * i as f64) as u8,
            (greentone - greentone / (greenshades.len() - 1) as f64 * i as f64) as u8
        ]))
    }
    
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
    for x in 4..(wy as usize - 3) {
        for y in 4..(hy as usize - 3) {
            let mut ghit2 = 0;
            let mut highhit2 = 0;

            for i in x..x+2 {
                for j in y..y+2 {
                    ghit2 += *yhit.get(&(i as u64, j as u64)).unwrap_or(&0);
                    highhit2 += *noyhit.get(&(i as u64, j as u64)).unwrap_or(&0);
                }
            }
            if ghit2 as f64 / (highhit2 as f64 + ghit2 as f64 + 0.01) > yellowthreshold {
                draw_filled_rect_mut(
                    &mut imgye2, 
                    Rect::at(
                        x as i32 * 3 + 2,
                        (hy as i32 - y as i32) * 3 - 4
                    ).of_size(3, 3),
                    ye2
                );
            }
        }
    }

    for x in 2..w as usize {
        for y in 2..h as usize {
            let mut ghit2 = 0;
            let mut highit2 = 0;
            let roof = *top.get(&(x as u64, y as u64)).unwrap_or(&0.0) - *xyz.get(&((x as f64 * block / size).floor() as u64, (y as f64 * block / size).floor() as u64)).unwrap_or(&0.0);

            let greenhit2 = *greenhit.get(&(x as u64, y as u64)).unwrap_or(&0.0);
            let mut firsthit2 = *firsthit.get(&(x as u64, y as u64)).unwrap_or(&0);
            for i in (x-2) as usize..x+3 as usize {
                for j in (y-2) as usize..y+3 as usize {
                    if firsthit2 > *firsthit.get(&(i as u64, j as u64)).unwrap_or(&0) {
                        firsthit2 = *firsthit.get(&(i as u64, j as u64)).unwrap_or(&0);
                    }
                }
            }
            highit2 += *highit.get(&(x as u64, y as u64)).unwrap_or(&0);
            ghit2 += *ghit.get(&(x as u64, y as u64)).unwrap_or(&0);

            let mut greenlimit = 9999.0;
            for threshold in thresholds.iter() {
                let parts = threshold.split("|");
                let v = parts.collect::<Vec<&str>>();
                let v0: f64 = v[0].parse::<f64>().unwrap();
                let v1: f64 = v[1].parse::<f64>().unwrap();
                let v2: f64 = v[2].parse::<f64>().unwrap();
                if roof >= v0 && roof < v1 {
                    greenlimit = v2;
                    break;
                }
            }

            let mut greenshade = 0;

            let thevalue = greenhit2 / (ghit2 as f64 + greenhit2 + 1.0) * (1.0 - topweight + topweight * highit2 as f64 / (ghit2 as f64 + greenhit2 + highit2 as f64 + 1.0)) * (1.0 - pointvolumefactor * firsthit2 as f64 / (aveg + 0.00001)).powf(pointvolumeexponent);
            if thevalue > 0.0 {
                for (i, gshade) in greenshades.iter().enumerate() {
                    let shade = gshade.parse::<f64>().unwrap();
                    if thevalue > greenlimit * shade {
                        greenshade = i + 1;
                    }
                }
                if greenshade > 0 {
                    draw_filled_rect_mut(
                        &mut imggr1, 
                        Rect::at(
                            ((x as f64 + 0.5) * block) as i32 - addition, 
                            (((h - y as f64) - 0.5) * block) as i32 - addition
                        ).of_size(
                            (block as i32 + addition) as u32,
                            (block as i32 + addition) as u32,
                        ),
                        *greens.get(greenshade - 1).unwrap()
                    );
                }
            }
        }
    }
    let med: u32 = conf.general_section().get("medianboxsize").unwrap_or("0").parse::<u32>().unwrap_or(0);
    if med > 0 {
        imggr1b = median_filter(&imggr1, med/2, med/2);
    }
    let med2: u32 = conf.general_section().get("medianboxsize2").unwrap_or("0").parse::<u32>().unwrap_or(0);
    if med2 > 0 {
        imggr1 = median_filter(&imggr1b, med2/2, med2/2);
    } else {
        imggr1 = imggr1b;
    }
    imggr1.save(format!("{}/greens.png", tmpfolder)).expect("could not save output png");
    imgye2.save(format!("{}/yellow.png", tmpfolder)).expect("could not save output png");
    
    let mut img = image::open(format!("{}/greens.png", tmpfolder)).ok().expect("Opening image failed");
    let img2 = image::open(format!("{}/yellow.png", tmpfolder)).ok().expect("Opening image failed");
    image::imageops::overlay(&mut img, &img2, 0, 0);
    img.save(format!("{}/vegetation.png", tmpfolder)).expect("could not save output png");

    let black = Rgb([0, 0, 0]);
    let blue = Rgb([29, 190, 255]);
    let water = conf.general_section().get("waterclass").unwrap_or("").parse::<u64>().unwrap_or(0);
    let buildings = conf.general_section().get("buildingsclass").unwrap_or("").parse::<u64>().unwrap_or(0);
    if buildings > 0 || water > 0 {
        if let Ok(lines) = read_lines(&xyz_file_in) {
            for line in lines {
                let ip = line.unwrap_or(String::new());
                let parts = ip.split(" ");
                let r = parts.collect::<Vec<&str>>();
                let x: f64 = r[0].parse::<f64>().unwrap();
                let y: f64 = r[1].parse::<f64>().unwrap();
                let c: u64 = r[3].parse::<u64>().unwrap();
                if c == buildings {
                    draw_filled_rect_mut(
                        &mut imgwater, 
                        Rect::at(
                            (x - xmin) as i32 - 1,
                            (ymax - y) as i32 - 1,
                        ).of_size(3, 3),
                        black
                    );
                }
                if c == water {
                    draw_filled_rect_mut(
                        &mut imgwater, 
                        Rect::at(
                            (x - xmin) as i32 - 1,
                            (ymax - y) as i32 - 1,
                        ).of_size(3, 3),
                        blue
                    );
                }
            }
        }
    }
    let waterele = conf.general_section().get("waterelevation").unwrap_or("").parse::<f64>().unwrap_or(-999999.0);
    
    let xyz_file_in = format!("{}/xyz2.xyz", tmpfolder);
    if let Ok(lines) = read_lines(&xyz_file_in) {
        for line in lines {
            let ip = line.unwrap_or(String::new());
            let parts = ip.split(" ");
            let r = parts.collect::<Vec<&str>>();
            let x: f64 = r[0].parse::<f64>().unwrap();
            let y: f64 = r[1].parse::<f64>().unwrap();
            let hh: f64 = r[2].parse::<f64>().unwrap();
            if hh < waterele {
                draw_filled_rect_mut(
                    &mut imgwater, 
                    Rect::at(
                        (x - xmin) as i32 - 1,
                        (ymax - y) as i32 - 1,
                    ).of_size(3, 3),
                    blue
                );
            }
        }
    }
    imgwater.save(format!("{}/blueblack.png", tmpfolder)).expect("could not save output png");
    
    let underg = Rgba([64, 121, 0, 255]);
    let tmpfactor = (600.0 / 254.0 / scalefactor) as f32;

    let ww = w * block;
    let hh = h * block;

    let mut x = 0.0;
    loop {
        if x >= ww {
            break;
        }
        let mut y = 0.0;
        loop {
            if y >= hh {
                break;
            }
            let xx = (x/block/6.0).floor() as u64;
            let yy = (y/block/6.0).floor() as u64;
            let foo = *ug.get(&(xx, yy)).unwrap_or(&0) as f64 / (
                *ug.get(&(xx, yy)).unwrap_or(&0) as f64 +
                *ugg.get(&(xx, yy)).unwrap_or(&0) as f64 +
                0.01
            );
            if foo > uglimit {
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor*(x+block*3.0) as f32, tmpfactor*(h*block-y-block*3.0) as f32), 
                    (tmpfactor*(x+block*3.0) as f32, tmpfactor*(h*block-y+block*3.0) as f32), 
                    underg
                );
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor*(x+block*3.0) as f32 + 1.0, tmpfactor*(h*block-y-block*3.0) as f32), 
                    (tmpfactor*(x+block*3.0) as f32 + 1.0, tmpfactor*(h*block-y+block*3.0) as f32), 
                    underg
                );
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor*(x-block*3.0) as f32, tmpfactor*(h*block-y-block*3.0) as f32), 
                    (tmpfactor*(x-block*3.0) as f32, tmpfactor*(h*block-y+block*3.0) as f32), 
                    underg
                );
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor*(x-block*3.0) as f32 + 1.0, tmpfactor*(h*block-y-block*3.0) as f32), 
                    (tmpfactor*(x-block*3.0) as f32 + 1.0, tmpfactor*(h*block-y+block*3.0) as f32), 
                    underg
                );
            }
            if foo > uglimit2 {
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor*x as f32, tmpfactor*(h*block-y-block*3.0) as f32), 
                    (tmpfactor*x as f32, tmpfactor*(h*block-y+block*3.0) as f32), 
                    underg
                );
                draw_line_segment_mut(
                    &mut imgug, 
                    (tmpfactor * x as f32 + 1.0, tmpfactor as f32*(h*block-y-block*3.0) as f32), 
                    (tmpfactor * x as f32 + 1.0, tmpfactor as f32*(h*block-y+block*3.0) as f32), 
                    underg
                );
            }
            y += block * 6.0;
        }
        x += block * 6.0;
    }
    imgug.save(format!("{}/undergrowth.png", tmpfolder)).expect("could not save output png");
    
    let ugpgw = File::create(&format!("{}/undergrowth.pgw", tmpfolder)).expect("Unable to create file");
    let mut ugpgw = BufWriter::new(ugpgw);
    ugpgw.write(format!("{}
0.0
0.0
-{}
{}
{}", 1.0/tmpfactor, 1.0/tmpfactor, xmin, ymax).as_bytes()).expect("Cannot write pgw file");

    let vegepgw = File::create(&format!("{}/vegetation.pgw", tmpfolder)).expect("Unable to create file");
    let mut vegepgw = BufWriter::new(vegepgw);
    vegepgw.write(format!("1.0
0.0
0.0
-1.0
{}
{}", xmin, ymax).as_bytes()).expect("Cannot write pgw file");

    println!("Done");
    Ok(())
}