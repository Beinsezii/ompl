use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

pub type Tags = HashMap<String, String>;

/// https://id3.org/id3v2.3.0#Declared_ID3v2_frames
const TAG_IDS: &[(&'static str, &'static str)] = &[
    ("TALB", "Album"),
    ("TBPM", "BPM"),
    ("TCOM", "Composer"),
    ("TCON", "Genre"),
    ("TCOP", "Copyright"),
    ("TDAT", "Date"),
    ("TDLY", "Delay"),
    ("TENC", "Encoder"),
    ("TEXT", "Lyricist"),
    ("TFLT", "FileType"),
    ("TIME", "Time"),
    ("TIT1", "Grouping"),
    ("TIT2", "Title"),
    ("TIT3", "Subtitle"),
    ("TKEY", "Key"),
    ("TLAN", "Language"),
    ("TLEN", "Length"),
    ("TMED", "Mediatype"),
    ("TOAL", "OriginalAlbum"),
    ("TOFN", "OriginalFilename"),
    ("TOLY", "OriginalLyricist"),
    ("TOPE", "OriginalArtist"),
    ("TORY", "OriginalYear"),
    ("TOWN", "Owner"),
    ("TPE1", "Artist"),
    ("TPE2", "Accompaniment"),
    ("TPE3", "Performer"),
    ("TPE4", "Mixer"),
    ("TPOS", "Set"),
    ("TPUB", "Publisher"),
    ("TRCK", "Track"),
    ("TRDA", "RecordingDate"),
    ("TRSN", "Station"),
    ("TRSO", "StationOwner"),
    ("TSIZ", "Size"),
    ("TSRC", "ISRC"),
    ("TSEE", "Equipment"),
    ("TYER", "Year"),
];

#[derive(Debug, Clone)]
pub struct Track {
    path: PathBuf,
    // meta: Option<symphonia::core::meta::MetadataRevision>,
    tags: Tags,
}

impl Track {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self {
            path: path.into(),
            tags: Tags::new(),
        }
    }

    pub fn get_reader(&self) -> BufReader<File> {
        BufReader::new(File::open(&self.path).unwrap())
    }

    /// Reads metadata into the struct. This doesn't happen on ::new() for performance reasons.
    pub fn load_meta(&mut self) {
        if let Ok(frames) = id3::Tag::read_from_path(&self.path) {
            for frame in frames.frames() {
                let id = frame.id();
                let content = frame.content();
                // 'custom text' handling
                if id == "TXXX" {
                    if let id3::Content::ExtendedText(text) = content {
                        self.tags.insert(text.description.clone(), text.value.clone());
                    }
                }
                // id3 standard tag strings
                else {
                    for (t_id, t_str) in TAG_IDS {
                        if t_id == &id {
                            if let id3::Content::Text(t) = content {
                                // lets you search for either the id3 ID or the 'pretty' name
                                self.tags.insert(t_str.to_string(), t.to_string());
                                self.tags.insert(t_id.to_string(), t.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }
        // use file stem if no title tag
        if !self.tags.contains_key("Title") {
            if let Some(path_title) = self.path.file_stem().map(|os_s| os_s.to_str()).flatten() {
                self.tags
                    .insert("Title".to_string(), path_title.to_string());
            }
        }
    }

    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    // Symphonia ver. Borken. Idk why. Symphonia is weird.
    // pub fn load_meta(&mut self) {
    //     let mut hint = symphonia::core::probe::Hint::new();
    //     let probe_result = symphonia::default::get_probe()
    //         .format(
    //             match self.path.extension() {
    //                 Some(s) => hint.with_extension(s.to_str().unwrap()), //can this fail? Idk if pathbuf.extension() assumes utf-8
    //                 None => &hint,
    //             },
    //             symphonia::core::io::MediaSourceStream::new(
    //                 Box::new(std::fs::File::open(&self.path).unwrap()), //can't fail cause it was found earlier, yes? Permissions though...
    //                 symphonia::core::io::MediaSourceStreamOptions::default(),
    //             ),
    //             &symphonia::core::formats::FormatOptions::default(),
    //             &symphonia::core::meta::MetadataOptions::default(),
    //         )
    //         .ok()
    //         .map(|m| m.metadata.into_inner())
    //         .flatten();
    //     // .unwrap()
    //     // .metadata.into_inner();

    //     self.meta = probe_result
    //         .map(|mut m| m.metadata().current().cloned())
    //         .flatten();
    // }
    // pub fn tags(&self) -> Option<&[symphonia::core::meta::Tag]> {
    //     self.meta.as_ref().map(|m| m.tags())
    // }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}
