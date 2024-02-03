# OMPL - Opinionated Music Player/Library v0.8.2
A music player organized exactly how I like it.

<img src="./screenshot.png" height = 400px />

## Features

  * Fully functional TUI with mouse & tty support
    * Filter and sort tags however you want. See [Tagstrings](https://github.com/Beinsezii/ompl#tagstrings) and [Filters](https://github.com/Beinsezii/ompl#filters)
    * Pretty colors
  * Fully functional CLI that interacts either with the TUI or a daemon
    * Identical functionality and control in both TUI and CLI/daemon
  * Sympal backend (default)
    * \+ Extremely efficient seeking and playback
    * \+ Supports many formats
    * \- Uses more RAM, particularly for [extremely long tracks](https://youtu.be/fQQxhyhdg-w)
    * \- Incompatible with Windows
    * \- Less stable, partially work-in-progress for edge cases
  * [Rodio](https://github.com/RustAudio/rodio) backend (default on Windows)
    * \+ Very safe, should always work
    * \+ Extremely low memory usage
    * \+ Compatible with Windows
    * \- No seeking of any kind
    * \- Limited supported formats
  * Support for many audio formats
    * Sympal backend: everything [Symphonia supports](https://github.com/pdeljanov/Symphonia#codecs-decoders)
      * Theoretically. Some things like .mp4 video files crash at the moment.
    * Rodio backend: [".mp3", ".flac", ".ogg", ".wav"]
    * All [ID3v2 tags/frames](https://id3.org/id3v2.3.0#Declared_ID3v2_frames). You may sort by either the 4-character codes (TALB, TIT2, etc) or the [human names](./src/library/track/mod.rs#L44). There's no standardization for the human names, so I tried to match what other taggers & players do.
    * Vorbis comments for FLAC/OGG
    * Utilizes ReplayGain (track gain only)
  * Pure Rust where possible. Very portable
  * Interfaces as a media player for direct OS control
  * Very fast - Handle a few thousand files effortlessly on a glorified tablet
  * Shouldn't crash

### Goals for 1.0
  * Code docs
  * Stability
  * More robust Sympal backend
  * See [TODO.md](./TODO.md) for more details
  
## Installation
Manually compiled and tested binaries for Linux and Windows are provided on the [releases tab](https://github.com/Beinsezii/ompl/releases)

Additionally, stable release binaries are automatically compiled and uploaded for Linux, Windows, and MacOS with the [Build Latest Release Tag Action](https://github.com/Beinsezii/ompl/actions/workflows/build_release_tag.yml)
while the latest unstable binaries are can be found in the [Build Master Release Action](https://github.com/Beinsezii/ompl/actions/workflows/build_release_master.yml)

Just pick the most recent (or otherwise) build of your choosing and download the artifact for your system. It'll arrive in a .zip file which you should be able to just unpack and run anywhere.

If you already have [Rust installed](https://rustup.rs/), you can build the latest release with

`cargo install --git https://github.com/Beinsezii/ompl.git --tag 0.8.2`

You may omit the `--tag` flag if you follow the rebel path and want the latest possibly unstable build.

Additionally, the some features are gated behind cargo flags.
These are all enabled by default, but can be disabled if a lighter binary is desired.
  * `media-controls` : Media interface, enables the operating system to control the player directly
  * `tui` : Enables the TUI. Without, will always run as a daemon
  * `clipboard` : Enables clipboard in the TUI
  * `backend-rodio` : Rodio backend
  * `backend-sympal` : Sympal backend

It's recommended you add the downloaded binary or cargo install directory to your environment `PATH` for ease of use.

## Usage

To start a simple example sorting by album run `ompl main Path/To/Music -f album`

To update the running program filters to genres "Epic" and "Game" while sorting results by album then title, run `ompl -f genre=Epic,Game -s album title`

To start a *second* server if your ADHD brain want two songs playing at once, run `ompl main Path/To/Music --port 12345`
Be careful to avoid commonly used port numbers. Typically this means > 1024, though I recommend at least 5 digits.

To view a full list of commands run `ompl help`

### Tagstrings
OMPL can sort by literal tags or "tagstrings", a special markup language for creating 'presentable' strings given the presence or lack of specific tags.

 * To simply filter by a single tag, you may type it literally: `album` will result in "Album"
 * To sort by multiple tags in one filter, use angle brackets like you would other markup language tags:`<genre> <album>` will result in "Genre Album"
 * To check for a tag's existence, use a vertical bar separating what you wish to display: `<album|<album> - ><title>` will result in "Album - Title" if the `album` tag is present, or "Title" if no album tag is present
 * To check for a tag's absence, add an exclamation after the first bracked: `<album|<album>><!album|<title>>` will result in "Album" if the `album` tag is present, or "Title" if no album tag is present.

Extra syntactical notes:
 * `???` will be the result if a non-conditional tag such as `<tag>` isn't found. Use a condition if you don't wish to display this: `<tag|<tag>><!tag|Tag not found!>`
 * Use `\` to escape characters: `\<title\>: <title>` will result in "<title>: Title"

### Filters
Filters are just Tagstrings that can also have values assigned to them.

 * In the TUI this is done by selecting them.
 * In the CLI you may append items after an equal `=`, ex `title=Song1,Song2` or `<genre>/<album>="Spicy/Meatball"`
   * Using Tagstrings directly (ie, without any items) is valid. This results in an empty filter, useful for laying out the TUI

## F.A.Q.
Question|Answer
---|---
Can you add support for my strange and unusual use-case?|OMPL isn't designed in any way to stream Spotify/show synchronized lyrics/etc. Use [quodlibet](https://quodlibet.readthedocs.io/en/latest/) or [foobar2000](https://www.foobar2000.org/), they both have similar layouts to OMPL
Can you change X functionality to be more like existing standards?|Maybe. Create an Issue with a good reason for the change, and ideally a source showing the standard implementation
Where is the configuration file?|In your heart. Every configurable setting is exposed by the CLI, so simply create a shortcut wherever you want and load up the command line flags. If something *isn't* available through CLI in some way, create an Issue

## SECRET KNOWLEDGE
* Left click on a filter's tagstring to invert the selection
* Right click on a filter's tagstring to clear the selection
* Right click and drag to [de]highlight many filter items
* The symbols on the bottom of filter/sorter panes are buttons for move<- add<- edit remove add-> move->
* Middle click a pane to highlight it without selecting anything
* Right click in the queue to select a track without playing it
* Right click the selected track again to center the view
* Scroll works almost everywhere, even on the volume indicator
* Right click the statusline or playback time to edit them directly
* Drag the seekbar to scrub it like a SoundCloud DJ
* Maybe more I forgot...
