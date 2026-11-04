//! Mapbox Vector Tile encoder (protobuf written by hand, spec 2.1).
//!
//! Geometry is in integer tile coordinates, y down, extent 4096. A polygon's rings are open (the first point is not repeated), the
//! exterior ring first with a positive area by the surveyor's formula (clockwise on screen), the holes after it with a negative one.

use std::collections::HashMap;

pub const EXTENT: u32 = 4096;

pub type Point = (i32, i32);

#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point(Point),
    Line(Vec<Point>),
    Polygon(Vec<Vec<Point>>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Double(f64),
    Int(i64),
}

#[derive(Clone, Debug)]
pub struct Feature {
    pub geometry: Geometry,
    pub properties: Vec<(&'static str, Value)>,
}

/// Twice the signed area of a ring by the surveyor's formula (positive = exterior in MVT, y down).
pub fn ring_area2(ring: &[Point]) -> i64 {
    let mut sum = 0i64;
    for i in 0..ring.len() {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % ring.len()];
        sum += x0 as i64 * y1 as i64 - x1 as i64 * y0 as i64;
    }
    sum
}

fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

fn write_key(buf: &mut Vec<u8>, field: u32, wire_type: u8) {
    write_varint(buf, ((field as u64) << 3) | wire_type as u64);
}

fn write_bytes(buf: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    write_key(buf, field, 2);
    write_varint(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn encode_value(value: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    match value {
        Value::Str(s) => write_bytes(&mut buf, 1, s.as_bytes()),
        Value::Double(d) => {
            write_key(&mut buf, 3, 1);
            buf.extend_from_slice(&d.to_le_bytes());
        }
        Value::Int(i) => {
            write_key(&mut buf, 4, 0);
            write_varint(&mut buf, *i as u64);
        }
    }
    buf
}

const MOVE_TO: u32 = 1;
const LINE_TO: u32 = 2;
const CLOSE_PATH: u32 = 7;

fn command(id: u32, count: u32) -> u32 {
    (id & 0x7) | (count << 3)
}

/// (geometry type, command integers)
fn encode_geometry(geometry: &Geometry) -> (u64, Vec<u32>) {
    let mut out = Vec::new();
    let mut cursor = (0i32, 0i32);
    let mut point = |out: &mut Vec<u32>, p: Point| {
        out.push(zigzag(p.0 - cursor.0));
        out.push(zigzag(p.1 - cursor.1));
        cursor = p;
    };
    match geometry {
        Geometry::Point(p) => {
            out.push(command(MOVE_TO, 1));
            point(&mut out, *p);
            (1, out)
        }
        Geometry::Line(points) => {
            out.push(command(MOVE_TO, 1));
            point(&mut out, points[0]);
            out.push(command(LINE_TO, points.len() as u32 - 1));
            for p in &points[1..] {
                point(&mut out, *p);
            }
            (2, out)
        }
        Geometry::Polygon(rings) => {
            for ring in rings {
                out.push(command(MOVE_TO, 1));
                point(&mut out, ring[0]);
                out.push(command(LINE_TO, ring.len() as u32 - 1));
                for p in &ring[1..] {
                    point(&mut out, *p);
                }
                out.push(command(CLOSE_PATH, 1));
            }
            (3, out)
        }
    }
}

/// One layer of a tile: collects features, deduplicating keys and values.
pub struct Layer {
    name: String,
    keys: Vec<&'static str>,
    key_index: HashMap<&'static str, u32>,
    values: Vec<Vec<u8>>,
    value_index: HashMap<Vec<u8>, u32>,
    features: Vec<Vec<u8>>,
}

impl Layer {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            keys: Vec::new(),
            key_index: HashMap::new(),
            values: Vec::new(),
            value_index: HashMap::new(),
            features: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    pub fn add(&mut self, feature: &Feature) {
        let mut tags = Vec::new();
        for (key, value) in &feature.properties {
            let k = *self.key_index.entry(key).or_insert_with(|| {
                self.keys.push(key);
                self.keys.len() as u32 - 1
            });
            let encoded = encode_value(value);
            let v = match self.value_index.get(&encoded) {
                Some(&v) => v,
                None => {
                    self.values.push(encoded.clone());
                    let v = self.values.len() as u32 - 1;
                    self.value_index.insert(encoded, v);
                    v
                }
            };
            write_varint(&mut tags, k as u64);
            write_varint(&mut tags, v as u64);
        }

        let (geometry_type, commands) = encode_geometry(&feature.geometry);
        let mut geometry = Vec::new();
        for c in commands {
            write_varint(&mut geometry, c as u64);
        }

        let mut buf = Vec::new();
        if !tags.is_empty() {
            write_bytes(&mut buf, 2, &tags);
        }
        write_key(&mut buf, 3, 0);
        write_varint(&mut buf, geometry_type);
        write_bytes(&mut buf, 4, &geometry);
        self.features.push(buf);
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_bytes(&mut buf, 1, self.name.as_bytes());
        for f in &self.features {
            write_bytes(&mut buf, 2, f);
        }
        for k in &self.keys {
            write_bytes(&mut buf, 3, k.as_bytes());
        }
        for v in &self.values {
            write_bytes(&mut buf, 4, v);
        }
        write_key(&mut buf, 5, 0);
        write_varint(&mut buf, EXTENT as u64);
        write_key(&mut buf, 15, 0);
        write_varint(&mut buf, 2);
        buf
    }
}

/// The scan years of a tile as one tiny empty layer, 8 bytes: the **name** of the layer is two characters that hold the first and last year of
/// the laser scanning the tile's data comes from, `'0' + (first - 2000)` and `'0' + (last - first)` (2000-2063, a span of up to 63 years: printable
/// ASCII), and the layer has the version field only. Two characters are what no real layer name has (`contour`, `vegetation` ...), a reader finds
/// the layer by that, and a reader that does not know it (OpenLayers' MVT format with a `layers` list, mapbox-vector-tile) takes it for an empty layer.
/// Tiles of two archives merged into one by the tile server keep both layers (different names are not merged); the reader takes the union.
/// Returns the bytes to append to a tile (the layer with its field tag and length), None for years the encoding does not hold.
pub fn scan_years_layer(first: u16, last: u16) -> Option<Vec<u8>> {
    if !(2000..=2063).contains(&first) || last < first || last - first > 63 {
        return None;
    }
    let mut layer = Vec::new();
    write_bytes(
        &mut layer,
        1,
        &[b'0' + (first - 2000) as u8, b'0' + (last - first) as u8],
    );
    layer.extend_from_slice(&[15 << 3, 2]); // version 2
    let mut out = Vec::new();
    write_bytes(&mut out, 3, &layer);
    Some(out)
}

/// The years of a layer name made by `scan_years_layer`
pub fn scan_years_of_layer_name(name: &[u8]) -> Option<(u16, u16)> {
    match name {
        [a, b] if (b'0'..=b'o').contains(a) && (b'0'..=b'o').contains(b) => {
            let first = 2000 + (a - b'0') as u16;
            Some((first, first + (b - b'0') as u16))
        }
        _ => None,
    }
}

/// The bytes of a tile (not compressed). Empty layers are left out.
pub fn encode_tile(layers: &[Layer]) -> Vec<u8> {
    let mut buf = Vec::new();
    for layer in layers.iter().filter(|l| !l.is_empty()) {
        write_bytes(&mut buf, 3, &layer.encode());
    }
    buf
}

#[cfg(test)]
pub mod decode {
    //! Minimal decoder for the tests.
    use super::*;

    pub struct Reader<'a> {
        pub data: &'a [u8],
        pub pos: usize,
    }

    impl<'a> Reader<'a> {
        pub fn new(data: &'a [u8]) -> Self {
            Self { data, pos: 0 }
        }
        pub fn done(&self) -> bool {
            self.pos >= self.data.len()
        }
        pub fn varint(&mut self) -> u64 {
            let mut v = 0u64;
            let mut shift = 0;
            loop {
                let b = self.data[self.pos];
                self.pos += 1;
                v |= ((b & 0x7F) as u64) << shift;
                if b & 0x80 == 0 {
                    return v;
                }
                shift += 7;
            }
        }
        pub fn bytes(&mut self) -> &'a [u8] {
            let n = self.varint() as usize;
            let s = &self.data[self.pos..self.pos + n];
            self.pos += n;
            s
        }
        pub fn key(&mut self) -> (u32, u8) {
            let k = self.varint();
            ((k >> 3) as u32, (k & 7) as u8)
        }
        pub fn fixed64(&mut self) -> [u8; 8] {
            let s = self.data[self.pos..self.pos + 8].try_into().unwrap();
            self.pos += 8;
            s
        }
    }

    #[derive(Debug)]
    pub struct DecodedLayer {
        pub name: String,
        pub extent: u32,
        pub features: Vec<(Vec<(String, Value)>, Geometry)>,
    }

    fn unzigzag(v: u32) -> i32 {
        ((v >> 1) as i32) ^ -((v & 1) as i32)
    }

    fn decode_value(b: &[u8]) -> Value {
        let mut r = Reader::new(b);
        let (field, _) = r.key();
        match field {
            1 => Value::Str(String::from_utf8(r.bytes().to_vec()).unwrap()),
            3 => Value::Double(f64::from_le_bytes(r.fixed64())),
            4 => Value::Int(r.varint() as i64),
            other => panic!("value field {other}"),
        }
    }

    fn decode_geometry(kind: u64, commands: &[u32]) -> Geometry {
        let mut rings: Vec<Vec<Point>> = Vec::new();
        let mut cur: Vec<Point> = Vec::new();
        let mut cursor = (0i32, 0i32);
        let mut i = 0;
        while i < commands.len() {
            let id = commands[i] & 7;
            let count = commands[i] >> 3;
            i += 1;
            match id {
                1 | 2 => {
                    for _ in 0..count {
                        cursor = (
                            cursor.0 + unzigzag(commands[i]),
                            cursor.1 + unzigzag(commands[i + 1]),
                        );
                        i += 2;
                        if id == 1 && !cur.is_empty() {
                            rings.push(std::mem::take(&mut cur));
                        }
                        cur.push(cursor);
                    }
                }
                7 => rings.push(std::mem::take(&mut cur)),
                other => panic!("command {other}"),
            }
        }
        if !cur.is_empty() {
            rings.push(cur);
        }
        match kind {
            1 => Geometry::Point(rings[0][0]),
            2 => Geometry::Line(rings.into_iter().next().unwrap()),
            _ => Geometry::Polygon(rings),
        }
    }

    pub fn decode_tile(data: &[u8]) -> Vec<DecodedLayer> {
        let mut layers = Vec::new();
        let mut r = Reader::new(data);
        while !r.done() {
            let (field, _) = r.key();
            assert_eq!(field, 3);
            let layer_bytes = r.bytes();
            let mut l = Reader::new(layer_bytes);
            let (mut name, mut extent) = (String::new(), 0);
            let (mut keys, mut values, mut raw_features) = (Vec::new(), Vec::new(), Vec::new());
            while !l.done() {
                let (field, wire) = l.key();
                match (field, wire) {
                    (1, 2) => name = String::from_utf8(l.bytes().to_vec()).unwrap(),
                    (2, 2) => raw_features.push(l.bytes()),
                    (3, 2) => keys.push(String::from_utf8(l.bytes().to_vec()).unwrap()),
                    (4, 2) => values.push(decode_value(l.bytes())),
                    (5, 0) => extent = l.varint() as u32,
                    (15, 0) => {
                        l.varint();
                    }
                    other => panic!("layer field {other:?}"),
                }
            }
            let features = raw_features
                .into_iter()
                .map(|fb| {
                    let mut f = Reader::new(fb);
                    let (mut tags, mut kind, mut commands) = (Vec::new(), 0, Vec::new());
                    while !f.done() {
                        let (field, wire) = f.key();
                        match (field, wire) {
                            (2, 2) => {
                                let mut t = Reader::new(f.bytes());
                                while !t.done() {
                                    tags.push(t.varint() as usize);
                                }
                            }
                            (3, 0) => kind = f.varint(),
                            (4, 2) => {
                                let mut g = Reader::new(f.bytes());
                                while !g.done() {
                                    commands.push(g.varint() as u32);
                                }
                            }
                            other => panic!("feature field {other:?}"),
                        }
                    }
                    let props = tags
                        .chunks(2)
                        .map(|kv| (keys[kv[0]].clone(), values[kv[1]].clone()))
                        .collect();
                    (props, decode_geometry(kind, &commands))
                })
                .collect();
            layers.push(DecodedLayer {
                name,
                extent,
                features,
            });
        }
        layers
    }
}

#[cfg(test)]
mod tests {
    use super::decode::decode_tile;
    use super::*;

    #[test]
    fn round_trips_points_lines_and_polygons_with_holes() {
        let mut contours = Layer::new("contour");
        let line = vec![(0, 0), (100, 50), (-20, 4000), (4095, 4095)];
        contours.add(&Feature {
            geometry: Geometry::Line(line.clone()),
            properties: vec![
                ("kind", Value::Str("index".into())),
                ("ele", Value::Double(87.5)),
            ],
        });
        let mut veg = Layer::new("vegetation");
        let polygon = vec![
            vec![(10, 10), (500, 10), (500, 500), (10, 500)],
            vec![(100, 100), (100, 200), (200, 200), (200, 100)],
        ];
        veg.add(&Feature {
            geometry: Geometry::Polygon(polygon.clone()),
            properties: vec![
                ("kind", Value::Str("green".into())),
                ("level", Value::Int(3)),
            ],
        });
        let mut knoll = Layer::new("knoll");
        knoll.add(&Feature {
            geometry: Geometry::Point((2048, 17)),
            properties: vec![("kind", Value::Str("dotknoll".into()))],
        });
        let empty = Layer::new("cliff");

        let tile = encode_tile(&[contours, veg, knoll, empty]);
        let layers = decode_tile(&tile);
        assert_eq!(
            layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["contour", "vegetation", "knoll"]
        );
        assert!(layers.iter().all(|l| l.extent == EXTENT));
        assert_eq!(layers[0].features[0].1, Geometry::Line(line));
        assert_eq!(
            layers[0].features[0].0,
            vec![
                ("kind".to_string(), Value::Str("index".into())),
                ("ele".to_string(), Value::Double(87.5))
            ]
        );
        assert_eq!(layers[1].features[0].1, Geometry::Polygon(polygon));
        assert_eq!(
            layers[1].features[0].0[1],
            ("level".to_string(), Value::Int(3))
        );
        assert_eq!(layers[2].features[0].1, Geometry::Point((2048, 17)));
    }

    #[test]
    fn keys_and_values_are_shared_between_features() {
        let mut layer = Layer::new("contour");
        for i in 0..3 {
            layer.add(&Feature {
                geometry: Geometry::Line(vec![(0, i), (10, 10)]),
                properties: vec![
                    ("kind", Value::Str("normal".into())),
                    ("ele", Value::Double(100.0)),
                ],
            });
        }
        let tile = encode_tile(&[layer]);
        // one key table entry per key and one value per distinct value: the tile holds "kind" and "ele" once
        let text = String::from_utf8_lossy(&tile);
        assert_eq!(text.matches("kind").count(), 1);
        assert_eq!(text.matches("normal").count(), 1);
    }

    #[test]
    fn a_polygon_ring_area_is_positive_for_the_exterior_in_y_down_coordinates() {
        // clockwise on screen (y down): right, down, left
        assert!(ring_area2(&[(0, 0), (10, 0), (10, 10), (0, 10)]) > 0);
        assert!(ring_area2(&[(0, 0), (0, 10), (10, 10), (10, 0)]) < 0);
    }
}

#[cfg(test)]
mod scan_years_tests {
    use super::*;

    #[test]
    fn the_scan_years_layer_is_under_10_bytes_and_decodes() {
        let bytes = scan_years_layer(2022, 2026).unwrap();
        assert!(bytes.len() < 10, "{} bytes", bytes.len());
        assert_eq!(bytes.len(), 8);
        // tile field 3, length 6: name (field 1, 2 bytes), version (field 15) 2
        assert_eq!(bytes[0], 3 << 3 | 2);
        assert_eq!(bytes[1], 6);
        assert_eq!(&bytes[2..4], &[0x0A, 2]);
        assert_eq!(scan_years_of_layer_name(&bytes[4..6]), Some((2022, 2026)));
        assert_eq!(&bytes[6..8], &[15 << 3, 2]);
        assert_eq!(
            scan_years_of_layer_name(&scan_years_layer(2008, 2008).unwrap()[4..6]),
            Some((2008, 2008))
        );
    }

    #[test]
    fn years_the_encoding_does_not_hold_are_left_out() {
        assert!(scan_years_layer(1999, 2000).is_none());
        assert!(scan_years_layer(2064, 2064).is_none());
        assert!(scan_years_layer(2026, 2025).is_none());
        assert!(scan_years_of_layer_name(b"contour").is_none());
        assert!(scan_years_of_layer_name(b"~~").is_none());
    }
}
