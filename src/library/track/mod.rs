use crate::{l1, l2, log, LOG_LEVEL};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use super::player::TYPES;
use walkdir::WalkDir;

pub type Tags = HashMap<String, String>;
pub mod tagstring;

// ## TAGS_IDS ## {{{
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
// ## TAGS ## }}}

// ## FNs ## {{{

pub fn get_tracks<T: AsRef<Path>>(path: T) -> Vec<Track> {
    l2!("Finding tracks...");
    let now = Instant::now();

    let tracks: Vec<Track> = WalkDir::new(path)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|s| !s.starts_with("."))
                .unwrap_or(false)
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| {
                    let mut res = false;
                    for t in TYPES.into_iter() {
                        if s.ends_with(t) {
                            res = true;
                            break;
                        }
                    }
                    res
                })
                .unwrap_or(false)
        })
        .map(|e| Track::new(e.path()))
        .collect();

    l1!(format!(
        "Found {} tracks in {:?}",
        tracks.len(),
        Instant::now() - now
    ));
    tracks
}

pub fn get_taglist<T: Into<String>, U: Deref<Target = Track>>(
    tagstring: T,
    tracks: &Vec<U>,
) -> Vec<String> {
    let tagstring = tagstring.into();
    tracks
        .iter()
        .filter_map(|t| Some(tagstring::parse(&tagstring, t.tags())))
        .collect::<Vec<String>>()
}

pub fn get_taglist_sort<T: Into<String>, U: Deref<Target = Track>>(
    tagstring: T,
    tracks: &Vec<U>,
) -> Vec<String> {
    let mut result = get_taglist(tagstring, tracks);
    result.sort();
    result.dedup();
    result
}

pub fn sort_by_tag<T: AsRef<str>, U: Deref<Target = Track>>(tag: T, tracks: &mut Vec<U>) {
    tracks.sort_by(|a, b| {
        let a = a.tags().get(tag.as_ref());
        let b = b.tags().get(tag.as_ref());
        if a.is_none() && b.is_none() {
            Ordering::Equal
        } else {
            match a {
                Some(a) => match b {
                    Some(b) => a.cmp(b),
                    None => Ordering::Greater,
                },
                None => Ordering::Less,
            }
        }
    })
}

// ## FNs }}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    path: PathBuf,
    tags: Tags,
}

impl Track {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self {
            path: path.into(),
            tags: Tags::new(),
        }
    }

    // # load_meta # {{{
    /// Reads metadata into the struct. This doesn't happen on ::new() for performance reasons.
    pub fn load_meta(&mut self) {
        if let Ok(frames) = id3::Tag::read_from_path(&self.path) {
            for frame in frames.frames() {
                let id = frame.id();
                let content = frame.content();
                // 'custom text' handling
                if id == "TXXX" {
                    if let id3::Content::ExtendedText(text) = content {
                        self.tags
                            .insert(text.description.clone(), text.value.clone());
                    }
                }
                // id3 standard tag strings
                else {
                    for (t_id, t_str) in TAG_IDS {
                        if t_id == &id {
                            if let id3::Content::Text(t) = content {
                                // lets you search for either the id3 ID or the 'pretty' name
                                self.tags
                                    .insert(t_str.to_ascii_lowercase().to_string(), t.to_string());
                                self.tags
                                    .insert(t_id.to_ascii_lowercase().to_string(), t.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }
        // use file stem if no title tag
        if !self.tags.contains_key("title") {
            if let Some(path_title) = self.path.file_stem().map(|os_s| os_s.to_str()).flatten() {
                self.tags
                    .insert("title".to_string(), path_title.to_string());
            }
        }
    }
    // # load_meta # }}}

    // ## GET / SET ## {{{

    pub fn get_reader(&self) -> BufReader<File> {
        BufReader::new(File::open(&self.path).unwrap())
    }

    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    pub fn tagstring<T: Into<String>>(&self, tagstring: T) -> String {
        tagstring::parse(tagstring, self.tags())
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    // ## GET / SET ## }}}
}

impl std::fmt::Display for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buff = format!("{}", self.path.to_str().unwrap_or("Invalid Path!"));
        for (t_id, t_str) in TAG_IDS {
            if let Some(tag) = self.tags().get(&t_id.to_ascii_lowercase()) {
                buff.push_str(&format!("\n{}/{}: {}", t_id, t_str, tag));
            }
        }
        write!(f, "{}", buff)
    }
}
