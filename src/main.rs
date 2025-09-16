use log::debug;
use log::info;
use pullauta::config::Config;
use pullauta::io::fs::FileSystem;
use pullauta::io::fs::memory::MemoryFileSystem;
use pullauta::shapefile;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::{thread, time};
fn main() {
    // setup and configure logging, default to INFO when RUST_LOG is not set
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buf, record| {
            use std::io::Write;
            let ts = buf.timestamp_seconds();
            let level_style = buf.default_level_style(record.level());

            writeln!(
                buf,
                "[{} {:?} {level_style}{}{level_style:#} {}] {}",
                ts,
                std::thread::current().id(),
                record.level(),
                record.module_path().unwrap_or(""),
                record.args()
            )
        })
        .init();

    let mut thread: String = String::new();

    let config =
        Arc::new(Config::load_or_create_default().expect("Could not open or create config file"));

    let fs = pullauta::io::fs::local::LocalFileSystem;

    let mut args: Vec<String> = env::args().collect();

    args.remove(0); // program name

    if !args.is_empty() && args[0].trim().parse::<usize>().is_ok() {
        thread = args.remove(0);
    }

    let command = if !args.is_empty() {
        args.remove(0)
    } else {
        String::new()
    };

    let command_lowercase = command.to_lowercase();

    if command.is_empty()
        || command_lowercase.ends_with(".las")
        || command_lowercase.ends_with(".laz")
        || command_lowercase.ends_with(".xyz")
        || command_lowercase.ends_with(".xyz.bin")
    {
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        println!("Karttapullautin v{VERSION}\nThere is no warranty. Use it at your own risk!\n");
    }

    let batch: bool = config.batch;

    let tmpfolder = PathBuf::from(format!("temp{thread}"));
    fs::create_dir_all(&tmpfolder).expect("Could not create tmp folder");

    let pnorthlinesangle = config.pnorthlinesangle;
    let pnorthlineswidth = config.pnorthlineswidth;

    if command.is_empty() && fs.exists(tmpfolder.join("vegetation.png")) && !batch {
        info!("Rendering png map with depressions");
        pullauta::render::render(
            &fs,
            &config,
            &thread,
            &tmpfolder,
            pnorthlinesangle,
            pnorthlineswidth,
            false,
        )
        .unwrap();
        info!("Rendering png map without depressions");
        pullauta::render::render(
            &fs,
            &config,
            &thread,
            &tmpfolder,
            pnorthlinesangle,
            pnorthlineswidth,
            true,
        )
        .unwrap();
        info!("\nAll done!");
        return;
    }

    if command.is_empty() && !batch {
        println!(
            "USAGE:\npullauta [parameter 1] [parameter 2] [parameter 3] ... [parameter n]\nSee README.MD for more details"
        );
        return;
    }

    if command == "cliffgeneralize" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "ground" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "ground2" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "groundfix" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "profile" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "makecliffsold" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "makeheight" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "xyzfixer" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "vege" {
        info!("Not implemented in this version, use the perl version");
        return;
    }

    if command == "internal2xyz" {
        if args.len() < 2 {
            info!("USAGE: internal2xyz [input file] [output file]");
            return;
        }

        pullauta::io::internal2xyz(&fs, &args[0], &args[1]).unwrap();
        return;
    }

    if command == "bin2dxf" {
        if args.len() < 2 {
            info!("USAGE: bin2dxf [.dxf.bin input file] [.dxf output file]");
            return;
        }
        pullauta::io::bin2dxf(&fs, &args[0], &args[1]).unwrap();
        return;
    }

    if command == "blocks" {
        pullauta::blocks::blocks(&fs, &tmpfolder).unwrap();
        return;
    }

    if command == "dotknolls" {
        pullauta::knolls::dotknolls(&fs, &config, &tmpfolder).unwrap();
        return;
    }

    if command == "dxfmerge" {
        pullauta::merge::bindxfmerge(&fs, &config).unwrap();
        return;
    }

    if command == "merge" {
        let mut scale = 1.0;
        if !args.is_empty() {
            scale = args[0].parse::<f64>().unwrap();
        }
        pullauta::merge::bindxfmerge(&fs, &config).unwrap();
        pullauta::merge::pngmergevege(&fs, &config, scale, false).unwrap();
        return;
    }

    if command == "knolldetector" {
        pullauta::knolls::knolldetector(&fs, &config, &tmpfolder).unwrap();
        return;
    }

    if command == "makecliffs" {
        pullauta::cliffs::makecliffs(&fs, &config, &tmpfolder).unwrap();
        return;
    }

    if command == "makevege" {
        pullauta::vegetation::makevege(&fs, &config, &tmpfolder).unwrap();
    }

    if command == "pngmerge" || command == "pngmergedepr" {
        let mut scale = 4.0;
        if !args.is_empty() {
            scale = args[0].parse::<f64>().unwrap();
        }
        pullauta::merge::pngmerge(&fs, &config, scale, command == "pngmergedepr").unwrap();
        return;
    }

    if command == "pngmergevege" {
        let mut scale = 1.0;
        if !args.is_empty() {
            scale = args[0].parse::<f64>().unwrap();
        }
        pullauta::merge::pngmergevege(&fs, &config, scale, false).unwrap();
        return;
    }

    if command == "pngmergevegeundergrowth" {
        let mut scale = 1.0;
        if !args.is_empty() {
            scale = args[0].parse::<f64>().unwrap();
        }
        pullauta::merge::pngmergevege(&fs, &config, scale, true).unwrap();
        return;
    }

    if command == "polylinedxfcrop" {
        let dxffilein = Path::new(&args[0]);
        let dxffileout = Path::new(&args[1]);
        let minx = args[2].parse::<f64>().unwrap();
        let miny = args[3].parse::<f64>().unwrap();
        let maxx = args[4].parse::<f64>().unwrap();
        let maxy = args[5].parse::<f64>().unwrap();

        // helpful message if normal dxf file is specified
        if dxffilein.extension().is_some_and(|e| e == "dxf")
            || dxffileout.extension().is_some_and(|e| e == "dxf")
        {
            info!(
                "The polylinedxfcrop command no longer takes raw DXF files as input and output. Please provide paths to `.bin.dxf` files instead."
            );
            return;
        }

        pullauta::crop::polylinebindxfcrop(
            &fs,
            dxffilein,
            dxffileout,
            config.output_dxf,
            minx,
            miny,
            maxx,
            maxy,
        )
        .unwrap();
        return;
    }

    if command == "pointdxfcrop" {
        let dxffilein = Path::new(&args[0]);
        let dxffileout = Path::new(&args[1]);
        let minx = args[2].parse::<f64>().unwrap();
        let miny = args[3].parse::<f64>().unwrap();
        let maxx = args[4].parse::<f64>().unwrap();
        let maxy = args[5].parse::<f64>().unwrap();
        if dxffilein.extension().is_some_and(|e| e == ".dxf")
            || dxffileout.extension().is_some_and(|e| e == ".dxf")
        {
            info!(
                "The pointdxfcrop command no longer takes raw DXF files as input and output. Please provide paths to `.bin.dxf` files instead."
            );
            return;
        }
        pullauta::crop::pointbindxfcrop(
            &fs,
            dxffilein,
            dxffileout,
            config.output_dxf,
            minx,
            miny,
            maxx,
            maxy,
        )
        .unwrap();
        return;
    }

    if command == "smoothjoin" {
        pullauta::merge::smoothjoin(&fs, &config, &tmpfolder).unwrap();
    }

    if command == "xyzknolls" {
        pullauta::knolls::xyzknolls(&fs, &config, &tmpfolder).unwrap();
    }

    #[cfg(feature = "shapefile")]
    if command == "unzipmtk" {
        pullauta::shapefile::unzip_and_render(&fs, &config, &tmpfolder, &args).unwrap();
    }

    #[cfg(feature = "shapefile")]
    if command == "mtkshaperender" {
        pullauta::shapefile::render(&fs, &config, &tmpfolder, false).unwrap();
    }

    if command == "xyz2contours" {
        let cinterval: f64 = args[0].parse::<f64>().unwrap();
        let xyzfilein = args[1].clone();
        let xyzfileout = args[2].clone();
        let dxffile = args[3].clone();
        let hmap = pullauta::contours::xyz2heightmap(&fs, &config, &tmpfolder, &xyzfilein).unwrap();

        if xyzfileout != "null" && !xyzfileout.is_empty() {
            hmap.to_file(&fs, xyzfileout).unwrap();
        }

        pullauta::contours::heightmap2contours(
            &fs,
            &tmpfolder,
            cinterval,
            &hmap,
            &dxffile,
            config.output_dxf,
        )
        .unwrap();
        return;
    }

    if command == "render" {
        let angle: f64 = args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .expect("expected first argument to be angle");
        let nwidth: usize = args
            .get(1)
            .and_then(|s| s.parse::<usize>().ok())
            .expect("expected second argument to be nwidth");
        let nodepressions: bool = args.len() > 2 && args[2] == "nodepressions";
        pullauta::render::render(
            &fs,
            &config,
            &thread,
            &tmpfolder,
            angle,
            nwidth,
            nodepressions,
        )
        .unwrap();
        return;
    }
    let proc = config.processes;
    if command.is_empty() && batch {
        // inner function to reduce code duplication
        fn launch_threads<F: FileSystem + Send + Clone + 'static>(
            fs: F,
            proc: u64,
            config: &Arc<Config>,
            zip_files: &[String],
        ) {
            let shapefiletmpdir = PathBuf::from("temp_shapefiles".to_string());
            fs.create_dir_all(&shapefiletmpdir).unwrap();
            if !zip_files.is_empty() {
                crate::shapefile::unzip_shapefiles(&fs, zip_files).unwrap();
            }
            // do the processing
            let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity((proc + 1) as usize);
            for i in 0..proc {
                let config = config.clone();
                let fs = fs.clone();
                let has_zip = !zip_files.is_empty();
                let handle = thread::spawn(move || {
                    info!("Starting thread");
                    pullauta::process::batch_process(&config, &fs, &format!("{}", i + 1), has_zip);
                    info!("Thread complete");
                });
                thread::sleep(time::Duration::from_millis(100));
                handles.push(handle);
            }
            for handle in handles {
                handle.join().unwrap();
            }
            fs.remove_dir_all(&shapefiletmpdir).unwrap();
        }

        let Config { lazfolder, .. } = &*config;

        let mut zip_files: Vec<String> = Vec::new();
        for path in fs.list(lazfolder).unwrap() {
            if let Some(extension) = path.extension() {
                if extension == "zip" {
                    zip_files.push(String::from(path.to_str().unwrap()));
                }
            }
        }

        if config.experimental_use_in_memory_fs {
            // copy all the input files into the memory file system
            let fs = pullauta::io::fs::memory::MemoryFileSystem::new();

            fs.create_dir_all(&config.lazfolder).unwrap();
            for file in fs::read_dir(&config.lazfolder).unwrap() {
                let file = file.unwrap();
                let path = file.path();
                println!("Copying {} into memory fs", path.display());
                fs.load_from_disk(&path, &path).unwrap();
            }
            // if there is an input vector file, copy it over as well

            if !config.vectorconf.is_empty() {
                let path = Path::new(&config.vectorconf);
                println!("Copying {} into memory fs", path.display());
                fs.load_from_disk(path, path).unwrap();
            }

            launch_threads(fs.clone(), proc, &config, &zip_files);

            // copy the output files back to disk
            std::fs::create_dir_all(&config.batchoutfolder).unwrap();
            for path in fs.list(&config.batchoutfolder).unwrap() {
                info!("Copying {} from memory fs to disk", path.display());
                fs.save_to_disk(&path, &path).unwrap();
            }
        } else {
            launch_threads(fs, proc, &config, &zip_files);
        }
        return;
    }

    if command_lowercase.ends_with(".zip") {
        let mut zips: Vec<String> = vec![command];
        zips.extend(args);
        pullauta::process::process_zip(&fs, &config, &thread, &tmpfolder, &zips, false).unwrap();
        return;
    }

    if command_lowercase.ends_with(".las")
        || command_lowercase.ends_with(".laz")
        || command_lowercase.ends_with(".xyz")
        || command_lowercase.ends_with(".xyz.bin")
    {
        let mut norender: bool = false;
        if args.len() > 1 {
            norender = args[1].clone() == "norender";
        }

        if config.experimental_use_in_memory_fs {
            let fs = pullauta::io::fs::memory::MemoryFileSystem::new();

            debug!("Copying input file into memory fs: {command}");
            // copy the input file into the memory file system
            fs.load_from_disk(Path::new(&command), Path::new("input.laz"))
                .expect("Could not copy input file into memory fs");

            debug!("Done");

            pullauta::process::process_tile(
                &fs,
                &config,
                &thread,
                &tmpfolder,
                // Path::new(&command),
                Path::new("input.laz"),
                norender,
            )
            .unwrap();

            // now write the output files to disk
            fn copy(fs: &MemoryFileSystem, name: &str) {
                if fs.exists(name) {
                    info!("Copying {name} from memory fs to disk");
                    fs.save_to_disk(name, name)
                        .expect("Could not copy from memory fs to disk");
                }
            }
            copy(&fs, "pullautus.png");
            copy(&fs, "pullautus_depr.png");
        } else {
            pullauta::process::process_tile(
                &fs,
                &config,
                &thread,
                &tmpfolder,
                Path::new(&command),
                norender,
            )
            .unwrap();
        }
    }
}
