use super::FileSystem;
use anyhow::Context;
use rustc_hash::FxHashMap as HashMap;

use core::str;
use std::io::{self, BufRead, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// An in-memory implementation of [`FileSystem`] for use whenever there is no access to a local
/// file system (such as on WASM), or to speed up the processing when there is a lot of RAM available.
///
/// This object is thread-safe and can be shared between threads. Uses [`Arc`] internally so it is
/// cheap to clone.
#[derive(Debug, Clone)]
pub struct MemoryFileSystem {
    root: Arc<RwLock<Root>>,
}

impl Default for MemoryFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryFileSystem {
    /// Create a new empty memory file system.
    pub fn new() -> Self {
        Self {
            root: Arc::new(RwLock::new(Root(Directory::default()))),
        }
    }

    /// Load the contents of a file on the local file system into the memory file system.
    pub fn load_from_disk(
        &self,
        from_disk: impl AsRef<Path>,
        to_internal: impl AsRef<Path>,
    ) -> io::Result<()> {
        let bytes = std::fs::read(from_disk)?;
        let mut writer = self.create(to_internal)?;
        writer.write_all(&bytes)?;
        Ok(())
    }
    /// Write the contents of a  file in the memory file system to the local file system.
    pub fn save_to_disk(
        &self,
        from_internal: impl AsRef<Path>,
        to_external: impl AsRef<Path>,
    ) -> io::Result<()> {
        let mut reader = self.open(from_internal)?;
        let mut writer = io::BufWriter::new(std::fs::File::create(to_external)?);
        std::io::copy(&mut reader, &mut writer)?;
        Ok(())
    }
}

/// Represents the root of the file system.
#[derive(Debug)]
struct Root(Directory);

impl Root {
    /// Traverse the file system to find the directory at the given path.
    fn get_directory(&self, path: impl AsRef<Path>) -> Result<&Directory, io::Error> {
        let path = path.as_ref();
        let mut dir: &Directory = &self.0;
        for name in &self.resolve_path(path)? {
            dir = match dir.subdirs.get(name) {
                Some(subdir) => subdir,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "directory not found",
                    ));
                }
            };
        }
        Ok(dir)
    }

    /// Traverse the file system to find the mutable directory at the given path.
    fn get_directory_mut(&mut self, path: impl AsRef<Path>) -> Result<&mut Directory, io::Error> {
        let path = path.as_ref();
        let parts = self.resolve_path(path)?;
        let mut dir: &mut Directory = &mut self.0;
        for name in &parts {
            dir = match dir.subdirs.get_mut(name) {
                Some(subdir) => subdir,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "directory not found",
                    ));
                }
            };
        }
        Ok(dir)
    }

    /// Resolve a path to a canonical path (removing "..", "." and "/") containing only the direct path coponents.
    fn resolve_path(&self, path: &Path) -> Result<Vec<String>, io::Error> {
        let mut part: Vec<String> = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(component) => {
                    let name = component.to_string_lossy().to_string();

                    part.push(name);
                }
                Component::ParentDir => {
                    // since this is the root directory, we cannot have paths like "../a", thus return an error
                    if part.pop().is_none() {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "root has no parent directory",
                        ));
                    }
                }
                Component::CurDir => {}
                Component::RootDir => {
                    part.clear();
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "path prefix not supported",
                    ));
                }
            }
        }
        Ok(part)
    }
}

#[derive(Debug, Default)]
struct Directory {
    subdirs: HashMap<String, Directory>,
    files: HashMap<String, FileEntry>,
}

/// Get the parent directory of a file or directory path.
fn file_parent(path: &Path) -> Result<&Path, io::Error> {
    path.parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "parent directory not found"))
}

#[derive(Debug)]
struct FileEntry {
    /// data is stored as an Arc to allow for multiple readers.
    /// Wrapped in an [`RwLock`] to allow for swapping the value when the Writer is dropped / finished.
    data: Arc<RwLock<FileData>>,
}

impl FileEntry {
    /// Create a new empty file entry.
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(FileData::new())),
        }
    }
}
impl std::fmt::Debug for FileData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.0.len();
        f.debug_struct("FileData").field("len", &len).finish()
    }
}

/// A file that is currently being written too. Has a link back to the [`FileData`] so it can
/// swap it whenever the writer is dropped.
struct WritableFile {
    /// The data beeing written to the file
    data: io::Cursor<Vec<u8>>,
    /// links back to the file entry so we can swap the data when the writer is dropped
    data_link: Arc<RwLock<FileData>>,
}

impl Write for WritableFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.data.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.data.flush()
    }
}

impl Seek for WritableFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.data.seek(pos)
    }
}

impl Drop for WritableFile {
    // swap the data into the file entry on drop
    fn drop(&mut self) {
        let data = core::mem::replace(&mut self.data, io::Cursor::new(Vec::new()));
        let mut data_link = self.data_link.write().expect("file data lock poisoned");
        *data_link = FileData(Arc::new(data.into_inner()));
    }
}

/// Holds the data of a file. Cheap to clone because the data is behind an [`Arc`].
#[derive(Clone)]
struct FileData(Arc<Vec<u8>>);

impl FileData {
    fn new() -> Self {
        Self(Arc::new(Vec::new()))
    }
}

/// this allows us to treat [`FileData`] as a slice of bytes, which is useful for the [`Read`] trait
impl AsRef<[u8]> for FileData {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl FileSystem for MemoryFileSystem {
    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut root = self.root.write().expect("root lock poisoned");
        let path = path.as_ref();

        // make sure all directories in the path exist
        let parts = root.resolve_path(path)?;
        let mut dir: &mut Directory = &mut root.0;
        for name in parts {
            dir = dir.subdirs.entry(name).or_default();
        }
        Ok(())
    }

    fn list(&self, path: impl AsRef<Path>) -> Result<Vec<PathBuf>, io::Error> {
        let root = self.root.read().expect("root lock poisoned");
        let path = path.as_ref();

        // find the directory
        let dir = root.get_directory(path)?;

        // list the contents
        let mut entries = vec![];
        for name in dir.subdirs.keys() {
            entries.push(path.join(name));
        }
        for name in dir.files.keys() {
            entries.push(path.join(name));
        }
        Ok(entries)
    }

    fn exists(&self, path: impl AsRef<Path>) -> bool {
        let root = self.root.read().expect("root lock poisoned");
        let path = path.as_ref();

        let Some(parent) = path.parent() else {
            return false;
        };

        // find the directory or return false if it does not exist
        let Some(dir) = root.get_directory(parent).ok() else {
            return false;
        };

        // get file / directory name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // check if it exists as a directory or file
        dir.subdirs.contains_key(&name) || dir.files.contains_key(&name)
    }

    fn open(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<impl BufRead + Seek + Send + 'static, io::Error> {
        let root = self.root.read().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the directory
        let dir = root.get_directory(parent)?;

        // get file name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // get the file entry
        let file = match dir.files.get(&name) {
            Some(file) => file,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "file not found")),
        };

        // create a reader by cloning the Arc
        let data = file.data.read().unwrap().clone();
        Ok(io::Cursor::new(data))
    }

    fn create(&self, path: impl AsRef<Path>) -> Result<impl Write + Seek, io::Error> {
        let mut root = self.root.write().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the parent directory
        let dir = root.get_directory_mut(parent)?;

        // get file name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // open or create new file
        let file = dir.files.entry(name).or_insert(FileEntry::new());

        // now we replace the arc with a new one which we will write to. This way existing readers
        // will continue to read the old data, while we start filling up some new data)
        let writer = WritableFile {
            data: io::Cursor::new(Vec::new()),
            data_link: file.data.clone(), // linked to the place where the data is stored
        };
        Ok(writer)
    }

    fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, io::Error> {
        let root = self.root.read().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the directory
        let dir = root.get_directory(parent)?;

        // get file name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // get the file entry
        let file = match dir.files.get(&name) {
            Some(file) => file,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "file not found")),
        };

        // create a reader by cloning the Arc
        let data = file.data.read().unwrap();

        // convert to string lossily expecting all data to be valid utf8
        let str = str::from_utf8(&data.0).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("invalid UTF-8: {e} "))
        })?;

        Ok(str.to_string())
    }

    fn remove_file(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut root = self.root.write().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the directory
        let dir = root.get_directory_mut(parent)?;

        // get file name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // remove the file
        dir.files
            .remove(&name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file not found"))?;

        Ok(())
    }

    fn remove_dir_all(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut root = self.root.write().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the parent directory
        let dir = root.get_directory_mut(parent)?;

        // get the dir name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        dir.subdirs
            .remove(&name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "subdir not found"))?;

        Ok(())
    }

    fn file_size(&self, path: impl AsRef<Path>) -> Result<u64, io::Error> {
        let root = self.root.read().expect("root lock poisoned");
        let path = path.as_ref();

        let parent = file_parent(path)?;

        // find the directory
        let dir = root.get_directory(parent)?;

        // get file name
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // get the file entry
        let file = match dir.files.get(&name) {
            Some(file) => file,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "file not found")),
        };

        let data = file.data.read().expect("file data lock poisoned");
        Ok(data.0.len() as u64)
    }

    fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut root = self.root.write().expect("root lock poisoned");
        let from = from.as_ref();
        let to = to.as_ref();

        let from_parent = file_parent(from)?;
        let to_parent = file_parent(to)?;

        // find the from directory
        let from_dir = root.get_directory(from_parent)?;

        // get the from file entry and clone the data
        let from_name = from.file_name().unwrap().to_string_lossy().to_string();
        let from_file = match from_dir.files.get(&from_name) {
            Some(file) => file,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "file not found")),
        };
        let from_data = from_file
            .data
            .read()
            .expect("file data lock poisoned")
            .clone();

        // find the to directory
        let to_dir = root.get_directory_mut(to_parent)?;

        // get file names
        let to_name = to.file_name().unwrap().to_string_lossy().to_string();
        let to_file = to_dir.files.entry(to_name).or_insert(FileEntry::new());
        // copy the data
        let mut to_data = to_file.data.write().expect("file data lock poisoned");
        *to_data = from_data;

        Ok(())
    }

    fn extract_zip(
        &self,
        archive: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let file = self.open(&archive).context("opening zip file")?;
        let mut zip_archive = zip::ZipArchive::new(file).context("reading zip archive")?;
        log::info!(
            "Extracting {:?} kB from {}",
            zip_archive.decompressed_size().map(|s| s / 1024),
            archive.as_ref().display()
        );

        for i in 0..zip_archive.len() {
            let mut file = zip_archive
                .by_index(i)
                .with_context(|| format!("get file index {i}"))?;

            if file.is_symlink() {
                log::warn!(
                    "Skipping symlink {} in zip archive as it is not supported",
                    file.name(),
                );
                continue;
            }

            let Some(name) = file.enclosed_name() else {
                log::warn!(
                    "Skipping file {} in zip archive as it is not a valid path",
                    file.name(),
                );
                continue;
            };

            let target_name = target.as_ref().join(&name);

            if file.is_dir() {
                log::debug!("Creating directory {}", target_name.display());
                self.create_dir_all(&target_name)?;
                continue;
            }

            log::debug!("Extracting file {}", name.display());

            // at this point the file is a file and so we copy it into the FS
            let mut output = self.create(target_name)?;
            std::io::copy(&mut file, &mut output)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::io::Read;

    use super::*;

    #[test]
    fn test_resolve_path() {
        let root = Root(Directory::default());
        assert_eq!(root.resolve_path(Path::new("folder")).unwrap(), ["folder"]);
        assert_eq!(
            root.resolve_path(Path::new("folder/folder2")).unwrap(),
            ["folder", "folder2"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/folder2/folder3"))
                .unwrap(),
            ["folder", "folder2", "folder3"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/../folder2")).unwrap(),
            ["folder2"]
        );
        assert_eq!(
            root.resolve_path(Path::new("./folder/../folder2")).unwrap(),
            ["folder2"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/./folder2")).unwrap(),
            ["folder", "folder2"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/folder2/./folder3"))
                .unwrap(),
            ["folder", "folder2", "folder3"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/folder2/../folder3"))
                .unwrap(),
            ["folder", "folder3"]
        );
        assert_eq!(
            root.resolve_path(Path::new("folder/folder2/../../folder3"))
                .unwrap(),
            ["folder3"]
        );
        assert_eq!(
            root.resolve_path(Path::new("/folder/../folder2")).unwrap(),
            ["folder2"]
        );

        // test error cases
        assert!(root.resolve_path(Path::new("..")).is_err());
        assert!(root.resolve_path(Path::new("../a")).is_err());
        assert!(root.resolve_path(Path::new("folder/../..")).is_err());
        assert!(
            root.resolve_path(Path::new("folder/folder2/../../.."))
                .is_err()
        );
    }

    #[test]
    fn test_file_parent() {
        assert_eq!(file_parent(Path::new("folder")).unwrap(), Path::new(""));
        assert_eq!(
            file_parent(Path::new("folder/folder2")).unwrap(),
            Path::new("folder")
        );
        assert_eq!(
            file_parent(Path::new("folder/folder2/folder3")).unwrap(),
            Path::new("folder/folder2")
        );
        assert_eq!(
            file_parent(Path::new("folder/../folder2")).unwrap(),
            Path::new("folder/..")
        );

        assert_eq!(file_parent(Path::new("./")).unwrap(), Path::new(""));

        // test error cases
        assert!(file_parent(Path::new("/")).is_err());
        assert!(file_parent(Path::new("")).is_err());
    }

    #[test]
    fn test_write_read_to_string_root() {
        let fs = super::MemoryFileSystem::new();
        let path = "test.txt";
        let content = "Hello, World!";

        fs.create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        let read = fs.read_to_string(path).unwrap();

        assert_eq!(read, content);
    }

    #[test]
    fn test_write_read_to_string_subdir() {
        let fs = super::MemoryFileSystem::new();
        let folder = Path::new("folder");
        fs.create_dir_all(folder).unwrap();
        let path = folder.join("test.txt");
        let content = "Hello, World!";

        fs.create(&path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        let read = fs.read_to_string(path).unwrap();

        assert_eq!(read, content);
    }

    #[test]
    fn test_read_string_invalid_utf8() {
        let fs = super::MemoryFileSystem::new();
        let folder = Path::new("folder");
        fs.create_dir_all(folder).unwrap();
        let path = folder.join("invalid.file");
        let content = [0, 1, 2, 3, 4, 5, 6, 255]; // invalid utf8

        fs.create(&path).unwrap().write_all(&content).unwrap();

        let read = fs.read_to_string(path).unwrap_err();
        assert_eq!(read.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_create_open() {
        let fs = super::MemoryFileSystem::new();
        let path = "file.json";
        let content = "contents of the file";

        fs.create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        let mut read = fs.open(path).unwrap();
        let mut buff = Vec::new();
        assert_eq!(read.read_to_end(&mut buff).unwrap(), content.len());

        assert_eq!(buff, content.as_bytes());
    }

    #[test]
    fn test_create_not_found() {
        let fs = super::MemoryFileSystem::new();
        let folder = Path::new("folder");
        let path = folder.join("nonexistant_subdirectory").join("file.json");

        fs.create_dir_all(folder).unwrap();
        match fs.create(&path) {
            Ok(_) => panic!("file should not exist"),
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        };
    }

    #[test]
    fn test_file_does_not_exist() {
        let fs = super::MemoryFileSystem::new();
        let path = "test.txt";

        assert!(!fs.exists(path));

        match fs.open(path) {
            Ok(_) => panic!("file should not exist"),
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn test_file_does_not_exist_subdirs() {
        let fs = super::MemoryFileSystem::new();
        let folder = Path::new("folder1/folder2");
        let path = folder.join("nonexistant_subfolder").join("test.txt");

        fs.create_dir_all(folder).unwrap();
        assert!(!fs.exists(&path));

        match fs.open(path) {
            Ok(_) => panic!("file should not exist"),
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn test_create_and_remove_file() {
        let fs = super::MemoryFileSystem::new();
        let path = "test.txt";
        let content = "Hello, World!";

        fs.create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        assert!(fs.exists(path));

        fs.remove_file(path).unwrap();

        assert!(!fs.exists(path));

        match fs.open(path) {
            Ok(_) => panic!("file should not exist"),
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn test_create_and_list_files_and_folders() {
        let fs = super::MemoryFileSystem::new();
        let folder = Path::new("folder");
        fs.create_dir_all(folder).unwrap();
        let path1 = folder.join("test1.txt");
        let path2 = folder.join("test2.txt");
        let path3 = folder.join("subfolder1");
        let path4 = folder.join("subfolder2");

        fs.create(&path1).unwrap();
        fs.create(&path2).unwrap();
        fs.create_dir_all(&path3).unwrap();
        fs.create_dir_all(&path4).unwrap();

        let files = fs.list(folder).unwrap();
        assert_eq!(files.len(), 4);
        assert!(files.contains(&path1));
        assert!(files.contains(&path2));
        assert!(files.contains(&path3));
        assert!(files.contains(&path4));
    }

    #[test]
    fn test_file_size() {
        let fs = super::MemoryFileSystem::new();
        let path = "test.txt";
        let content = "Hello, World!";

        fs.create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        let size = fs.file_size(path).unwrap();
        assert_eq!(size, content.len() as u64);
    }

    #[test]
    fn test_copy_file() {
        let fs = super::MemoryFileSystem::new();
        let path1 = "test1.txt";
        let path2 = "test2.txt";
        let content = "Hello, World!";

        fs.create(path1)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();

        fs.copy(path1, path2).unwrap();

        let read = fs.read_to_string(path2).unwrap();
        assert_eq!(read, content);
    }
}
