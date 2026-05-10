//! This module contains logic for planning the processing execution.

use std::path::PathBuf;

use anyhow::Context;

use crate::io::fs::FileSystem;

pub struct Plan {
    input_files: Vec<InputFile>,
    files_to_process: Vec<InputFileIndex>,
}

pub struct InputFile {
    pub path: PathBuf,
    pub output_path: PathBuf,
    // TODO: add bounds etc
}

#[derive(Clone, Copy)]
pub struct InputFileIndex(usize);

impl Plan {
    pub fn new_from_input_files<F: FileSystem + Send + Clone + 'static>(
        fs: F,
        input_folder: &str,
        output_folder: &str,
    ) -> anyhow::Result<Self> {
        // list all the files that we have to process
        let mut laz_files: Vec<PathBuf> = Vec::new();
        for path in fs.list(input_folder).context("listing input files")? {
            if let Some(extension) = path.extension() {
                if extension == "laz" || extension == "las" {
                    laz_files.push(path);
                }
            }
        }

        let mut input_files: Vec<InputFile> = Vec::with_capacity(laz_files.len());
        for path in laz_files {
            let output_path = PathBuf::from(output_folder)
                .join(path.file_name().context("input filename")?)
                .with_extension("png");

            input_files.push(InputFile { path, output_path })
        }

        // now check if teir corresponding output files already exist, and if so, skip them
        let mut files_to_process: Vec<InputFileIndex> = Vec::with_capacity(input_files.len());
        for (i, input_file) in input_files.iter().enumerate() {
            if !fs.exists(&input_file.output_path) {
                files_to_process.push(InputFileIndex(i));
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
}

