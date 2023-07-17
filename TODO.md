## Just a disorganized to-do list since notes in my phone are too complicated for me

### TUI
* Right now pop-ups are kinda jank. Can only be one at a time and they each have their 100% separate own event loop.
    * Maybe instead of a custom draw_inject() fn, UI could keep a list of extra layers that each have their own drawing and event processing fns that are run in the main loops. Then input/message simply add a layer top. Might need an additional identifier of some kind if you wish to avoid 75 of the same popup but right now there's nothing that could cause that so idk how worthwhile that would be..

### MAIN
* Logging overhaul. Should properly save log entries and output after TUI is closed. 3rd-party crate?
* When the codesplosions are done, every project file needs #![warn(missing_docs)]
    * also variants, methods, and fields all need sorting and clustering beyond just whim

### LIBRARY
* Sympal needs proper error handling. Also nothing should stop it from playing files like .mkv and .mp4, but it currently dies.
* Should also investigate resampling in Sympal for use on Windows [and direct ALSA?]
    * I saw a crate for this somewhere on the CPAL page but I don't remember what it's called.
* Possibly implement multiple or generic bit depths
    * Even most FLAC files I see are only 16 so maybe not useful

## Very stretchy goals
### TUI
* Display album art? I know certain terminals can, but checking compatibility and finding a library to do it nicely is gonna be pain.
