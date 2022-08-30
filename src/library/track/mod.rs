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

pub fn get_tracks<T: AsRef<Path>>(path: T, include_hidden: bool) -> Vec<Track> {
    l2!("Finding tracks...");
    let now = Instant::now();

    let tracks: Vec<Track> = WalkDir::new(path)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|s| include_hidden || !s.starts_with("."))
                .unwrap_or(false)
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            if e.path().is_dir() {
                false
            } else {
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
            }
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

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    path: PathBuf,
    tags: Tags,
    gain: f32,
}

impl Track {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self {
            path: path.into(),
            tags: Tags::new(),
            gain: 1.0,
        }
    }

    // ## META ## {{{
    /// Reads metadata into the struct. This doesn't happen on ::new() for performance reasons.

    // # load_meta # {{{
    pub fn load_meta(&mut self) {
        match self.path.extension().map(|e| e.to_str()).flatten() {
            Some("mp3") | Some("wav") => self.load_meta_id3(),
            Some("flac") => self.load_meta_vorbis::<symphonia::default::formats::FlacReader>(),
            Some("ogg") => self.load_meta_vorbis::<symphonia::default::formats::OggReader>(),
            _ => (),
        }

        if let Some(text) = self.tags.get("replaygain_track_gain") {
            if let Ok(gain) = text[..text
                .rfind(|c: char| c.is_numeric())
                .unwrap_or(text.len() - 1)
                + 1]
                .trim_start()
                .parse::<f32>()
            {
                // according to the internet, A2 = A1 * 10(GdB / 20)
                // where A1 is our volume set in library, G is the replaygain
                // offset, and A2 is the final result Rodio should eat.
                self.gain = 10f32.powf(gain / 20.0)
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

    // # id3 # {{{
    fn load_meta_id3(&mut self) {
        if let Ok(frames) = id3::Tag::read_from_path(&self.path) {
            for frame in frames.frames() {
                let id = frame.id();
                let content = frame.content();
                // 'custom text' handling
                if id == "TXXX" {
                    if let id3::Content::ExtendedText(text) = content {
                        self.tags
                            .insert(text.description.to_ascii_lowercase(), text.value.clone());
                    }
                }
                // id3 standard tag strings
                else {
                    for (t_id, t_str) in TAG_IDS {
                        if t_id == &id {
                            if let id3::Content::Text(t) = content {
                                // lets you search for either the id3 ID or the 'pretty' name
                                self.tags.insert(t_str.to_ascii_lowercase(), t.to_string());
                                self.tags.insert(t_id.to_ascii_lowercase(), t.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    // # id3 # }}}

    // # vorbis comment # {{{
    fn load_meta_vorbis<R: symphonia::core::formats::FormatReader>(&mut self) {
        let formatreader: Result<Result<R, _>, _> = File::open(&self.path).map(|file| {
            symphonia::core::formats::FormatReader::try_new(
                symphonia::core::io::MediaSourceStream::new(
                    Box::new(file),
                    symphonia::core::io::MediaSourceStreamOptions::default(),
                ),
                &symphonia::core::formats::FormatOptions::default(),
            )
        }); // flatten is still experimental
        if let Ok(Ok(mut reader)) = formatreader {
            if let Some(meta) = reader.metadata().skip_to_latest() {
                for tag in meta.tags() {
                    self.tags
                        .insert(tag.key.to_ascii_lowercase(), tag.value.to_string());
                }
            }
        }
    }
    // # vorbis comment # }}}

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

    pub fn gain(&self) -> f32 {
        self.gain
    }

    // ## GET / SET ## }}}
}

impl std::fmt::Display for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buff1 = format!("{}", self.path.to_str().unwrap_or("Invalid Path!"));
        let mut buff2 = String::new();

        let ids: Vec<&str> = TAG_IDS.iter().map(|tid| tid.0).collect();
        let tags: Vec<&str> = TAG_IDS.iter().map(|tid| tid.1).collect();

        for key in self.tags().keys() {
            if let Some(p) = ids
                .iter()
                .position(|&x| x.to_ascii_lowercase() == key.to_ascii_lowercase())
            {
                buff1.push_str(&format!(
                    "\n{}/{}: {}",
                    ids[p],
                    tags[p],
                    self.tags().get(key).unwrap()
                ));
            } else if tags
                .iter()
                .position(|&x| x.to_ascii_lowercase() == key.to_ascii_lowercase())
                .is_none()
            {
                buff2.push_str(&format!("\n{}: {}", key, self.tags().get(key).unwrap()))
            }
        }
        write!(f, "{}{}", buff1, buff2)
    }
}
