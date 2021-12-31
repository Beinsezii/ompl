# OMPL - Opinionated Music Player/Library v 0.2.0
A music player organized exactly how *I* like it.

## Features
  * Fully functional TUI with mouse & tty support
  * Fully functional CLI that interacts either with the TUI or a daemon
  * Support for audio formats present in [rodio](https://github.com/RustAudio/rodio) [".mp3", ".flac", ".ogg", ".wav"]
    * Supports all [ID3v2 tags/frames](https://id3.org/id3v2.3.0#Declared_ID3v2_frames). You may sort by either the 4-character codes or the common names that I definitely didn't just make up on the spot. See [here for the common names](./src/library/track.rs#L18)
  * Pure Rust where possible. *Should* be portable.
  * Interfaces as a media player for Linux MPRIS, Windows, and [untested] MacOS
  * Very fast - Handle a few thousand files effortlessly on a shitty 2006 acer laptop with a failing harddisk
    * Memory usage something like a few MBs
  * Shouldn't crash.

## WIP/Blocking features for 1.0.0
  * Filling out of `print` cli command
  * More advanced tag sorting, something a la [quodlibet's tag patterns](https://quodlibet.readthedocs.io/en/latest/guide/tags/patterns.html)
  * Theme customization
  * Polish passes/hundred papercuts
  * Possibly some form of retained settings/config file
    * Personally just modifying the cli command in my startup config has been fine, but this is a very common thing among any somewhat major program.
  
## Usage
<img src="./screenshot.png" width = 400px />
Either download a binary from the tags/releases tab or if you already have Rust installed, run `cargo install --git https://github.com/Beinsezii/ompl.git`.
It is recommended you add the downloaded binary or cargo install dir to your `PATH` for ease of use.

To start a simple example sorting by album run `ompl -l Path/To/Music -f album`

To update the running program filters to genres "Epic" and "Game" while sorting results by title, run `ompl -f genre=Epic,Game -f title`

To start a *new* TUI if you want two songs playing at once like a crackhead, run `ompl -l Path/To/Music --port 12345` or whatever valid port # you want.
Be careful to avoid commonly used ports such as 80, as other programs may be occupying these sockets.

To view a full list of daeomon/tui initialization commands, run `ompl --help` while no TUI/Daemons are open on the active port.

To view a full list of CLI commands, run `ompl --help` while the TUI/Daemon is already open on the active port

Both helps will also print TUI keybinds.

### Compiling
Have Rust 2021 installed, clone repo and just run `cargo build`.
`build_release.sh` will build in release mode for linux-x86_64-gnu and pc-windows-gnu, moving the binaries to ./bin/

Compiling with windows using the GNU abi will disable the media interface. This is to avoid miscompiles when cross-compiling via MinGW.
Compile on windows using MSVC or compile with MSYS2 and disable the windows-gnu checks to have a functional media interface.
I believe this is a problem with the [windows-rs](https://github.com/microsoft/windows-rs) crate and consequently [souvlaki](https://github.com/Sinono3/souvlaki) that I'm not sure how to work around.

## F.A.Q.
Question|Answer
---|---
Can you add support for my strange and unusual use-case?|Use [quodlibet](https://quodlibet.readthedocs.io/en/latest/) or [foobar2000](https://www.foobar2000.org/). This player is *mine*, not yours.
Can you change X functionality to be more like existing standards?|File a bug report with a good reason and I'll *consider* it.
Why are you so passive-aggressive?|I'm lonely.
