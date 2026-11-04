//! ETRS-TM35FIN (EPSG:3067) or SWEREF 99 TM (EPSG:3006) to Web Mercator (EPSG:3857). The two differ only in the central meridian
//! (27 and 15 degrees east); both use the GRS80 ellipsoid, scale 0.9996 and false easting 500 000 m.
//!
//! The inverse transverse Mercator is the Krüger n-series of Karney (2011), to n^5, on the GRS80 ellipsoid; ETRS89 and WGS84 are
//! taken as the same datum, as proj does. The error is far below a millimetre inside Finland and Sweden.

use std::f64::consts::PI;

const A: f64 = 6_378_137.0;
const F: f64 = 1.0 / 298.257_222_101;
const K0: f64 = 0.9996;
const FALSE_EASTING: f64 = 500_000.0;
const TM35FIN_CENTRAL_MERIDIAN_DEG: f64 = 27.0;
const SWEREF99TM_CENTRAL_MERIDIAN_DEG: f64 = 15.0;

/// The coordinate system of the pullautin output
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Crs {
    /// ETRS-TM35FIN, EPSG:3067 (Finland)
    #[default]
    Tm35fin,
    /// SWEREF 99 TM, EPSG:3006 (Sweden)
    Sweref99Tm,
}

impl Crs {
    pub fn projection(self) -> Tm35finToMercator {
        match self {
            Crs::Tm35fin => Tm35finToMercator::new(),
            Crs::Sweref99Tm => {
                Tm35finToMercator::with_central_meridian(SWEREF99TM_CENTRAL_MERIDIAN_DEG)
            }
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name
            .to_ascii_lowercase()
            .replace(['-', '_', ' '], "")
            .as_str()
        {
            "tm35fin" | "etrstm35fin" | "3067" | "epsg3067" => Some(Crs::Tm35fin),
            "sweref99tm" | "sweref99" | "3006" | "epsg3006" => Some(Crs::Sweref99Tm),
            _ => None,
        }
    }
}

/// Constants of the series, computed once (the name is historical: any GRS80 transverse Mercator zone of scale 0.9996).
pub struct Tm35finToMercator {
    central_meridian_deg: f64,
    /// k0 * rectifying radius
    scale: f64,
    beta: [f64; 5],
    delta: [f64; 5],
}

impl Default for Tm35finToMercator {
    fn default() -> Self {
        Self::new()
    }
}

impl Tm35finToMercator {
    pub fn new() -> Self {
        Self::with_central_meridian(TM35FIN_CENTRAL_MERIDIAN_DEG)
    }

    pub fn with_central_meridian(central_meridian_deg: f64) -> Self {
        let n = F / (2.0 - F);
        let (n2, n3, n4, n5, n6) = (n * n, n.powi(3), n.powi(4), n.powi(5), n.powi(6));
        let rectifying = A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
        let beta = [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0 - 81.0 * n5 / 512.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0 + 46.0 * n5 / 105.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0 - 209.0 * n5 / 4480.0,
            4397.0 * n4 / 161_280.0 - 11.0 * n5 / 504.0,
            4583.0 * n5 / 161_280.0,
        ];
        let delta = [
            2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0 + 26.0 * n5 / 45.0,
            7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0 + 2704.0 * n5 / 315.0,
            56.0 * n3 / 15.0 - 136.0 * n4 / 35.0 - 1262.0 * n5 / 105.0,
            4279.0 * n4 / 630.0 - 332.0 * n5 / 35.0,
            4174.0 * n5 / 315.0,
        ];
        Self {
            central_meridian_deg,
            scale: K0 * rectifying,
            beta,
            delta,
        }
    }

    /// Longitude and latitude in radians.
    pub fn to_lon_lat(&self, easting: f64, northing: f64) -> (f64, f64) {
        let xi = northing / self.scale;
        let eta = (easting - FALSE_EASTING) / self.scale;

        let mut xi_p = xi;
        let mut eta_p = eta;
        for (j, b) in self.beta.iter().enumerate() {
            let k = 2.0 * (j as f64 + 1.0);
            xi_p -= b * (k * xi).sin() * (k * eta).cosh();
            eta_p -= b * (k * xi).cos() * (k * eta).sinh();
        }
        let chi = (xi_p.sin() / eta_p.cosh()).asin();
        let mut lat = chi;
        for (j, d) in self.delta.iter().enumerate() {
            lat += d * (2.0 * (j as f64 + 1.0) * chi).sin();
        }
        let lon = self.central_meridian_deg.to_radians() + eta_p.sinh().atan2(xi_p.cos());
        (lon, lat)
    }

    /// Web Mercator metres.
    pub fn to_mercator(&self, easting: f64, northing: f64) -> (f64, f64) {
        let (lon, lat) = self.to_lon_lat(easting, northing);
        (A * lon, A * (PI / 4.0 + lat / 2.0).tan().ln())
    }
}

/// Half the width of the Web Mercator world in metres.
pub const MERCATOR_ORIGIN: f64 = 20_037_508.342_789_244;

#[cfg(test)]
mod tests {
    use super::*;

    // (easting, northing, x, y) from pyproj: Transformer.from_crs("EPSG:3067", "EPSG:3857", always_xy=True)
    const REFERENCE: [(f64, f64, f64, f64); 7] = [
        (325312.80, 6811892.80, 2641372.269635, 8718605.957155),
        (320000.00, 6807000.00, 2630842.175192, 8707852.144477),
        (326000.00, 6813000.00, 2642687.205665, 8720985.957425),
        (500000.00, 7000000.00, 3005626.251418, 9132035.784391),
        (233000.50, 6700000.25, 2466759.681381, 8477670.449569),
        (700000.00, 7700000.00, 3571468.514940, 10855579.282223),
        (100000.00, 6650000.00, 2211236.100726, 8353938.132683),
    ];

    #[test]
    fn matches_proj_within_a_millimetre() {
        let p = Tm35finToMercator::new();
        for (e, n, x, y) in REFERENCE {
            let (mx, my) = p.to_mercator(e, n);
            assert!((mx - x).abs() < 1e-3, "x of ({e}, {n}): {mx} vs {x}");
            assert!((my - y).abs() < 1e-3, "y of ({e}, {n}): {my} vs {y}");
        }
    }

    // from pyproj: Transformer.from_crs("EPSG:3006", "EPSG:3857", always_xy=True)
    const SWEDEN: [(f64, f64, f64, f64); 3] = [
        (674032.0, 6580821.0, 2010339.715788, 8252105.501022),
        (890000.0, 7375000.0, 2638348.571816, 9945730.992440),
        (450000.0, 6300000.0, 1578555.370646, 7727714.519710),
    ];

    #[test]
    fn sweref99tm_matches_proj_within_a_millimetre() {
        let p = Crs::Sweref99Tm.projection();
        for (e, n, x, y) in SWEDEN {
            let (mx, my) = p.to_mercator(e, n);
            assert!((mx - x).abs() < 1e-3, "x of ({e}, {n}): {mx} vs {x}");
            assert!((my - y).abs() < 1e-3, "y of ({e}, {n}): {my} vs {y}");
        }
    }

    #[test]
    fn crs_names_are_parsed() {
        assert_eq!(Crs::parse("SWEREF99TM"), Some(Crs::Sweref99Tm));
        assert_eq!(
            Crs::parse("epsg:3006".replace(':', "").as_str()),
            Some(Crs::Sweref99Tm)
        );
        assert_eq!(Crs::parse("tm35fin"), Some(Crs::Tm35fin));
        assert_eq!(Crs::parse("wgs84"), None);
    }

    #[test]
    fn the_central_meridian_is_27_degrees() {
        let (lon, _) = Tm35finToMercator::new().to_lon_lat(500_000.0, 6_900_000.0);
        assert!((lon.to_degrees() - 27.0).abs() < 1e-12);
    }
}
