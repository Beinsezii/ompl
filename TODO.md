## Just a disorganized to-do list since notes in my phone are too complicated for me

### TUI
* Right now pop-ups are kinda jank. Can only be one at a time and they each have their 100% separate own event loop.
    * Maybe instead of a custom draw_inject() fn, UI could keep a list of extra layers that each have their own drawing and event processing fns that are run in the main loops. Then input/message simply add a layer top. Might need an additional identifier of some kind if you wish to avoid 75 of the same popup but right now there's nothing that could cause that so idk how worthwhile that would be..

### MAIN
* Logging overhaul. 3rd-party crate?
* When the codesplosions are done, every project file needs #![warn(missing_docs)]
    * also variants, methods, and fields all need sorting and clustering beyond just whim

### LIBRARY
* tagstring engine needs refinement. Currently a song titled "Song title <bonus>" will show "Song title ???"
    * Take tagstring as read-only and build new string out of stuff? That way tag data is never read by parser.

## Long-term aka unhinged ramblings

### TUI
* Could use a method of opening without CLI commands. Handy for Windows specifially.
    * Could just barf filters, library path, and volume into a .json file at the os-appropriate config home, then if ompl is run with no args or --resume it loads the TUI from there.
    * TUI will need a way to set library path from within.
        * If launched without a .json present, should ideally prompt for a library path. orrrrr maybe it could just default to ~/Music....
        * This will be a pain in the ASS to type. maybe it could capture the whole screen like message(), and show a list of folders auto-completion style below..? Or I could write a whole ass directory browser. Or ignore it and just rely on input-bar's copy-paste.
* Display album art? I know certain terminals can, but checking compatibility and finding a library to do it nicely is gonna be pain.
* a bargraph visualizer would be dope. idk how it'd read the samples for that.

### LIBRARY
* Should really be able to seek. Possible gStreamer alternate backend?
* Pausing should drop the sound device. This might actually be possible with Rodio if I can somehow 'save' the decoded samples and re-use them.

### GUI
* With the above, probably wouldn't be valuable. The only attractive option is GTK, and I've had enough bad experiences with GTK music playeres in particular for some reason. In fact, one of the main motivators for this is certain GTK on my system will randomly hang with certain apps after a few hours, notably Quod Libet and Tint2.
* Something like egui might be fun. Problem is idk how well it'll handle event-driven shit with updating from cli usage. egui either updates ALWAYS or when input is detected. idk if widgets can 'animate' idly. also the text looks like ass.
