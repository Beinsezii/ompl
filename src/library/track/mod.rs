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

// ## ID3 TAGS ## {{{
/// https://id3.org/id3v2.3.0#Declared_ID3v2_frames
const ID3_TAGS: &[(&'static str, &'static str)] = &[
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
// ## ID3 TAGS ## }}}

// ## ID3 GENRES ## {{{
// https://id3.org/id3v2.3.0#Declared_ID3v2_frames
const ID3_GENRES: &[&'static str] = &[
    "Blues",             // 0
    "Classic Rock",      // 1
    "Country",           // 2
    "Dance",             // 3
    "Disco",             // 4
    "Funk",              // 5
    "Grunge",            // 6
    "Hip-Hop",           // 7
    "Jazz",              // 8
    "Metal",             // 9
    "New Age",           // 10
    "Oldies",            // 11
    "Other",             // 12
    "Pop",               // 13
    "R&B",               // 14
    "Rap",               // 15
    "Reggae",            // 16
    "Rock",              // 17
    "Techno",            // 18
    "Industrial",        // 19
    "Alternative",       // 20
    "Ska",               // 21
    "Death Metal",       // 22
    "Pranks",            // 23
    "Soundtrack",        // 24
    "Euro-Techno",       // 25
    "Ambient",           // 26
    "Trip-Hop",          // 27
    "Vocal",             // 28
    "Jazz+Funk",         // 29
    "Fusion",            // 30
    "Trance",            // 31
    "Classical",         // 32
    "Instrumental",      // 33
    "Acid",              // 34
    "House",             // 35
    "Game",              // 36
    "Sound Clip",        // 37
    "Gospel",            // 38
    "Noise",             // 39
    "AlternRock",        // 40
    "Bass",              // 41
    "Soul",              // 42
    "Punk",              // 43
    "Space",             // 44
    "Meditative",        // 45
    "Instrumental Pop",  // 46
    "Instrumental Rock", // 47
    "Ethnic",            // 48
    "Gothic",            // 49
    "Darkwave",          // 50
    "Techno-Industrial", // 51
    "Electronic",        // 52
    "Pop-Folk",          // 53
    "Eurodance",         // 54
    "Dream",             // 55
    "Southern Rock",     // 56
    "Comedy",            // 57
    "Cult",              // 58
    "Gangsta",           // 59
    "Top 40",            // 60
    "Christian Rap",     // 61
    "Pop/Funk",          // 62
    "Jungle",            // 63
    "Native American",   // 64
    "Cabaret",           // 65
    "New Wave",          // 66
    "Psychadelic",       // 67
    "Rave",              // 68
    "Showtunes",         // 69
    "Trailer",           // 70
    "Lo-Fi",             // 71
    "Tribal",            // 72
    "Acid Punk",         // 73
    "Acid Jazz",         // 74
    "Polka",             // 75
    "Retro",             // 76
    "Musical",           // 77
    "Rock & Roll",       // 78
    "Hard Rock",         // 79
    // Winamp extensions
    "Folk",             // 80
    "Folk-Rock",        // 81
    "National Folk",    // 82
    "Swing",            // 83
    "Fast Fusion",      // 84
    "Bebob",            // 85
    "Latin",            // 86
    "Revival",          // 87
    "Celtic",           // 88
    "Bluegrass",        // 89
    "Avantgarde",       // 90
    "Gothic Rock",      // 91
    "Progressive Rock", // 92
    "Psychedelic Rock", // 93
    "Symphonic Rock",   // 94
    "Slow Rock",        // 95
    "Big Band",         // 96
    "Chorus",           // 97
    "Easy Listening",   // 98
    "Acoustic",         // 99
    "Humour",           // 100
    "Speech",           // 101
    "Chanson",          // 102
    "Opera",            // 103
    "Chamber Music",    // 104
    "Sonata",           // 105
    "Symphony",         // 106
    "Booty Bass",       // 107
    "Primus",           // 108
    "Porn Groove",      // 109
    "Satire",           // 110
    "Slow Jam",         // 111
    "Club",             // 112
    "Tango",            // 113
    "Samba",            // 114
    "Folklore",         // 115
    "Ballad",           // 116
    "Power Ballad",     // 117
    "Rhythmic Soul",    // 118
    "Freestyle",        // 119
    "Duet",             // 120
    "Punk Rock",        // 121
    "Drum Solo",        // 122
    "A cappella",       // 123
    "Euro-House",       // 124
    "Dance Hall",       // 125
];
// ## ID3 GENRES ## }}}

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
                    for (t_id, t_str) in ID3_TAGS {
                        if t_id == &id {
                            if let id3::Content::Text(t) = content {
                                // convert ID3v1 genre numbers
                                let t = if t_id == &"TCON" {
                                    t.trim()
                                        .trim_start_matches('(')
                                        .trim_end_matches(')')
                                        .parse::<usize>()
                                        .ok()
                                        .map(|i| ID3_GENRES.get(i).map(|s| s.to_string()))
                                        .flatten()
                                        .unwrap_or(t.to_string())
                                } else {
                                    t.to_string()
                                };
                                // lets you search for either the id3 ID or the 'pretty' name
                                self.tags.insert(t_str.to_ascii_lowercase(), t.clone());
                                self.tags.insert(t_id.to_ascii_lowercase(), t);
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

        let ids: Vec<&str> = ID3_TAGS.iter().map(|tid| tid.0).collect();
        let tags: Vec<&str> = ID3_TAGS.iter().map(|tid| tid.1).collect();

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
