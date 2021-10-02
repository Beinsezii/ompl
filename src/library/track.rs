use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Track {
    path: PathBuf,
}

impl Track {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self { path: path.into() }
    }
    pub fn get_reader(&self) -> BufReader<File> {
        BufReader::new(File::open(&self.path).unwrap())
    }
}
