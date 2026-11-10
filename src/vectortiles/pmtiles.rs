//! PMTiles v3 writer (https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md): gzip-compressed MVT tiles, clustered.
//!
//! Tile data is streamed to a temporary file next to the output, so a large archive does not have to fit in memory; `finish` builds the
//! directories (root, and leaf directories when the root would not fit in the first 16 kB) and writes the final file.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const HEADER_LEN: usize = 127;
/// the header and the root directory must fit in the first 16 kB
const ROOT_BUDGET: usize = 16_384 - HEADER_LEN;

/// Hilbert-curve tile id of a tile (spec: "Tile IDs")
pub fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let mut acc = 0u64;
    for t_z in 0..z {
        acc += 1u64 << (2 * t_z as u64);
    }
    let n = 1i64 << z;
    // i64 like the reference implementation: the rotation can go negative and the masks use two's complement
    let (mut tx, mut ty) = (x as i64, y as i64);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = (tx & s) > 0;
        let ry = (ty & s) > 0;
        d += (s * s) as u64 * ((3 * rx as u64) ^ ry as u64);
        if !ry {
            if rx {
                tx = s - 1 - tx;
                ty = s - 1 - ty;
            }
            std::mem::swap(&mut tx, &mut ty);
        }
        s /= 2;
    }
    acc + d
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tile_id: u64,
    pub offset: u64,
    pub length: u64,
    /// 0 = the entry points to a leaf directory
    pub run_length: u64,
}

fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

pub fn serialize_directory(entries: &[Entry]) -> Vec<u8> {
    let mut buf = Vec::new();
    write_varint(&mut buf, entries.len() as u64);
    let mut last = 0;
    for e in entries {
        write_varint(&mut buf, e.tile_id - last);
        last = e.tile_id;
    }
    for e in entries {
        write_varint(&mut buf, e.run_length);
    }
    for e in entries {
        write_varint(&mut buf, e.length);
    }
    for (i, e) in entries.iter().enumerate() {
        if i > 0 && e.offset == entries[i - 1].offset + entries[i - 1].length {
            write_varint(&mut buf, 0);
        } else {
            write_varint(&mut buf, e.offset + 1);
        }
    }
    buf
}

pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).expect("write to a Vec");
    encoder.finish().expect("finish a Vec")
}

pub struct Header {
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// degrees
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
    pub center_zoom: u8,
    pub center_lon: f64,
    pub center_lat: f64,
}

/// Root directory (gzip) and leaf directories (gzip, concatenated) for the entries
fn build_directories(entries: &[Entry]) -> (Vec<u8>, Vec<u8>) {
    let root = gzip(&serialize_directory(entries));
    if root.len() <= ROOT_BUDGET {
        return (root, Vec::new());
    }
    let mut leaf_size = 4096usize;
    loop {
        let mut root_entries = Vec::new();
        let mut leaves = Vec::new();
        for chunk in entries.chunks(leaf_size) {
            let leaf = gzip(&serialize_directory(chunk));
            root_entries.push(Entry {
                tile_id: chunk[0].tile_id,
                offset: leaves.len() as u64,
                length: leaf.len() as u64,
                run_length: 0,
            });
            leaves.extend_from_slice(&leaf);
        }
        let root = gzip(&serialize_directory(&root_entries));
        if root.len() <= ROOT_BUDGET {
            return (root, leaves);
        }
        leaf_size = (leaf_size as f64 * 1.2) as usize;
    }
}

pub struct PmtilesWriter {
    path: PathBuf,
    tiles_path: PathBuf,
    tiles: BufWriter<File>,
    entries: Vec<Entry>,
    offset: u64,
    last_id: Option<u64>,
}

impl PmtilesWriter {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let mut tiles_path = path.as_os_str().to_owned();
        tiles_path.push(".tiles.tmp");
        let tiles_path = PathBuf::from(tiles_path);
        Ok(Self {
            path: path.to_path_buf(),
            tiles: BufWriter::new(File::create(&tiles_path)?),
            tiles_path,
            entries: Vec::new(),
            offset: 0,
            last_id: None,
        })
    }

    /// Tiles must come in increasing tile id order (the archive is clustered). `data` is the MVT bytes, not compressed.
    pub fn write_tile(&mut self, tile_id: u64, data: &[u8]) -> std::io::Result<()> {
        assert!(
            self.last_id.is_none_or(|last| tile_id > last),
            "tile ids must increase"
        );
        self.last_id = Some(tile_id);
        let compressed = gzip(data);
        self.tiles.write_all(&compressed)?;
        self.entries.push(Entry {
            tile_id,
            offset: self.offset,
            length: compressed.len() as u64,
            run_length: 1,
        });
        self.offset += compressed.len() as u64;
        Ok(())
    }

    pub fn tile_count(&self) -> usize {
        self.entries.len()
    }

    /// Writes the archive. `metadata_json` is the JSON metadata (it is compressed here).
    pub fn finish(mut self, header: &Header, metadata_json: &str) -> std::io::Result<()> {
        self.tiles.flush()?;
        drop(self.tiles);

        let (root, leaves) = build_directories(&self.entries);
        let metadata = gzip(metadata_json.as_bytes());

        let root_offset = HEADER_LEN as u64;
        let metadata_offset = root_offset + root.len() as u64;
        let leaves_offset = metadata_offset + metadata.len() as u64;
        let tiles_offset = leaves_offset + leaves.len() as u64;
        let tiles_len = self.offset;
        let count = self.entries.len() as u64;

        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(b"PMTiles");
        h.push(3);
        for v in [
            root_offset,
            root.len() as u64,
            metadata_offset,
            metadata.len() as u64,
            leaves_offset,
            leaves.len() as u64,
            tiles_offset,
            tiles_len,
            count, // addressed tiles
            count, // tile entries
            count, // tile contents
        ] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.push(1); // clustered
        h.push(2); // internal compression: gzip
        h.push(2); // tile compression: gzip
        h.push(1); // tile type: MVT
        h.push(header.min_zoom);
        h.push(header.max_zoom);
        for degrees in [
            header.min_lon,
            header.min_lat,
            header.max_lon,
            header.max_lat,
        ] {
            h.extend_from_slice(&((degrees * 1e7).round() as i32).to_le_bytes());
        }
        h.push(header.center_zoom);
        h.extend_from_slice(&((header.center_lon * 1e7).round() as i32).to_le_bytes());
        h.extend_from_slice(&((header.center_lat * 1e7).round() as i32).to_le_bytes());
        debug_assert_eq!(h.len(), HEADER_LEN);

        let mut out = BufWriter::new(File::create(&self.path)?);
        out.write_all(&h)?;
        out.write_all(&root)?;
        out.write_all(&metadata)?;
        out.write_all(&leaves)?;
        let mut tiles = File::open(&self.tiles_path)?;
        std::io::copy(&mut tiles, &mut out)?;
        out.flush()?;
        std::fs::remove_file(&self.tiles_path)?;
        Ok(())
    }
}

/// Reads one tile back (used by the tests and the checks of the tile builder).
pub fn read_tile(path: &Path, tile: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let mut all = Vec::new();
    file.read_to_end(&mut all)?;
    let u64_at = |o: usize| u64::from_le_bytes(all[o..o + 8].try_into().unwrap());
    let (root_off, root_len) = (u64_at(8) as usize, u64_at(16) as usize);
    let (leaves_off, tiles_off) = (u64_at(40) as usize, u64_at(56) as usize);

    let gunzip = |bytes: &[u8]| {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut out)
            .map(|_| out)
    };
    let mut dir = deserialize_directory(&gunzip(&all[root_off..root_off + root_len])?);
    loop {
        let Some(i) = dir.iter().rposition(|e| e.tile_id <= tile) else {
            return Ok(None);
        };
        let e = dir[i].clone();
        if e.run_length > 0 {
            if tile >= e.tile_id + e.run_length {
                return Ok(None);
            }
            let start = tiles_off + e.offset as usize;
            return Ok(Some(gunzip(&all[start..start + e.length as usize])?));
        }
        let start = leaves_off + e.offset as usize;
        dir = deserialize_directory(&gunzip(&all[start..start + e.length as usize])?);
    }
}

fn deserialize_directory(buf: &[u8]) -> Vec<Entry> {
    let mut pos = 0;
    let mut varint = || {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = buf[pos];
            pos += 1;
            v |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    };
    let n = varint() as usize;
    let mut entries = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0
        };
        n
    ];
    let mut last = 0;
    for e in entries.iter_mut() {
        last += varint();
        e.tile_id = last;
    }
    for e in entries.iter_mut() {
        e.run_length = varint();
    }
    for e in entries.iter_mut() {
        e.length = varint();
    }
    for i in 0..n {
        let v = varint();
        entries[i].offset = if v == 0 && i > 0 {
            entries[i - 1].offset + entries[i - 1].length
        } else {
            v - 1
        };
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_match_the_reference_implementation() {
        // values of the Python pmtiles library (pmtiles.tile.zxy_to_tileid)
        for (z, x, y, id) in [
            (0, 0, 0, 0),
            (1, 0, 0, 1),
            (1, 0, 1, 2),
            (1, 1, 1, 3),
            (1, 1, 0, 4),
            (2, 0, 0, 5),
            (14, 9271, 4627, 319380357),
            (14, 9267, 4623, 319380303),
            (14, 9275, 4631, 319380421),
            (14, 0, 0, 89478485),
            (14, 16383, 16383, 268435455),
            (8, 100, 200, 50117),
            (20, 500000, 300000, 527794063359),
        ] {
            assert_eq!(tile_id(z, x, y), id, "z{z} x{x} y{y}");
        }
    }

    #[test]
    fn writes_and_reads_back_with_and_without_leaf_directories() {
        let dir = std::env::temp_dir().join(format!("pmtiles_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let header = Header {
            min_zoom: 14,
            max_zoom: 14,
            min_lon: 23.6,
            min_lat: 61.3,
            max_lon: 23.8,
            max_lat: 61.5,
            center_zoom: 14,
            center_lon: 23.7,
            center_lat: 61.4,
        };
        for (name, n) in [("small", 50u32), ("large", 150_000)] {
            let path = dir.join(format!("{name}.pmtiles"));
            let mut w = PmtilesWriter::create(&path).unwrap();
            let mut ids: Vec<(u64, u32)> = (0..n)
                .map(|i| (tile_id(14, 9000 + i % 200, 4000 + i / 200), i))
                .collect();
            ids.sort();
            for (id, i) in &ids {
                w.write_tile(
                    *id,
                    format!(
                        "tile {i} {}",
                        "x".repeat(((*i as u64 * 2_654_435_761) % 997) as usize)
                    )
                    .as_bytes(),
                )
                .unwrap();
            }
            w.finish(&header, "{\"name\":\"test\"}").unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[..7], b"PMTiles");
            assert_eq!(bytes[7], 3);
            let leaves_len = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
            assert_eq!(leaves_len > 0, name == "large", "{name}: leaf directories");
            for (id, i) in ids.iter().step_by(97) {
                let data = read_tile(&path, *id).unwrap().expect("tile present");
                assert_eq!(
                    data,
                    format!(
                        "tile {i} {}",
                        "x".repeat(((*i as u64 * 2_654_435_761) % 997) as usize)
                    )
                    .as_bytes()
                );
            }
            assert_eq!(read_tile(&path, tile_id(14, 1, 1)).unwrap(), None);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
