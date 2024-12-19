use std::{path::Path, str::FromStr};

use ini::Ini;

/// The config parsed from the .ini configuration file.
pub struct Config {
    pub batch: bool,
    pub processes: u64,

    pub experimental_use_in_memory_fs: bool,

    // only one can be set at a time
    pub vegeonly: bool,
    pub cliffsonly: bool,
    pub contoursonly: bool,

    pub pnorthlinesangle: f64,
    pub pnorthlineswidth: usize,

    pub lazfolder: String,
    pub batchoutfolder: String,
    pub savetempfiles: bool,
    pub savetempfolders: bool,

    pub scalefactor: f64,
    pub vege_bitmode: bool,
    pub zoff: f64,
    pub thinfactor: f64,

    pub skipknolldetection: bool,
    pub vegemode: bool,

    pub xfactor: f64,
    pub yfactor: f64,
    pub zfactor: f64,

    pub contour_interval: f64,
    pub basemapcontours: f64,

    pub detectbuildings: bool,

    pub water_class: u8,

    // merge
    pub inidotknolls: f64,
    pub smoothing: f64,
    pub curviness: f64,
    pub indexcontours: f64,
    pub formline: f64,
    pub depression_length: usize,

    // cliffs
    pub c1_limit: f64,
    pub c2_limit: f64,
    pub cliff_thin: f64,
    pub steep_factor: f64,
    pub flat_place: f64,
    pub no_small_ciffs: f64,

    // vegetation
    pub zones: Vec<Zone>,
    pub thresholds: Vec<(f64, f64, f64)>,
    pub greenshades: Vec<f64>,
    pub yellowheight: f64,
    pub yellowthreshold: f64,
    pub greenground: f64,
    pub pointvolumefactor: f64,
    pub pointvolumeexponent: f64,
    pub greenhigh: f64,
    pub topweight: f64,
    pub greentone: f64,
    pub vegezoffset: f64,
    pub uglimit: f64,
    pub uglimit2: f64,
    pub addition: i32,
    pub firstandlastreturnasground: u64,
    pub firstandlastfactor: f64,
    pub lastfactor: f64,
    pub yellowfirstlast: u64,
    pub vegethin: u32,
    pub greendetectsize: f64,
    pub proceed_yellows: bool,
    pub med: u32,
    pub med2: u32,
    pub medyellow: u32,
    pub water: u8,
    pub buildings: u8,
    pub waterele: f64,

    // render
    pub buildingcolor: (u8, u8, u8),
    pub vectorconf: String,
    pub mtkskiplayers: Vec<String>,
    pub cliffdebug: bool,

    pub formlinesteepness: f64,
    // pub formline: f64,
    pub formlineaddition: f64,
    pub dashlength: f64,
    pub gaplength: f64,
    pub minimumgap: u32,
    pub label_depressions: bool,
    pub remove_touching_contours: bool,
}

pub struct Zone {
    pub low: f64,
    pub high: f64,
    pub roof: f64,
    pub factor: f64,
}

const DEFAULT_CONFIG_FILE: &str = "pullauta.ini";

impl Config {
    pub fn load_or_create_default() -> Result<Self, Box<dyn std::error::Error>> {
        let path = Path::new(DEFAULT_CONFIG_FILE);
        // populate the default if no file was found
        if !path.exists() {
            std::fs::write(path, include_bytes!("../pullauta.default.ini"))?;
        }
        Self::from_file(path)
    }

    fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let conf = Ini::load_from_file(path)?;

        let gs = conf.general_section();

        // only one can be set at a time
        let vegeonly: bool = gs.get("vegeonly").unwrap_or("0") == "1";
        let cliffsonly: bool = gs.get("cliffsonly").unwrap_or("0") == "1";
        let contoursonly: bool = gs.get("contoursonly").unwrap_or("0") == "1";

        // clippy complains about this, but we want it like this for understandability
        #[allow(clippy::nonminimal_bool)]
        if (vegeonly && (cliffsonly || contoursonly))
            || (cliffsonly && (vegeonly || contoursonly))
            || (contoursonly && (vegeonly || cliffsonly))
        {
            return Err(
                "Only one of vegeonly, cliffsonly, or contoursonly can be set!"
                    .to_string()
                    .into(),
            );
        }

        fn parse_typed<T: FromStr>(props: &ini::Properties, name: &str, default: T) -> T {
            props
                .get(name)
                .and_then(|s| s.parse::<T>().ok())
                .unwrap_or(default)
        }

        let pnorthlinesangle: f64 = parse_typed(gs, "northlinesangle", 0.0);
        let pnorthlineswidth: usize = parse_typed(gs, "northlineswidth", 0);

        let processes: u64 = gs.get("processes").unwrap().parse::<u64>().unwrap();
        let experimental_use_in_memory_fs: bool =
            gs.get("experimental_use_in_memory_fs").unwrap_or("0") == "1";

        let lazfolder = gs.get("lazfolder").unwrap_or("").to_string();
        let batchoutfolder = gs.get("batchoutfolder").unwrap_or("").to_string();
        let savetempfiles: bool = gs.get("savetempfiles").unwrap() == "1";
        let savetempfolders: bool = gs.get("savetempfolders").unwrap() == "1";

        let scalefactor: f64 = parse_typed(gs, "scalefactor", 1.0);
        let vege_bitmode: bool = gs.get("vege_bitmode").unwrap_or("0") == "1";
        let zoff = parse_typed(gs, "zoffset", 0.0);
        let mut thinfactor: f64 = parse_typed(gs, "thinfactor", 1.0);
        if !(0.0..=1.0).contains(&thinfactor) {
            return Err(format!(
                "Value {} of `thinfactor` is outside the allowed range of 0.0 to 1.0",
                thinfactor
            )
            .into());
        }
        if thinfactor == 0.0 {
            thinfactor = 1.0;
        }

        let skipknolldetection = gs.get("skipknolldetection").unwrap_or("0") == "1";
        let vegemode: bool = gs.get("vegemode").unwrap_or("0") == "1";
        if vegemode {
            return Err("vegemode=1 not implemented, use perl version"
                .to_string()
                .into());
        }

        let mut xfactor: f64 = parse_typed(gs, "coordxfactor", 1.0);
        let mut yfactor: f64 = parse_typed(gs, "coordyfactor", 1.0);
        let mut zfactor: f64 = parse_typed(gs, "coordzfactor", 1.0);
        if xfactor == 0.0 {
            xfactor = 1.0;
        }
        if yfactor == 0.0 {
            yfactor = 1.0;
        }
        if zfactor == 0.0 {
            zfactor = 1.0;
        }

        let contour_interval: f64 = parse_typed(gs, "contour_interval", 5.0);

        let basemapcontours: f64 = parse_typed(gs, "basemapinterval", 0.0);

        let detectbuildings: bool = gs.get("detectbuildings").unwrap_or("0") == "1";

        let water_class = parse_typed(gs, "waterclass", 9);

        let inidotknolls: f64 = parse_typed(gs, "knolls", 0.8);
        let smoothing: f64 = parse_typed(gs, "smoothing", 1.0);
        let curviness: f64 = parse_typed(gs, "curviness", 1.0);
        let indexcontours: f64 = parse_typed(gs, "indexcontours", 12.5);
        let formline: f64 = parse_typed(gs, "formline", 2.0);

        let depression_length: usize = parse_typed(gs, "depression_length", 181);

        // cliffs
        let c1_limit: f64 = parse_typed(gs, "cliff1", 1.0);
        let c2_limit: f64 = parse_typed(gs, "cliff2", 1.0);
        let cliff_thin: f64 = parse_typed(gs, "cliffthin", 1.0);
        if !(0.0..=1.0).contains(&cliff_thin) {
            return Err(format!(
                "Value {} of `cliffthin` is outside the allowed range of 0.0 to 1.0",
                cliff_thin
            )
            .into());
        }
        let steep_factor: f64 = parse_typed(gs, "cliffsteepfactor", 0.33);
        let flat_place: f64 = parse_typed(gs, "cliffflatplace", 6.6);
        let no_small_ciffs: f64 = parse_typed(gs, "cliffnosmallciffs", 0.0);

        // vegetation

        let mut zones = vec![];
        let mut i: u32 = 1;
        loop {
            let zone = gs.get(format!("zone{}", i)).unwrap_or("");
            if zone.is_empty() {
                break;
            }

            let mut parts = zone.split('|');

            zones.push(Zone {
                low: parts.next().unwrap().parse::<f64>().unwrap(),
                high: parts.next().unwrap().parse::<f64>().unwrap(),
                roof: parts.next().unwrap().parse::<f64>().unwrap(),
                factor: parts.next().unwrap().parse::<f64>().unwrap(),
            });
            i += 1;
        }
        let thresholds = {
            let mut thresholds = vec![];
            let mut i: u32 = 1;
            loop {
                let last_threshold = gs.get(format!("thresold{}", i)).unwrap_or("");
                if last_threshold.is_empty() {
                    break;
                }
                // parse the threshold values
                let mut parts = last_threshold.split('|');
                let v0: f64 = parts.next().unwrap().parse::<f64>().unwrap();
                let v1: f64 = parts.next().unwrap().parse::<f64>().unwrap();
                let v2: f64 = parts.next().unwrap().parse::<f64>().unwrap();

                thresholds.push((v0, v1, v2));
                i += 1;
            }
            thresholds
        };

        let greenshades = gs
            .get("greenshades")
            .unwrap_or("")
            .split('|')
            .map(|v| v.parse::<f64>().unwrap())
            .collect::<Vec<f64>>();
        let yellowheight: f64 = parse_typed(gs, "yellowheight", 0.9);
        let yellowthreshold: f64 = parse_typed(gs, "yellowthresold", 0.9);
        let greenground: f64 = parse_typed(gs, "greenground", 0.9);
        let pointvolumefactor: f64 = parse_typed(gs, "pointvolumefactor", 0.1);
        let pointvolumeexponent: f64 = parse_typed(gs, "pointvolumeexponent", 1.0);
        let greenhigh: f64 = parse_typed(gs, "greenhigh", 2.0);
        let topweight: f64 = parse_typed(gs, "topweight", 0.8);
        let greentone: f64 = parse_typed(gs, "lightgreentone", 200.0);
        let vegezoffset: f64 = parse_typed(gs, "vegezoffset", 0.0);
        let uglimit: f64 = parse_typed(gs, "undergrowth", 0.35);
        let uglimit2: f64 = parse_typed(gs, "undergrowth2", 0.56);
        let addition: i32 = parse_typed(gs, "greendotsize", 0);
        let firstandlastreturnasground = parse_typed(gs, "firstandlastreturnasground", 1);
        let firstandlastfactor = parse_typed(gs, "firstandlastreturnfactor", 0.0);
        let lastfactor = parse_typed(gs, "lastreturnfactor", 0.0);

        let yellowfirstlast = parse_typed(gs, "yellowfirstlast", 1);
        let vegethin: u32 = parse_typed(gs, "vegethin", 0);

        let greendetectsize: f64 = parse_typed(gs, "greendetectsize", 3.0);
        let proceed_yellows: bool = gs.get("yellow_smoothing").unwrap_or("0") == "1";
        let med: u32 = parse_typed(gs, "medianboxsize", 0);
        let med2: u32 = parse_typed(gs, "medianboxsize2", 0);
        let medyellow: u32 = parse_typed(gs, "yellowmedianboxsize", 0);
        let water = parse_typed(gs, "waterclass", 0);
        let buildings = parse_typed(gs, "buildingsclass", 0);
        let waterele = parse_typed(gs, "waterelevation", -999999.0);

        // render
        let buildingcolor: (u8, u8, u8) = {
            let mut split = gs.get("buildingcolor").unwrap_or("0,0,0").split(',');
            (
                split.next().unwrap_or("0").parse::<u8>().unwrap_or(0),
                split.next().unwrap_or("0").parse::<u8>().unwrap_or(0),
                split.next().unwrap_or("0").parse::<u8>().unwrap_or(0),
            )
        };

        let vectorconf = gs.get("vectorconf").unwrap_or("").into();
        let mtkskiplayers: Vec<String> = gs
            .get("mtkskiplayers")
            .unwrap_or("")
            .split(',')
            .map(Into::into)
            .collect();

        let cliffdebug: bool = gs.get("cliffdebug").unwrap_or("0") == "1";

        let formlinesteepness: f64 = parse_typed(gs, "formlinesteepness", 0.37);
        let formlineaddition: f64 = parse_typed(gs, "formlineaddition", 13.0);
        let dashlength: f64 = parse_typed(gs, "dashlength", 60.0);
        let gaplength: f64 = parse_typed(gs, "gaplength", 12.0);
        let minimumgap: u32 = parse_typed(gs, "minimumgap", 30);
        let label_depressions: bool = gs.get("label_formlines_depressions").unwrap_or("0") == "1";
        let remove_touching_contours: bool =
            gs.get("remove_touching_contours").unwrap_or("0") == "1";
        Ok(Self {
            batch: gs.get("batch").unwrap() == "1",
            processes,
            experimental_use_in_memory_fs,
            vegeonly,
            cliffsonly,
            contoursonly,
            pnorthlinesangle,
            pnorthlineswidth,
            lazfolder,
            batchoutfolder,
            savetempfolders,
            savetempfiles,
            scalefactor,
            vege_bitmode,
            zoff,
            thinfactor,
            skipknolldetection,
            vegemode,
            xfactor,
            yfactor,
            zfactor,
            contour_interval,
            basemapcontours,
            detectbuildings,
            water_class,
            inidotknolls,
            smoothing,
            curviness,
            indexcontours,
            formline,
            depression_length,
            c1_limit,
            c2_limit,
            cliff_thin,
            steep_factor,
            flat_place,
            no_small_ciffs,
            zones,
            thresholds,
            greenshades,
            yellowheight,
            yellowthreshold,
            greenground,
            pointvolumefactor,
            pointvolumeexponent,
            greenhigh,
            topweight,
            greentone,
            vegezoffset,
            uglimit,
            uglimit2,
            addition,
            firstandlastreturnasground,
            firstandlastfactor,
            lastfactor,
            yellowfirstlast,
            vegethin,
            greendetectsize,
            proceed_yellows,
            med,
            med2,
            medyellow,
            water,
            buildings,
            waterele,
            buildingcolor,
            vectorconf,
            mtkskiplayers,
            cliffdebug,
            formlinesteepness,
            formlineaddition,
            dashlength,
            gaplength,
            minimumgap,
            label_depressions,
            remove_touching_contours,
        })
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use super::Config;

    #[test]
    fn should_load_config_template_successfully() {
        Config::from_file(Path::new("pullauta.default.ini"))
            .expect("Could not load and parse the default config template");
    }
}
