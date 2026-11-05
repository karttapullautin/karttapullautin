//! The coordinate system of the pullautin output to Web Mercator (EPSG:3857), for any projected CRS: an EPSG code (`EPSG:3067`, `3006`, `25832` ...),
//! a PROJ string (`"+proj=utm +zone=33 +ellps=GRS80 +units=m"`), a UTM zone (`utm33n`, `etrs89utm32`) or one of the short names `tm35fin` and `sweref99tm`.
//! The transformation is done by `proj4rs`, a Rust port of PROJ.4 (no C library): the CRS definitions are the ones of its `crs-definitions` database, and a
//! datum shift uses the 3 or 7 parameters of the definition, not grid files (accurate to about a metre for national grids on an old datum, to a millimetre
//! for the ETRS89, SWEREF 99 and WGS 84 based ones, which is far below a pixel of a z14 tile, 9 m).
//! The coordinates of the pullautin output must be in metres: a geographic (longitude/latitude) CRS is refused.

use proj4rs::Proj;

/// The coordinate system of the pullautin output: a definition that `proj4rs` has accepted (`EPSG:3067`, `+proj=...`).
/// The default is ETRS-TM35FIN (Finland).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crs {
    definition: String,
}

impl Default for Crs {
    fn default() -> Self {
        Self {
            definition: "EPSG:3067".to_string(),
        }
    }
}

/// UTM zone names: `utm33n`, `utm33s`, `wgs84utm33n` (WGS 84, EPSG:326xx / 327xx) and `etrs89utm32`, `etrs89utm32n` (EPSG:258xx, zones 28-38)
fn utm_epsg(name: &str) -> Option<u16> {
    let (etrs, rest) = match name.strip_prefix("etrs89utm") {
        Some(rest) => (true, rest),
        None => (
            false,
            name.strip_prefix("wgs84utm")
                .or_else(|| name.strip_prefix("utm"))?,
        ),
    };
    let south = rest.ends_with('s');
    let digits = rest.trim_end_matches(['n', 's']);
    let zone: u16 = digits.parse().ok().filter(|z| (1..=60).contains(z))?;
    if etrs {
        (!south && (28..=38).contains(&zone)).then_some(25800 + zone)
    } else {
        Some(if south { 32700 } else { 32600 } + zone)
    }
}

impl Crs {
    /// Builds the CRS from a name (case, spaces, `-` and `_` do not matter): `tm35fin`, `sweref99tm`, `utm33n`, `etrs89utm32`, an EPSG code with or without
    /// the `EPSG:` prefix, or a PROJ string that starts with `+`. The error says what was wrong.
    pub fn parse(name: &str) -> Result<Self, String> {
        let name = name.trim();
        let definition = if name.starts_with('+') {
            name.to_string()
        } else {
            let key = name.to_ascii_lowercase().replace(['-', '_', ' '], "");
            let code = match key.as_str() {
                "tm35fin" | "etrstm35fin" => Some(3067),
                "sweref99tm" | "sweref99" => Some(3006),
                "wgs84" => Some(4326), // refused below with the reason (geographic)
                _ => utm_epsg(&key)
                    .or_else(|| key.strip_prefix("epsg:").unwrap_or(&key).parse().ok()),
            };
            match code {
                Some(code) => format!("EPSG:{code}"),
                None => {
                    return Err(format!(
                        "unknown coordinate system \"{name}\": use an EPSG code (3067, EPSG:25832), a UTM zone (utm33n, etrs89utm32), tm35fin, sweref99tm or a PROJ string (\"+proj=...\")"
                    ));
                }
            }
        };
        let proj = Proj::from_user_string(&definition)
            .map_err(|e| format!("coordinate system \"{name}\" ({definition}): {e}"))?;
        if proj.is_latlong() {
            return Err(format!(
                "coordinate system \"{name}\" is geographic: the pullautin output must be in a projected CRS in metres"
            ));
        }
        Ok(Self { definition })
    }

    /// The definition (`EPSG:3067` or the PROJ string) as it is passed to `proj4rs`
    pub fn definition(&self) -> &str {
        &self.definition
    }

    pub fn projection(&self) -> Projection {
        Projection::new(self)
    }
}

/// The transformation from the CRS of the pullautin output to Web Mercator metres
pub struct Projection {
    source: Proj,
    mercator: Proj,
}

impl Default for Projection {
    /// ETRS-TM35FIN, the default CRS
    fn default() -> Self {
        Crs::default().projection()
    }
}

impl Projection {
    fn new(crs: &Crs) -> Self {
        Self {
            // the definition was accepted by Crs::parse; the default is a known EPSG code
            source: Proj::from_user_string(&crs.definition).expect("a CRS that parse accepted"),
            mercator: Proj::from_epsg_code(3857).expect("EPSG:3857 is in the definitions"),
        }
    }

    /// Web Mercator metres; (NaN, NaN) for a point the projection cannot handle
    pub fn to_mercator(&self, easting: f64, northing: f64) -> (f64, f64) {
        let mut point = (easting, northing);
        match proj4rs::transform::transform(&self.source, &self.mercator, &mut point) {
            Ok(()) => point,
            Err(_) => (f64::NAN, f64::NAN),
        }
    }
}

/// Half the width of the Web Mercator world in metres.
pub const MERCATOR_ORIGIN: f64 = 20_037_508.342_789_244;

#[cfg(test)]
mod tests {
    use super::*;

    // (easting, northing, x, y) from pyproj: Transformer.from_crs(code, "EPSG:3857", always_xy=True)
    const TM35FIN: [(f64, f64, f64, f64); 7] = [
        (325312.80, 6811892.80, 2641372.269635, 8718605.957155),
        (320000.00, 6807000.00, 2630842.175192, 8707852.144477),
        (326000.00, 6813000.00, 2642687.205665, 8720985.957425),
        (500000.00, 7000000.00, 3005626.251418, 9132035.784391),
        (233000.50, 6700000.25, 2466759.681381, 8477670.449569),
        (700000.00, 7700000.00, 3571468.514940, 10855579.282223),
        (100000.00, 6650000.00, 2211236.100726, 8353938.132683),
    ];
    const SWEREF99TM: [(f64, f64, f64, f64); 3] = [
        (674032.0, 6580821.0, 2010339.715788, 8252105.501022),
        (890000.0, 7375000.0, 2638348.571816, 9945730.992440),
        (450000.0, 6300000.0, 1578555.370646, 7727714.519710),
    ];

    fn check(crs: &str, points: &[(f64, f64, f64, f64)], tolerance: f64) {
        let p = Crs::parse(crs).unwrap().projection();
        for &(e, n, x, y) in points {
            let (mx, my) = p.to_mercator(e, n);
            assert!(
                (mx - x).abs() < tolerance,
                "{crs} x of ({e}, {n}): {mx} vs {x}"
            );
            assert!(
                (my - y).abs() < tolerance,
                "{crs} y of ({e}, {n}): {my} vs {y}"
            );
        }
    }

    #[test]
    fn the_default_is_tm35fin_and_matches_proj_within_a_millimetre() {
        assert_eq!(Crs::default(), Crs::parse("tm35fin").unwrap());
        let p = Crs::default().projection();
        for (e, n, x, y) in TM35FIN {
            let (mx, my) = p.to_mercator(e, n);
            assert!((mx - x).abs() < 1e-3, "x of ({e}, {n}): {mx} vs {x}");
            assert!((my - y).abs() < 1e-3, "y of ({e}, {n}): {my} vs {y}");
        }
    }

    #[test]
    fn sweref99tm_matches_proj_within_a_millimetre() {
        check("sweref99tm", &SWEREF99TM, 1e-3);
    }

    #[test]
    fn any_epsg_code_and_proj_string_works() {
        // UTM zones on ETRS89 / WGS 84, a southern zone, and a PROJ string of the same CRS as EPSG:32633
        check(
            "EPSG:25832",
            &[
                (500000.0, 6000000.0, 1001875.417139, 7198255.485510),
                (380000.0, 5300000.0, 823340.114513, 6080632.391906),
            ],
            1e-3,
        );
        check(
            "32633",
            &[
                (500000.0, 7000000.0, 1669792.361899, 9132035.784123),
                (350000.0, 6500000.0, 1382315.397327, 8097500.676365),
            ],
            1e-3,
        );
        check(
            "epsg:32735",
            &[(500000.0, 6000000.0, 3005626.251418, -4320552.674095)],
            1e-3,
        );
        check(
            "+proj=utm +zone=33 +datum=WGS84 +units=m +no_defs",
            &[(500000.0, 7000000.0, 1669792.361899, 9132035.784123)],
            1e-3,
        );
        // LAEA Europe (3035), no datum shift
        check(
            "EPSG:3035",
            &[(4321000.0, 3210000.0, 1113194.907933, 6800125.454397)],
            1e-3,
        );
        // national grids on an old datum: the definition's 7 parameters, not the grid files of PROJ (a metre or two)
        check(
            "28992",
            &[
                (155000.0, 463000.0, 599700.751356, 6828231.372688),
                (120000.0, 487000.0, 542491.936808, 6867261.547180),
            ],
            3.0,
        );
        check(
            "EPSG:27700",
            &[(530000.0, 180000.0, -14288.295295, 6710932.763392)],
            6.0,
        );
        check(
            "EPSG:2056",
            &[(2600000.0, 1200000.0, 828064.773290, 5934093.187678)],
            3.0,
        );
    }

    #[test]
    fn crs_names_are_parsed() {
        let sweref = Crs::parse("sweref99tm").unwrap();
        for name in [
            "SWEREF99TM",
            "SWEREF 99 TM",
            "sweref-99-tm",
            "3006",
            "EPSG:3006",
            "epsg:3006",
            " 3006 ",
        ] {
            assert_eq!(Crs::parse(name), Ok(sweref.clone()), "{name}");
        }
        assert_eq!(Crs::parse("ETRS-TM35FIN"), Ok(Crs::default()));
        assert_eq!(Crs::parse("utm33n").unwrap().definition(), "EPSG:32633");
        assert_eq!(
            Crs::parse("WGS84 UTM 35 S").unwrap().definition(),
            "EPSG:32735"
        );
        assert_eq!(
            Crs::parse("etrs89utm32").unwrap().definition(),
            "EPSG:25832"
        );
        assert_eq!(
            Crs::parse("+proj=utm +zone=33 +ellps=GRS80 +units=m")
                .unwrap()
                .definition(),
            "+proj=utm +zone=33 +ellps=GRS80 +units=m"
        );
    }

    #[test]
    fn what_is_not_a_projected_crs_is_refused_with_a_reason() {
        for name in ["wgs84", "4326", "EPSG:4258", "+proj=longlat +datum=WGS84"] {
            let error = Crs::parse(name).unwrap_err();
            assert!(error.contains("geographic"), "{name}: {error}");
        }
        for name in [
            "",
            "nowhere",
            "utm99n",
            "etrs89utm45",
            "EPSG:99999999",
            "+proj=nonsense",
        ] {
            assert!(Crs::parse(name).is_err(), "{name}");
        }
        assert!(Crs::parse("moon").unwrap_err().contains("EPSG code"));
    }

    #[test]
    fn a_point_the_projection_cannot_handle_is_nan_not_a_panic() {
        let (x, y) = Crs::default().projection().to_mercator(f64::NAN, 0.0);
        assert!(x.is_nan() && y.is_nan());
    }
}
