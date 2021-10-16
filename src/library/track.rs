use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Track {
    path: PathBuf,
    meta: Option<symphonia::core::meta::MetadataRevision>,
}

impl Track {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self {
            path: path.into(),
            meta: None,
        }
    }

    pub fn get_reader(&self) -> BufReader<File> {
        BufReader::new(File::open(&self.path).unwrap())
    }

    pub fn load_meta(&mut self) {
        let mut hint = symphonia::core::probe::Hint::new();
        let probe_result = symphonia::default::get_probe()
            .format(
                match self.path.extension() {
                    Some(s) => hint.with_extension(s.to_str().unwrap()), //can this fail? Idk if pathbuf.extension() assumes utf-8
                    None => &hint,
                },
                symphonia::core::io::MediaSourceStream::new(
                    Box::new(std::fs::File::open(&self.path).unwrap()), //can't fail cause it was found earlier, yes? Permissions though...
                    symphonia::core::io::MediaSourceStreamOptions::default(),
                ),
                &symphonia::core::formats::FormatOptions::default(),
                &symphonia::core::meta::MetadataOptions::default(),
            )
            .ok()
            .map(|m| m.metadata.into_inner())
            .flatten();
        // .unwrap()
        // .metadata.into_inner();

        self.meta = probe_result
            .map(|mut m| m.metadata().current().cloned())
            .flatten();
    }

    pub fn tags(&self) -> Option<&[symphonia::core::meta::Tag]> {
        self.meta.as_ref().map(|m| m.tags())
    }
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}
