Nisdos Terminal
===
A terminal emulator we are now using at Nisdos, open-sourced for community needs. It aims at high performance, while having all modern features. Built on pure Rust & [SDL3](https://www.libsdl.org/).

Features
===
- flexible layout: movable tabs, resizable panes
- extensive state, restored on startup
- cross-platform (Linux, Mac, Windows)
- configurable hotkeys and mouse tricks
- scrollable output history
- primary buffer support on Linux
- smart Ctrl+C & Ctrl+V (works when it doesn't interfere with terminal apps)
- sequential hotkeys (built-in example: Alt-G-P — go to prompt)
- emojis, including modifiers and combined emojis
- multiterminal typing (send input to several terminals simultaneously)
- terminal keeps own input and output history in state
- convenient command history search
- AI-assistant to help working with OS (not for coding)
- error detection

Next steps
===
- further expand settings
- voice input
- UI themes
- plugins system
- ligatures support
- [Kitty](https://sw.kovidgoyal.net/kitty/graphics-protocol/) protocol support

State
===
State includes layout structure, relative pane sizes, current working directory, tab names, a few of last commands of each terminal and some amount of previous output of each terminal. State is saved and loaded automatically. State files are human-readable to make it transparent what we actually store in them.

History search
===
If a terminal is in a normal, not grouped, mode you can press Ctrl+R (configurable hotkey) and get a visual history search dialog with combined shell & private history in one list. Entries are deduplicated and filtered on the fly.

Settings
===
Click on CPU load indicator on the top right corner to open settings in your default text editor. There are only a few settings for now, more will come soon.

Terminal grouping
===
Holding Ctrl key and clicking left mouse button on a terminal pane adds it to the list of terminals to send input to. Ctrl-clicking on a selected terminal removes it from the list. When you have several active terminals, everything you type will be sent to all of them simultaneously.

Current platform support
===
- Linux — low bugs probability
- macOS — low bugs probability; GUI is a bit unstable, the rest is okay
- Windows — average bugs probability due to a non-UNIX-like environment

The builds included in "releases" directory **may be slightly outdated**. If you want to get the most recent version, please build it from source. I'll add details on that later.
