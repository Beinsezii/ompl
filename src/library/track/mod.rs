#![warn(missing_docs)]

use crate::logging::*;
use std::collections::HashMap;
use std::fs::File;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataRevision;
use symphonia::core::probe::Hint;

use lexical_sort::natural_lexical_cmp;
use walkdir::WalkDir;

pub type Tags = HashMap<String, String>;
pub mod tagstring;

// ## ID3 TAGS ## {{{
// TODO eventually cross-reference with non-free stuff?
// Might be overkill...
//
// Foobar2000:
// https://wiki.hydrogenaud.io/index.php?title=Foobar2000:ID3_Tag_Mapping
// Why even keep the source closed? It's been freeware since forever.
//
// Mp3Tag:
// https://docs.mp3tag.de/mapping-table/

/// Actually official reference. No human names?
/// https://id3.org/id3v2.3.0#Declared_ID3v2_frames
/// Also site has been down for a few months...
///
/// What other apps do for human names in order of personal preference:
///
/// FFmpeg:
/// https://git.ffmpeg.org/gitweb/ffmpeg.git/blob/HEAD:/libavformat/id3v2.c
///
/// puddletag:
/// https://docs.puddletag.net/source/id3.html
///
/// QuodLibet/ExFalso:
/// https://github.com/quodlibet/quodlibet/blob/main/quodlibet/formats/_id3.py
/// "ID3 is absolutely the worst thing ever." - lol
const ID3_TAGS: &[(&'static str, &'static str)] = &[
    //
    //
    // Seen in all 3
    //
    //
    ("talb", "album"),
    ("tcom", "composer"),
    ("tcon", "genre"),
    ("tcop", "copyright"),
    ("tenc", "encodedby"),
    ("tit1", "grouping"),
    ("tit2", "title"),
    ("tlan", "language"),
    ("tpe1", "artist"),
    //
    // itunescompilationflag in puddletag compilation elsewhere
    ("tcmp", "compilation"),
    //
    // year in puddletag date elsewhere.
    // Fuck both of them, TDAT exists, this is
    // record date now
    ("tdrc", "recorddate"),
    //
    // performer in quodlibet albumartist elsewhere
    ("tpe2", "albumartist"),
    //
    // performer in ffmpeg conductor elsewhere
    ("tpe3", "performer"),
    //
    // disc in ffmpeg discnumber elsewhere
    ("tpos", "disc"),
    //
    // publisher in ffmpeg orginization elsewhere
    ("tpub", "publisher"),
    //
    // tracknumber in quodlibet track elsewhere
    ("trck", "track"),
    //
    // albumsortorder in puddletag albumsort elsewhere
    ("tsoa", "albumsort"),
    //
    // performersortoder in puddletag artistsort elsewhere
    ("tsop", "artistsort"),
    //
    // titlesortorder in puddletag titlesort elsewhere
    ("tsot", "titlesort"),
    //
    // unsyncedlyrics in puddletag lyrics elsewhere
    ("uslt", "lyrics"),
    //
    // FFmpeg and PuddleTag
    //
    ("tsse", "encodingsettings"),
    //
    // creationtime in ffmpeg encodingtime in puddletag
    ("tden", "creationtime"),
    //
    // date in ffmpeg releasetime in puddletag
    // using releasedate for consistency with tdrc &&
    // avoiding TDAT 3 electric boogaloo
    ("tdrl", "releasedate"),
    //
    //
    // QuodLibet and Puddleteg
    //
    //
    ("tbpm", "bpm"),
    ("text", "lyricist"),
    ("tit3", "version"),
    ("tkey", "initialkey"),
    ("tmoo", "mood"),
    ("toal", "originalalbum"),
    ("toly", "author"),
    ("tope", "originalartist"),
    ("tpe4", "arranger"),
    ("tsrc", "isrc"),
    //
    // discsubtitle in quodlibet setsubtitle in puddletag
    ("tsst", "setsubtitle"),
    //
    // originaldate in quodlibet originalreleasetime in puddletag
    ("tdor", "originaldate"),
    //
    // website in quodlibet wwwartist in puddletag
    ("woar", "website"),
    //
    // albumartistsort in quodlibet itunesalbumsortorder in puddletag
    ("tso2", "albumartistsort"),
    //
    // composersort in quodlibet itunescomposersortorder in puddletag
    ("tsoc", "composersort"),
    //
    // media in quodlibet mediatype in puddletag
    ("tmed", "media"),
    //
    //
    // Only PuddleTag
    //
    //
    ("pcnt", "playcount"),
    ("popm", "popularimeter"),
    ("rva2", "rgain"), // apparently its for replaygain???
    ("tdat", "date"),
    ("tdly", "audiodelay"),
    ("tdtg", "taggingtime"),
    ("tflt", "filetype"),
    ("time", "time"),
    ("tipl", "involvedpeople"),
    ("tlen", "audiolength"),
    ("tmcl", "musiciancredits"),
    ("tofn", "filename"),
    ("tory", "originalyear"),
    ("town", "fileowner"),
    ("tpro", "producednotice"),
    ("trda", "recordingdates"),
    ("trsn", "radiostationname"),
    ("trso", "radioowner"),
    ("tsiz", "audiosize"),
    ("tyer", "year"),
    // the following normally have www in front which looks gross.
    // changing to site at end instead
    // have these been used for anything ever?
    ("wcom", "commercialinfosite"),
    ("wcop", "copyrightsite"),
    ("woaf", "fileinfosite"),
    ("woas", "sourcesite"),
    ("wors", "radiosite"),
    ("wpay", "paymentsite"),
    ("wpub", "publishersite"),
    //
    //
    // Not in any sourcej
    //
    //
    ("tsee", "equipment"),
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

pub fn find_tracks<T: AsRef<Path>>(path: T, types: &[String], include_hidden: bool) -> Vec<Track> {
    debug!("Finding tracks...");
    let now = Instant::now();

    let tracks: Vec<Track> = WalkDir::new(path)
        .follow_links(true)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| e.file_name().to_str().map(|s| include_hidden || !s.starts_with(".")).unwrap_or(false))
        .filter_map(|e| e.ok())
        .filter(|e| {
            if e.path().is_dir() {
                false
            } else {
                e.file_name()
                    .to_str()
                    .map(|s| {
                        let mut res = false;
                        for t in types.into_iter() {
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
        .filter_map(|e| Track::new(e.path()))
        .collect();

    bench!("Found {} tracks in {:?}", tracks.len(), now.elapsed());
    tracks
}

pub fn get_taglist<T: AsRef<str>, U: Deref<Target = Track>>(tagstring: T, tracks: &Vec<U>) -> Vec<String> {
    tracks
        .iter()
        .filter_map(|t| Some(tagstring::parse(tagstring.as_ref(), t.tags())))
        .collect::<Vec<String>>()
}

pub fn get_taglist_sort<T: AsRef<str>, U: Deref<Target = Track>>(tagstring: T, tracks: &Vec<U>) -> Vec<String> {
    let mut result = get_taglist(tagstring, tracks);
    result.sort_by(|a, b| natural_lexical_cmp(&a, &b));
    result.dedup();
    result
}

// ## FNs }}}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    path: PathBuf,
    tags: Tags,
    gain: f32,
}

impl Track {
    pub fn new<T: AsRef<Path>>(path: T) -> Option<Self> {
        path.as_ref().canonicalize().ok().map(|path| Self {
            path,
            tags: Tags::new(),
            gain: 1.0,
        })
    }

    /// Reads the current metadata revision
    fn read_metadata(&self) -> Option<MetadataRevision> {
        // {{{
        let Ok(file) = File::open(&self.path) else { return None };
        let Ok(mut probed) = symphonia::default::get_probe().format(
            Hint::new().with_extension(
                self.path()
                    .extension()
                    .map(|s| s.to_str())
                    .flatten()
                    .expect("HINT EXTENSION FAIL - should be unreachable"),
            ),
            MediaSourceStream::new(Box::new(file), Default::default()),
            &Default::default(),
            &Default::default(),
        ) else {
            return None;
        };
        probed
            .metadata
            .get()
            .map(|m| m.current().cloned())
            .flatten()
            // Vorbis comments aren't found until the FormatReader is initialized
            .or_else(|| probed.format.metadata().current().cloned())
        // }}}
    }

    /// Reads metadata into the struct. This doesn't happen on ::new() for performance reasons.
    pub fn load_meta(&mut self) {
        // {{{
        let Some(meta) = self.read_metadata() else {
            return;
        };

        for tag in meta.tags() {
            let mut val = tag.value.to_string();
            let mut key = tag.key.to_ascii_lowercase();

            // convert id3v1 genres
            if key == "tcon" {
                val = val
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .parse::<usize>()
                    .ok()
                    .map(|i| ID3_GENRES.get(i).map(|s| s.to_string()))
                    .flatten()
                    .unwrap_or(val)
            }

            // convert id3v2 keys to human readables
            for (fromkey, tokey) in ID3_TAGS {
                if fromkey == &key {
                    self.tags.insert(tokey.to_string(), val.clone());
                    break;
                }
            }

            // removing id3's "txxx:" leader for custom values
            if key.starts_with("txxx:") {
                key.replace_range(0..5, "");
            }
            // Additionally push all tags as they are
            self.tags.insert(key, val);
        }

        if let Some(text) = self.tags.get("replaygain_track_gain") {
            if let Ok(gain) = text[..text.rfind(|c: char| c.is_numeric()).unwrap_or(text.len() - 1) + 1]
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
                self.tags.insert("title".to_string(), path_title.to_string());
            }
        }
    } // }}}

    pub fn read_art(&self) -> Option<Box<[Box<[[u8; 4]]>]>> {
        // {{{
        let meta = self.read_metadata();
        let Some(visual) = meta.as_ref().map(|m| m.visuals().get(0)).flatten() else {
            return None;
        };
        let buff: &[u8] = &visual.data;
        let Ok(format) = image::guess_format(buff) else { return None };
        if let Ok(img) = image::load(std::io::Cursor::new(buff), format) {
            let (width, _height) = (img.width(), img.height());
            Some(
                img.into_rgba8()
                    .into_vec()
                    .chunks_exact(4)
                    .map(|chunk| chunk.try_into().unwrap())
                    .collect::<Vec<[u8; 4]>>()
                    .chunks_exact(width as usize)
                    .map(|v| v.into())
                    .collect(),
            )
        } else {
            None
        }
    } //}}}

    // ## GET / SET ## {{{

    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    pub fn tagstring<T: AsRef<str>>(&self, tagstring: T) -> String {
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
        let mut buff = format!("{}\nGAIN: {}\n", self.path.to_str().unwrap_or("Invalid Path!"), self.gain());

        let mut its = self.tags().iter().collect::<Vec<(&String, &String)>>();
        its.sort_by(|a, b| (a.0).cmp(b.0));

        for (k, v) in its.into_iter() {
            buff.push_str(&format!("\n{k}: {v}"))
        }

        f.write_str(&buff)
    }
}

#[cfg(test)]
mod id3tests {
    use super::ID3_TAGS;

    fn tovecs() -> (Vec<&'static str>, Vec<&'static str>) {
        let mut frames = Vec::new();
        let mut names = Vec::new();
        for (frame, name) in ID3_TAGS.into_iter() {
            frames.push(*frame);
            names.push(*name)
        }

        (frames, names)
    }

    #[test]
    /// No duplicates of anything
    fn duplicates() {
        let (mut frames, mut names) = tovecs();

        frames.sort();
        names.sort();

        let mut framesdedup = frames.clone();
        let mut namesdedup = names.clone();

        framesdedup.dedup();
        namesdedup.dedup();

        assert_eq!(
            frames.len(),
            framesdedup.len(),
            "{}",
            frames
                .iter()
                .zip(framesdedup.iter())
                .map(|(a, b)| format!("{} | {}", a, b))
                .collect::<Vec<String>>()
                .join("\n")
        );
        assert_eq!(
            names.len(),
            namesdedup.len(),
            "{}",
            names
                .iter()
                .zip(namesdedup.iter())
                .map(|(a, b)| format!("{} | {}", a, b))
                .collect::<Vec<String>>()
                .join("\n")
        );
    }

    #[test]
    /// Makes sure they're all lowercase for matching
    fn is_lowercase() {
        for (frame, name) in ID3_TAGS {
            assert_eq!(frame, &frame.to_lowercase());
            assert_eq!(name, &name.to_lowercase());
        }
    }

    #[test]
    /// Make sure all ID3 frames are covered.
    fn exists() {
        // Outdated and incomplete, as id3.org is down
        // so I only have the V2.3 frames from an old OMPL version
        const DECLARED_FRAMES: &[&'static str] = &[
            "talb", "tbpm", "tcom", "tcon", "tcop", "tdat", "tdly", "tenc", "text", "tflt", "time", "tit1", "tit2", "tit3", "tkey", "tlan", "tlen",
            "tmed", "toal", "tofn", "toly", "tope", "tory", "town", "tpe1", "tpe2", "tpe3", "tpe4", "tpos", "tpub", "trck", "trda", "trsn", "trso",
            "tsiz", "tsrc", "tsee", "tyer",
        ];

        let (frames, _names) = tovecs();

        for frame in DECLARED_FRAMES {
            assert!(frames.contains(frame), "FRAME: {}", frame)
        }
    }
}
