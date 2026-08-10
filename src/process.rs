use anyhow::Context;
use image::{GrayImage, Luma, Rgb, RgbImage, Rgba, RgbaImage};
use itertools::izip;
use las::{PointDataBuilder, Reader};
use log::debug;
use log::info;
use rand::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::io::BufRead;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crate::blocks;
use crate::cliffs;
use crate::config::Config;
use crate::contours;
use crate::crop;
use crate::io::fs::FileSystem;
use crate::io::heightmap::HeightMap;
use crate::io::xyz::XyzInternalWriter;
use crate::io::xyz::XyzRecord;
use crate::knolls;
use crate::merge;
use crate::plan::InputFileIndex;
use crate::plan::Operation;
use crate::plan::Plan;
use crate::plan::Rect;
use crate::render;
use crate::util::Consumer;
use crate::util::Timing;
use crate::util::read_lines_no_alloc;
use crate::vegetation;

// compute the number of elements we can buffer for 50MB of memory usage during LAZ -> XyzRecord conversion
const LAZ_BUFFER_SIZE: usize =
    50 * 1024 * 1024 / (size_of::<las::Point>() + size_of::<XyzRecord>());

/// Launches threads and coordinates the logic for processing multiple files in parallell.
/// When it returns, all files have been processed and output files have been generated according to
/// the Config.
pub fn launch_threads<F: FileSystem + Send + Clone + 'static>(
    fs: F,
    config: Arc<Config>,
    zip_files: &[String],
) -> anyhow::Result<()> {
    // first unzip all zip files (if any) to a temporary folder, so that the threads can access the
    // shapefiles without having to worry about unzipping them in parallel
    let shapefiletmpdir = PathBuf::from("temp_shapefiles".to_string());
    fs.create_dir_all(&shapefiletmpdir)
        .context("Could not create temporary folder for shapefiles")?;
    if !zip_files.is_empty() {
        crate::shapefile::unzip_shapefiles(&fs, zip_files).unwrap();
    }

    // TODO: this is hard-coded but should maybe be configurable?
    let padding = 127.0;

    // folder where we store temporary extracted files to process later
    let staging_folder = Path::new("temp_staging");
    fs.create_dir_all(staging_folder)
        .context("Could not create staging folder")?;

    let timing = Timing::start_now("create_plan");
    let plan = crate::plan::Plan::new_from_input_files(
        fs.clone(),
        &config.lazfolder,
        &config.batchoutfolder,
        staging_folder,
        padding,
    )
    .context("creating plan")?;
    drop(timing);

    if plan.files_to_process().is_empty() {
        info!("No files to process, exiting");
        return Ok(());
    }

    let plan = Arc::new(plan);

    // make sure the output directory exists
    fs.create_dir_all(&config.batchoutfolder)
        .expect("Could not create output folder");

    // we only need to launch maximum as many threads as there are files to process
    let num_threads = config.processes.min(plan.files_to_process().len() as u64) as usize;

    // Create a queue where we send the files that are ready to process to the worker threads.
    // Bound it based on the number of threads so that we cannot have too many files converted
    // waiting for processing at a time.
    // TODO: make it configurable?
    let (tx, rx) = crate::util::make_bounded_queue::<InputFileIndex>((num_threads / 2).max(1));

    // do the processing
    let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(num_threads);
    for i in 0..num_threads {
        let config = config.clone();
        let fs = fs.clone();
        let rx = rx.clone();
        let has_zip = !zip_files.is_empty();
        let arc_plan = plan.clone();

        let handle = thread::Builder::new()
            .name(format!("worker_{i}"))
            .spawn(move || {
                info!("Starting thread");
                batch_process(&config, &fs, &format!("{}", i + 1), has_zip, arc_plan, rx);
            })
            .expect("Could not spawn thread");
        handles.push(handle);
    }

    // let mut planner = plan.naive_planner();
    let mut planner = plan.extract_once_planner();

    let mut writers: HashMap<InputFileIndex, XyzInternalWriter<_>> = HashMap::default();

    let options = las::ReaderOptions::default().with_laz_parallelism(if config.laz_parallel {
        las::LazParallelism::Yes
    } else {
        las::LazParallelism::No
    });
    log::debug!("Using LAZ parallelism: {:?}", config.laz_parallel);

    // prepare buffers needed for extraction of LAZ files based on the planned operations
    let mut records = Vec::with_capacity(LAZ_BUFFER_SIZE);

    let &Config {
        zoff, thinfactor, ..
    } = &*config;

    let mut rng = rand::rng();
    let randdist = rand::distr::Bernoulli::new(thinfactor).unwrap();

    while let Some(ops) = planner.next_operation() {
        for op in ops {
            match op {
                Operation::Extract { from, to } => {
                    log::trace!("Extracting from {from:?} to {to:?}");

                    let laz_p = plan.get_input_file(from);

                    let mut reader = Reader::with_options(
                        fs.open(&laz_p.path).expect("Could not open file"),
                        options,
                    )
                    .expect("Could not create reader");

                    let mut pd = PointDataBuilder::new().for_header(reader.header()).build();

                    loop {
                        // points.clear();
                        // PD.clear?
                        let n = reader
                            .fill_points(LAZ_BUFFER_SIZE as u64, &mut pd)
                            .expect("could not read LAZ points");
                        if n == 0 {
                            break;
                        }

                        // for each dependency, we need to check all points against their boundary
                        // to know if they should be included in the output.

                        for &to_i in &to {
                            let to_file = plan.get_input_file(to_i);
                            let padded_bounds = to_file.header.bounds.expand(padding);

                            let to_self = to_i == from;

                            // convert all read points to records
                            records.clear();
                            for (
                                pt_x,
                                pt_y,
                                pt_z,
                                pt_classification,
                                pt_number_of_returns,
                                pt_return_number,
                            ) in izip!(
                                pd.x(),
                                pd.y(),
                                pd.z(),
                                pd.classification(),
                                pd.number_of_returns(),
                                pd.return_number()
                            ) {
                                if (to_self || padded_bounds.contains(pt_x, pt_y))
                                    && (thinfactor == 1.0 || rng.sample(randdist))
                                {
                                    records.push(crate::io::xyz::XyzRecord {
                                        x: pt_x,
                                        y: pt_y,
                                        z: (pt_z + zoff) as f32,
                                        classification: pt_classification,
                                        number_of_returns: pt_number_of_returns,
                                        return_number: pt_return_number,
                                        ..Default::default()
                                    });
                                }
                            }

                            // get or create the writer for this file
                            let writer = match writers.entry(to_i) {
                                Entry::Occupied(e) => e.into_mut(),
                                Entry::Vacant(e) => {
                                    let outfile = &to_file.staging_path;
                                    let writer = XyzInternalWriter::new(
                                        fs.create(outfile)
                                            .context("Could not create output file")?,
                                    );
                                    e.insert(writer)
                                }
                            };

                            // write all at once
                            writer
                                .write_records(&records)
                                .expect("Could not write records");
                        }
                    }
                }
                Operation::Process { tile } => {
                    log::trace!("Processing tile {tile:?}");

                    // make sure we close the file for this tile
                    if let Some(mut w) = writers.remove(&tile) {
                        w.finish().context("Could not finish writer")?;
                    } else {
                        anyhow::bail!("Internal error: missing writer for tile {tile:?}");
                    };

                    // finally post output file for processing!
                    // This will block the main thread if the queue is full to provide backpressure.
                    tx.push(tile);
                }
            }
        }
    }

    // we are done, close the producer and wait for all threads to exit
    drop(tx);
    for handle in handles {
        handle.join().unwrap();
    }

    // cleanup extracted shapefiles
    fs.remove_dir_all(&shapefiletmpdir)
        .context("Could not remove temporary shapefile folder")?;
    Ok(())
}

pub fn process_zip(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    filenames: &[String],
    batch: bool,
) -> Result<(), Box<dyn Error>> {
    let mut timing = Timing::start_now("process_zip");
    let &Config {
        pnorthlineswidth,
        pnorthlinesangle,
        ..
    } = config;
    #[cfg(feature = "shapefile")]
    {
        if !batch && !filenames.is_empty() {
            info!("Rendering shape files");
            timing.start_section("unzip and render shape files");
            crate::shapefile::unzip_and_render(fs, config, tmpfolder, filenames).unwrap();
        } else {
            crate::shapefile::render(fs, config, tmpfolder, true).unwrap();
        }
    }

    info!("Rendering png map with depressions");
    timing.start_section("Rendering png map with depressions");
    render::render(
        fs,
        config,
        thread,
        tmpfolder,
        pnorthlinesangle,
        pnorthlineswidth,
        false,
    )
    .unwrap();

    info!("Rendering png map without depressions");
    timing.start_section("Rendering png map without depressions");
    render::render(
        fs,
        config,
        thread,
        tmpfolder,
        pnorthlinesangle,
        pnorthlineswidth,
        true,
    )
    .unwrap();

    Ok(())
}

pub fn process_tile(
    fs: &impl FileSystem,
    config: &Config,
    thread: &String,
    tmpfolder: &Path,
    input_file: &Path,
    skip_rendering: bool,
) -> Result<(), Box<dyn Error>> {
    let mut timing = Timing::start_now("process_tile");
    fs.create_dir_all(tmpfolder)
        .expect("Could not create tmp folder");

    let &Config {
        pnorthlinesangle,
        pnorthlineswidth,
        skipknolldetection,
        ..
    } = config;

    timing.start_section("preparing input file");
    info!("Preparing input file");

    let filename = input_file
        .file_name()
        .ok_or_else(|| format!("No extension for input file {}", input_file.display()))?
        .to_string_lossy()
        .to_lowercase();

    let target_file = tmpfolder.join("xyztemp.xyz.bin");

    if filename.ends_with(".xyz") {
        // if we are here we don't know if the file has at least 6 columns, but we assume that it is in the format
        // x y z classification number_of_returns return_number

        info!("Converting points from .xyz to internal binary format");

        debug!("Writing records to {:?}", target_file);
        let mut writer =
            XyzInternalWriter::new(fs.create(&target_file).expect("Could not create writer"));
        read_lines_no_alloc(fs, input_file, |line| {
            let mut parts = line.split(' ');
            let x = parts.next().unwrap().parse::<f64>().unwrap();
            let y = parts.next().unwrap().parse::<f64>().unwrap();
            let z = parts.next().unwrap().parse::<f32>().unwrap();

            let classification = parts.next().unwrap_or("2").parse::<u8>().unwrap();
            let number_of_returns = parts.next().unwrap_or("0").parse::<u8>().unwrap();
            let return_number = parts.next().unwrap_or("0").parse::<u8>().unwrap();

            writer
                .write_records(&[crate::io::xyz::XyzRecord {
                    x,
                    y,
                    z,
                    classification,
                    number_of_returns,
                    return_number,
                    ..Default::default()
                }])
                .expect("Could not write record");
        })
        .expect("Could not read file");
        writer.finish().expect("Unable to finish writing");
    } else if filename.ends_with(".laz") || filename.ends_with(".las") {
        info!("Converting points from .laz/laz to internal binary format");
        let &Config {
            thinfactor,
            xfactor,
            yfactor,
            zfactor,
            zoff,
            ..
        } = config;

        if thinfactor != 1.0 {
            info!("Using thinning factor {thinfactor}");
        }

        let mut rng = rand::rng();
        let randdist = rand::distr::Bernoulli::new(thinfactor).unwrap();

        let options = las::ReaderOptions::default().with_laz_parallelism(if config.laz_parallel {
            las::LazParallelism::Yes
        } else {
            las::LazParallelism::No
        });
        let mut reader =
            Reader::with_options(fs.open(input_file).expect("Could not open file"), options)
                .expect("Could not create reader");

        debug!("Writing records to {:?}", target_file);
        let mut writer =
            XyzInternalWriter::new(fs.create(&target_file).expect("Could not create writer"));

        let mut records = Vec::with_capacity(LAZ_BUFFER_SIZE);
        let mut pd = PointDataBuilder::new().for_header(reader.header()).build();
        loop {
            let n = reader.fill_points(LAZ_BUFFER_SIZE as u64, &mut pd).unwrap();

            if n == 0 {
                break;
            }

            // convert all read points to records
            records.clear();
            for (pt_x, pt_y, pt_z, pt_classification, pt_number_of_returns, pt_return_number) in izip!(
                pd.x(),
                pd.y(),
                pd.z(),
                pd.classification(),
                pd.number_of_returns(),
                pd.return_number()
            ) {
                if thinfactor == 1.0 || rng.sample(randdist) {
                    records.push(crate::io::xyz::XyzRecord {
                        x: pt_x * xfactor,
                        y: pt_y * yfactor,
                        z: (pt_z * zfactor + zoff) as f32,
                        classification: pt_classification,
                        number_of_returns: pt_number_of_returns,
                        return_number: pt_return_number,
                        ..Default::default()
                    });
                }
            }

            // write all at once
            writer.write_records(&records)?;
        }
        writer.finish().expect("Unable to finish writing");
    } else if filename.ends_with(".xyz.bin") {
        info!("Copying input file");
        fs.copy(input_file, target_file)
            .expect("Could not copy file");
    } else {
        return Err(format!("Unsupported input file: {}", input_file.display()).into());
    }

    info!("Done");

    info!("Knoll detection part 1");
    timing.start_section("knoll detection part 1");

    let &Config {
        scalefactor,
        vegeonly,
        cliffsonly,
        contoursonly,
        ..
    } = config;

    let xyz_03 = contours::xyz2heightmap(
        fs,
        config,
        tmpfolder,
        "xyztemp.xyz.bin", //point cloud in
    )
    .expect("contour generation failed");
    xyz_03.to_file(fs, tmpfolder.join("xyz_03.hmap")).unwrap();

    if !(vegeonly || cliffsonly) {
        contours::heightmap2contours(
            fs,
            tmpfolder,
            scalefactor * 0.3,
            &xyz_03,
            "contours03.dxf.bin", // dxf curves generated from the heightmap
            config.output_dxf,
        )
        .expect("contour generation failed");
    }
    drop(xyz_03);

    // copy the generated heightmap
    fs.copy(tmpfolder.join("xyz_03.hmap"), tmpfolder.join("xyz2.hmap"))
        .expect("Could not copy file");

    let &Config {
        contour_interval,
        basemapcontours,
        ..
    } = config;
    let halfinterval = contour_interval / 2.0 * scalefactor;

    if !vegeonly && !cliffsonly {
        if basemapcontours != 0.0 {
            info!("Basemap contours");
            let xyz2 = HeightMap::from_file(fs, tmpfolder.join("xyz2.hmap"))
                .expect("could not read xyz2 heightmap");
            contours::heightmap2contours(
                fs,
                tmpfolder,
                basemapcontours,
                &xyz2,
                "basemap.dxf.bin", // generate dxf contours
                config.output_dxf,
            )
            .expect("contour generation failed");
        }
        if !skipknolldetection {
            info!("Knoll detection part 2");
            timing.start_section("knoll detection part 2");
            knolls::knolldetector(fs, config, tmpfolder).unwrap();
        }
        info!("Contour generation part 1");
        timing.start_section("contour generation part 1");
        knolls::xyzknolls(fs, config, tmpfolder).unwrap(); // modifies the heightmap (but does not change dimensions

        info!("Contour generation part 2");
        timing.start_section("contour generation part 2");
        if !skipknolldetection {
            // contours 2.5
            let xyz_knolls = HeightMap::from_file(fs, tmpfolder.join("xyz_knolls.hmap"))
                .expect("could not read xyz_knolls heightmap");
            contours::heightmap2contours(
                fs,
                tmpfolder,
                halfinterval,
                &xyz_knolls,
                "out.dxf.bin", // generates dxf curves
                config.output_dxf,
            )
            .unwrap();
        } else {
            let hmap = contours::xyz2heightmap(fs, config, tmpfolder, "xyztemp.xyz.bin")
                .expect("could not generate heightmap");
            contours::heightmap2contours(
                fs,
                tmpfolder,
                halfinterval,
                &hmap,
                "out.dxf.bin", // generate dxf curves
                config.output_dxf,
            )
            .unwrap();
        }
        info!("Contour generation part 3");
        timing.start_section("contour generation part 3");
        merge::smoothjoin(fs, config, tmpfolder).unwrap();

        info!("Contour generation part 4");
        timing.start_section("contour generation part 4");
        knolls::dotknolls(fs, config, tmpfolder).unwrap();
    }

    if !cliffsonly && !contoursonly {
        info!("Vegetation generation");
        timing.start_section("vegetation generation");
        vegetation::makevege(fs, config, tmpfolder).unwrap();
    }

    if !vegeonly && !contoursonly {
        info!("Cliff generation");
        timing.start_section("cliff generation");
        cliffs::makecliffs(fs, config, tmpfolder).unwrap();
    }
    if !vegeonly && !contoursonly && !cliffsonly && config.detectbuildings {
        info!("Detecting buildings");
        timing.start_section("detecting buildings");
        blocks::blocks(fs, tmpfolder).unwrap();
    }
    if !skip_rendering && !vegeonly && !contoursonly && !cliffsonly {
        info!("Rendering png map with depressions");
        timing.start_section("rendering png map with depressions");
        render::render(
            fs,
            config,
            thread,
            tmpfolder,
            pnorthlinesangle,
            pnorthlineswidth,
            false,
        )
        .unwrap();

        info!("Rendering png map without depressions");
        timing.start_section("rendering png map without depressions");
        render::render(
            fs,
            config,
            thread,
            tmpfolder,
            pnorthlinesangle,
            pnorthlineswidth,
            true,
        )
        .unwrap();
    } else if contoursonly {
        info!("Rendering formlines");
        timing.start_section("rendering formlines");
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]));
        render::draw_curves(fs, config, &mut img, tmpfolder, false, false).unwrap();
    } else {
        info!("Skipped rendering");
    }
    info!("All done!");
    Ok(())
}

pub fn batch_process(
    conf: &Config,
    fs: &impl FileSystem,
    thread: &String,
    has_zip: bool,
    plan: Arc<Plan>,
    rx: Consumer<InputFileIndex>,
) {
    let &Config {
        vegeonly,
        cliffsonly,
        contoursonly,
        savetempfolders,
        savetempfiles,
        scalefactor,
        vege_bitmode,
        ..
    } = conf;

    let Config { batchoutfolder, .. } = conf;

    // take input files from queue until there are no more
    while let Some(laz_path) = rx.pop() {
        let file_to_process = plan.get_input_file(laz_path);

        let infile = file_to_process.path.as_path();
        let laz = infile.file_stem().unwrap().to_str().unwrap();
        let outfile = file_to_process.output_path.as_path();

        info!("{} -> {}", infile.display(), outfile.display());

        let Rect {
            minx,
            miny,
            maxx,
            maxy,
        } = file_to_process.header.bounds;

        // we need to move the input points from the staging to our own temporary folder
        let tmp_filename = PathBuf::from(format!("temp{thread}.xyz.bin"));
        debug!(
            "Moving input file {} -> {}",
            file_to_process.staging_path.display(),
            tmp_filename.display()
        );
        fs.rename(&file_to_process.staging_path, &tmp_filename)
            .expect("Could not move file to temporary folder");

        let tmpfolder = PathBuf::from(format!("temp{thread}"));

        if !has_zip {
            // Delete artifacts of a previous run where there would have been a zip
            let low_file = tmpfolder.join("low.png");
            if fs.exists(&low_file) {
                fs.remove_file(low_file).unwrap();
            }
            let high_file = tmpfolder.join("high.png");
            if fs.exists(&high_file) {
                fs.remove_file(high_file).unwrap();
            }
        }

        // Process the tile
        process_tile(fs, conf, thread, &tmpfolder, &tmp_filename, has_zip).unwrap();

        if has_zip && !vegeonly && !cliffsonly && !contoursonly {
            process_zip(fs, conf, thread, &tmpfolder, &[], true).unwrap();
        }

        // crop
        let tfw_in = PathBuf::from(format!("pullautus{thread}.pgw"));
        if fs.exists(&tfw_in) {
            let mut lines = fs.open(&tfw_in).expect("PGW file does not exist").lines();
            let tfw0 = lines
                .next()
                .expect("no 1 line")
                .expect("Could not read line 1")
                .parse::<f64>()
                .unwrap();
            let tfw1 = lines
                .next()
                .expect("no 2 line")
                .expect("Could not read line 2")
                .parse::<f64>()
                .unwrap();
            let tfw2 = lines
                .next()
                .expect("no 3 line")
                .expect("Could not read line 3")
                .parse::<f64>()
                .unwrap();
            let tfw3 = lines
                .next()
                .expect("no 4 line")
                .expect("Could not read line 4")
                .parse::<f64>()
                .unwrap();
            let tfw4 = lines
                .next()
                .expect("no 5 line")
                .expect("Could not read line 5")
                .parse::<f64>()
                .unwrap();
            let tfw5 = lines
                .next()
                .expect("no 6 line")
                .expect("Could not read line 6")
                .parse::<f64>()
                .unwrap();

            drop(lines);

            let dx = minx - tfw4;
            let dy = -maxy + tfw5;

            let mut pgw_file_out = fs.create(&tfw_in).expect("Unable to create file");
            write!(
                &mut pgw_file_out,
                "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n",
                tfw0,
                tfw1,
                tfw2,
                tfw3,
                minx + tfw0 / 2.0,
                maxy - tfw0 / 2.0
            )
            .expect("Unable to write to file");

            drop(pgw_file_out);
            fs.copy(
                Path::new(&format!("pullautus{thread}.pgw")),
                Path::new(&format!("pullautus_depr{thread}.pgw")),
            )
            .expect("Could not copy file");

            let orig_img = fs
                .read_image_png(format!("pullautus{thread}.png"))
                .expect("Opening image failed");
            let mut img = RgbImage::from_pixel(
                ((maxx - minx) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                ((maxy - miny) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                Rgb([255, 255, 255]),
            );
            image::imageops::overlay(
                &mut img,
                &orig_img.to_rgb8(),
                (-dx * 600.0 / 254.0 / scalefactor) as i64,
                (-dy * 600.0 / 254.0 / scalefactor) as i64,
            );

            img.write_to(
                &mut fs
                    .create(format!("pullautus{thread}.png"))
                    .expect("could not save output png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

            let orig_img = fs
                .read_image_png(format!("pullautus_depr{thread}.png"))
                .expect("Opening image failed");
            let mut img = RgbImage::from_pixel(
                ((maxx - minx) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                ((maxy - miny) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                Rgb([255, 255, 255]),
            );
            image::imageops::overlay(
                &mut img,
                &orig_img.to_rgb8(),
                (-dx * 600.0 / 254.0 / scalefactor) as i64,
                (-dy * 600.0 / 254.0 / scalefactor) as i64,
            );

            img.write_to(
                &mut fs
                    .create(format!("pullautus_depr{thread}.png"))
                    .expect("could not save output png"),
                image::ImageFormat::Png,
            )
            .expect("could not save output png");

            fs.copy(format!("pullautus{thread}.png"), outfile)
                .expect("Could not copy file to output folder");
            fs.copy(
                format!("pullautus{thread}.pgw"),
                format!("{batchoutfolder}/{laz}.pgw"),
            )
            .expect("Could not copy file to output folder");
            fs.copy(
                format!("pullautus_depr{thread}.png"),
                format!("{batchoutfolder}/{laz}_depr.png"),
            )
            .expect("Could not copy file to output folder");
            fs.copy(
                format!("pullautus_depr{thread}.pgw"),
                format!("{batchoutfolder}/{laz}_depr.pgw"),
            )
            .expect("Could not copy file to output folder");
        }

        if savetempfiles {
            if !contoursonly && !cliffsonly {
                let path = format!("temp{thread}/undergrowth.pgw");
                let tfw_in = Path::new(&path);
                let mut lines = fs.open(tfw_in).expect("PGW file does not exist").lines();
                let tfw0 = lines
                    .next()
                    .expect("no 1 line")
                    .expect("Could not read line 1")
                    .parse::<f64>()
                    .unwrap();
                let tfw1 = lines
                    .next()
                    .expect("no 2 line")
                    .expect("Could not read line 2")
                    .parse::<f64>()
                    .unwrap();
                let tfw2 = lines
                    .next()
                    .expect("no 3 line")
                    .expect("Could not read line 3")
                    .parse::<f64>()
                    .unwrap();
                let tfw3 = lines
                    .next()
                    .expect("no 4 line")
                    .expect("Could not read line 4")
                    .parse::<f64>()
                    .unwrap();
                let tfw4 = lines
                    .next()
                    .expect("no 5 line")
                    .expect("Could not read line 5")
                    .parse::<f64>()
                    .unwrap();
                let tfw5 = lines
                    .next()
                    .expect("no 6 line")
                    .expect("Could not read line 6")
                    .parse::<f64>()
                    .unwrap();

                let dx = minx - tfw4;
                let dy = -maxy + tfw5;

                let mut pgw_file_out = fs
                    .create(PathBuf::from(&format!(
                        "{batchoutfolder}/{laz}_undergrowth.pgw"
                    )))
                    .expect("Unable to create file");
                write!(
                    &mut pgw_file_out,
                    "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n",
                    tfw0,
                    tfw1,
                    tfw2,
                    tfw3,
                    minx + tfw0 / 2.0,
                    maxy - tfw0 / 2.0
                )
                .expect("Unable to write to file");
                drop(pgw_file_out);

                let mut orig_img_reader = image::ImageReader::new(
                    fs.open(format!("temp{thread}/undergrowth.png"))
                        .expect("Opening undergrowth image failed"),
                );
                orig_img_reader.set_format(image::ImageFormat::Png);
                orig_img_reader.no_limits();
                let orig_img = orig_img_reader.decode().unwrap();
                let mut img = RgbaImage::from_pixel(
                    ((maxx - minx) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                    ((maxy - miny) * 600.0 / 254.0 / scalefactor + 2.0) as u32,
                    Rgba([255, 255, 255, 0]),
                );
                image::imageops::overlay(
                    &mut img,
                    &orig_img,
                    (-dx * 600.0 / 254.0 / scalefactor) as i64,
                    (-dy * 600.0 / 254.0 / scalefactor) as i64,
                );

                img.write_to(
                    &mut fs
                        .create(format!("{batchoutfolder}/{laz}_undergrowth.png"))
                        .expect("could not save output png"),
                    image::ImageFormat::Png,
                )
                .expect("could not save output png");

                let mut orig_img_reader = image::ImageReader::new(
                    fs.open(format!("temp{thread}/vegetation.png"))
                        .expect("Opening vegetation image failed"),
                );
                orig_img_reader.set_format(image::ImageFormat::Png);
                orig_img_reader.no_limits();
                let orig_img = orig_img_reader.decode().unwrap();
                let mut img = RgbImage::from_pixel(
                    ((maxx - minx) + 1.0) as u32,
                    ((maxy - miny) + 1.0) as u32,
                    Rgb([255, 255, 255]),
                );
                image::imageops::overlay(&mut img, &orig_img.to_rgb8(), -dx as i64, -dy as i64);

                img.write_to(
                    &mut fs
                        .create(format!("{batchoutfolder}/{laz}_vege.png"))
                        .expect("could not save output png"),
                    image::ImageFormat::Png,
                )
                .expect("could not save output png");

                let mut pgw_file_out = fs
                    .create(format!("{batchoutfolder}/{laz}_vege.pgw"))
                    .expect("Unable to create file");
                write!(
                    &mut pgw_file_out,
                    "1.0\r\n0.0\r\n0.0\r\n-1.0\r\n{}\r\n{}\r\n",
                    minx + 0.5,
                    maxy - 0.5
                )
                .expect("Unable to write to file");

                drop(pgw_file_out);

                if vege_bitmode {
                    let mut orig_img_reader = image::ImageReader::new(
                        fs.open(format!("temp{thread}/vegetation_bit.png"))
                            .expect("Opening vegetation bit bit image failed"),
                    );
                    orig_img_reader.set_format(image::ImageFormat::Png);
                    orig_img_reader.no_limits();
                    let orig_img = orig_img_reader.decode().unwrap();
                    let mut img = GrayImage::from_pixel(
                        ((maxx - minx) + 1.0) as u32,
                        ((maxy - miny) + 1.0) as u32,
                        Luma([0]),
                    );
                    image::imageops::overlay(
                        &mut img,
                        &orig_img.to_luma8(),
                        -dx as i64,
                        -dy as i64,
                    );
                    img.write_to(
                        &mut fs
                            .create(format!("{batchoutfolder}/{laz}_vege_bit.png"))
                            .expect("could not save output png"),
                        image::ImageFormat::Png,
                    )
                    .expect("could not save output png");

                    let mut orig_img_reader = image::ImageReader::new(
                        fs.open(format!("temp{thread}/undergrowth_bit.png"))
                            .expect("Opening undergrowth bit image failed"),
                    );
                    orig_img_reader.set_format(image::ImageFormat::Png);
                    orig_img_reader.no_limits();
                    let orig_img = orig_img_reader.decode().unwrap();
                    let mut img = GrayImage::from_pixel(
                        ((maxx - minx) + 1.0) as u32,
                        ((maxy - miny) + 1.0) as u32,
                        Luma([0]),
                    );
                    image::imageops::overlay(
                        &mut img,
                        &orig_img.to_luma8(),
                        -dx as i64,
                        -dy as i64,
                    );
                    img.write_to(
                        &mut fs
                            .create(format!("{batchoutfolder}/{laz}_undergrowth_bit.png"))
                            .expect("could not save output png"),
                        image::ImageFormat::Png,
                    )
                    .expect("could not save output png");

                    fs.copy(
                        format!("{batchoutfolder}/{laz}_vege.pgw"),
                        format!("{batchoutfolder}/{laz}_vege_bit.pgw"),
                    )
                    .expect("Could not copy file");

                    fs.copy(
                        format!("{batchoutfolder}/{laz}_vege.pgw"),
                        format!("{batchoutfolder}/{laz}_undergrowth_bit.pgw"),
                    )
                    .expect("Could not copy file");
                }
            }

            let out2_path = PathBuf::from(format!("temp{thread}/out2.dxf.bin"));
            if fs.exists(&out2_path) {
                crop::polylinebindxfcrop(
                    fs,
                    &out2_path,
                    Path::new(&format!("{batchoutfolder}/{laz}_contours.dxf.bin")),
                    conf.output_dxf,
                    minx,
                    miny,
                    maxx,
                    maxy,
                )
                .unwrap();
            }
            let dxf_files = ["c2g", "c3g", "contours03", "detected", "formlines"];
            for dxf_file in dxf_files.iter() {
                let dxf_path = PathBuf::from(format!("temp{thread}/{dxf_file}.dxf.bin"));
                if fs.exists(&dxf_path) {
                    crop::polylinebindxfcrop(
                        fs,
                        &dxf_path,
                        Path::new(&format!("{batchoutfolder}/{laz}_{dxf_file}.dxf.bin")),
                        conf.output_dxf,
                        minx,
                        miny,
                        maxx,
                        maxy,
                    )
                    .unwrap();
                }
            }
            let dotknolls_file = PathBuf::from(format!("temp{thread}/dotknolls.dxf.bin"));
            if fs.exists(&dotknolls_file) {
                crop::pointbindxfcrop(
                    fs,
                    &dotknolls_file,
                    Path::new(&format!("{batchoutfolder}/{laz}_dotknolls.dxf.bin")),
                    conf.output_dxf,
                    minx,
                    miny,
                    maxx,
                    maxy,
                )
                .unwrap();
            }
        }

        let basemap_file = PathBuf::from(format!("temp{thread}/basemap.dxf.bin"));
        if fs.exists(&basemap_file) {
            crop::polylinebindxfcrop(
                fs,
                &basemap_file,
                Path::new(&format!("{batchoutfolder}/{laz}_basemap.dxf.bin")),
                conf.output_dxf,
                minx,
                miny,
                maxx,
                maxy,
            )
            .unwrap();
        }
        if savetempfolders {
            fs.create_dir_all(format!("temp_{laz}_dir"))
                .expect("Could not create output folder");
            for path in fs.list(format!("temp{thread}")).unwrap() {
                if fs.exists(&path) {
                    let filename = path.file_name().unwrap().to_str().unwrap();
                    fs.copy(&path, Path::new(&format!("temp_{laz}_dir/{filename}")))
                        .unwrap();
                }
            }
        }
    }
}
