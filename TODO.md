## Just a disorganized to-do list since notes in my phone are too complicated for me

### TUI
* Revisit draw logic. Currently draws every click cause of pane/queue buggery.
    * since we already capture active states, we could just check for those changing && events.
    * Would need a special handler for scroll.
* Input bar CTRL-V/C/Back.
    * Copy/paste should be a feature flag, enabled by default.
* Stop button should highlight when stopped (not paused),
    * Way to color individual cells? Would writing ' ' work?

### MAIN
* method to print current filters for re-use
    * might need the --filter command to be updated into a single command.
      something like  --filter=album=Alb,Alb2||artist=billy||title
* CTRL-Z to close the TUI whilst keeping the daemon open. would be dope to re-open it later but idk how to read key shortcuts without crossterm.
    * should be as simple as tui() returning a bool that on true makes server .join()
* Logging overhaul. 3rd-party crate?

### LIBRARY
* should be able to reload
* should be able to handle songs being removed.
    * could just check if exists before sending to player, and remove() if not. technically would double filesystem calls, but they're infrequent *and* it'll hit the same spot twice for caching purposes. i mean it queries hundreds of files on startup in a second, it should be fine...
* should be able to play sequentially instead of always random. Good time to add now that queue sorting exists.

## Long-term aka unhinged ramblings

### TUI
* Investigate separating into default feature flag
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
