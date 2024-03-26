## Just a disorganized to-do list since notes in my phone are too complicated for me

### TUI
* Right now pop-ups are kinda jank. Can only be one at a time and they each have their 100% separate own event loop.
    * Maybe instead of a custom draw_inject() fn, UI could keep a list of extra layers that each have their own drawing and event processing fns that are run in the main loops. Then input/message simply add a layer top. Might need an additional identifier of some kind if you wish to avoid 75 of the same popup but right now there's nothing that could cause that so idk how worthwhile that would be..
* Migrate to ratatui or something because apparently `tui` is abandoned
    * https://ratatui.rs/how-to/develop-apps/migrate-from-tui-rs/

### MAIN
* Logging overhaul. Should properly save log entries and output after TUI is closed. 3rd-party crate?
* When the codesplosions are done, every project file needs #![warn(missing_docs)]
    * also variants, methods, and fields all need sorting and clustering beyond just whim

### LIBRARY
* Sympal should play any file you can imagine, like .mkv and .mp4. Right now it dies on video containers and some extra goofy audio streams.

## Very stretchy goals
### TUI
* Display album art? I know certain terminals can, but checking compatibility and finding a library to do it nicely is gonna be pain.
