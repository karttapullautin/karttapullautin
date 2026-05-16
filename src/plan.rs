//! This module contains logic for planning the processing execution.

use std::path::PathBuf;

use anyhow::Context;
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use crate::io::fs::FileSystem;

pub struct Plan {
    input_files: Vec<InputFile>,
    files_to_process: Vec<InputFileIndex>,
    mapping: HashMap<InputFileIndex, HashSet<InputFileIndex>>,
}

pub struct InputFile {
    /// Path of the input LAS/LAZ file
    pub path: PathBuf,
    /// Path of the output PNG
    pub output_path: PathBuf,

    /// Path of the temporary .xyz.bin file to store points in
    pub staging_path: PathBuf,

    /// The header read from the input file
    pub header: Header,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct InputFileIndex(usize);

impl Plan {
    pub fn new_from_input_files<F: FileSystem + Send + Clone + 'static>(
        fs: F,
        input_folder: &str,
        output_folder: &str,
        staging_folder: &str,
        padding: f64,
    ) -> anyhow::Result<Self> {
        // list all the files that we have to process
        let mut laz_files: Vec<PathBuf> = Vec::new();
        for path in fs.list(input_folder).context("listing input files")? {
            if let Some(extension) = path.extension()
                && (extension == "laz" || extension == "las")
            {
                laz_files.push(path);
            }
        }

        // sort for deterministic processing order (not strictly needed, but makes it easier to debug and test)
        laz_files.sort();

        let mut input_files: Vec<InputFile> = Vec::with_capacity(laz_files.len());
        for path in laz_files {
            let output_path = PathBuf::from(output_folder)
                .join(path.file_name().context("input filename")?)
                .with_extension("png");

            let staging_path = PathBuf::from(staging_folder)
                .join(path.file_name().context("input filename")?)
                .with_extension("xyz.bin");

            let mut file = fs.open(&path).context("opening input file")?;
            let header =
                las::raw::Header::read_from(&mut file).context("reading input file header")?;
            let header: Header = header.into();
            log::debug!(
                "Reading file header: {:?} -> {:?}",
                path.file_name(),
                header
            );

            input_files.push(InputFile {
                path,
                output_path,
                staging_path,
                header,
            })
        }

        let mut mapping = mapping_from_headers(&input_files, padding);

        // now check if their corresponding output files already exist, and if so, skip them
        let mut files_to_process: Vec<InputFileIndex> = Vec::with_capacity(input_files.len());
        for (i, input_file) in input_files.iter().enumerate() {
            if !fs.exists(&input_file.output_path) {
                files_to_process.push(InputFileIndex(i));
            } else {
                // drop it from the mapping, so that we don't process it or its dependencies when not needed
                mapping.remove(&InputFileIndex(i));
            }
        }

        log::info!(
            "Found {} input files, {} of which need to be processed",
            input_files.len(),
            files_to_process.len()
        );

        Ok(Self {
            input_files,
            files_to_process,
            mapping,
        })
    }

    pub fn get_input_file(&self, index: InputFileIndex) -> &InputFile {
        &self.input_files[index.0]
    }

    pub fn input_files(&self) -> &[InputFile] {
        &self.input_files
    }

    pub fn files_to_process(&self) -> &[InputFileIndex] {
        &self.files_to_process
    }

    pub fn naive_planner(&self) -> Box<dyn Planner> {
        Box::new(NaivePlanner::new(self.mapping.clone()))
    }
}

/// Create a mapping of which files are dependent on which other files, based on their headers and a padding value.
fn mapping_from_headers(
    headers: &[InputFile],
    padding: f64,
) -> HashMap<InputFileIndex, HashSet<InputFileIndex>> {
    let mut mapping: HashMap<InputFileIndex, HashSet<InputFileIndex>> = HashMap::default();

    // this is now O(n^2) but we expect n to be small, and it is simpler to implement than a spatial index
    for (i, ih) in headers.iter().enumerate() {
        let padded_bounds = ih.header.bounds.expand(padding);

        let e = mapping.entry(InputFileIndex(i)).or_default();

        for (j, jh) in headers.iter().enumerate() {
            // skip self
            if i == j {
                continue;
            }

            // if overlap, there is a dependency (in both directions!)
            if jh.header.bounds.overlaps(&padded_bounds) {
                e.insert(InputFileIndex(j));
            }
        }
    }
    mapping
}

/// Represents the header of the input file.
#[derive(Debug)]
pub struct Header {
    pub bounds: Rect,
    #[allow(unused)]
    pub n_points: u32,
}

impl From<las::raw::Header> for Header {
    fn from(value: las::raw::Header) -> Self {
        Self {
            bounds: Rect::new(value.min_x, value.min_y, value.max_x, value.max_y),
            n_points: value.number_of_point_records,
        }
    }
}

/// Represents an operation that needs to be performed in order to process the input files. This is
/// generated by the `Planner` and executed by the `Performer`.
#[derive(Debug)]
pub enum Operation {
    /// Load and extract a file into the file(s) specified.
    /// The performer is required to respect the padding value (global),
    /// and put all points that are within the padding area into the respective
    /// destination files.
    Extract {
        from: InputFileIndex,
        to: Vec<InputFileIndex>,
    },
    /// This indicates that the tile can be processed, eg. it has been filled with all points
    /// from neighboring tiles (using `Extract` operations).
    Process { tile: InputFileIndex },
}

pub trait Planner {
    /// Get the next operation(s) to perform, if any. If it returns `None`, the processing is done.
    fn next_operation(&mut self) -> Option<Vec<Operation>>;
}

pub struct NaivePlanner {
    mapping: HashMap<InputFileIndex, HashSet<InputFileIndex>>,
}

impl NaivePlanner {
    fn new(mapping: HashMap<InputFileIndex, HashSet<InputFileIndex>>) -> Self {
        Self { mapping }
    }
}
impl Planner for NaivePlanner {
    fn next_operation(&mut self) -> Option<Vec<Operation>> {
        // take the first key and process it
        let Some(&i) = self.mapping.keys().next() else {
            return None;
        };

        // construct operations to perform:
        let mut ops = Vec::new();

        // we need to extract from the tile itself to the tile
        ops.push(Operation::Extract {
            from: i,
            to: vec![i],
        });

        // for each of our dependencies, extract from it to the tile itself (but not to any other
        // tiles that also depend on it, which is what the `ExtractOncePlanner` does)
        if let Some(deps) = self.mapping.remove(&i) {
            for dep in deps {
                ops.push(Operation::Extract {
                    from: dep,
                    to: vec![i],
                });
            }
        }

        // now we can process the tile i
        ops.push(Operation::Process { tile: i });

        Some(ops)
    }
}
/// A simple rectangle struct to represent a rectangle in 2D space.
#[derive(Debug, Clone)]
pub struct Rect {
    pub minx: f64,
    pub miny: f64,
    pub maxx: f64,
    pub maxy: f64,
}

impl Rect {
    pub fn new(minx: f64, miny: f64, maxx: f64, maxy: f64) -> Self {
        Self {
            minx,
            miny,
            maxx,
            maxy,
        }
    }

    /// Expand the rectangle by a given padding value in all directions.
    pub fn expand(&self, padding: f64) -> Self {
        Self {
            minx: self.minx - padding,
            miny: self.miny - padding,
            maxx: self.maxx + padding,
            maxy: self.maxy + padding,
        }
    }

    /// Check if this rectangle overlaps with another rectangle.
    pub fn overlaps(&self, other: &Rect) -> bool {
        other.maxx > self.minx
            && other.minx < self.maxx
            && other.maxy > self.miny
            && other.miny < self.maxy
    }

    /// Check if point is within the rectangle (excluding the boundary).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x > self.minx && x < self.maxx && y > self.miny && y < self.maxy
    }
}
