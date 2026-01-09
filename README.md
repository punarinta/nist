Nisdos Terminal
===
A terminal we use at Nisdos and decided to open-source. It aims at low CPU and memory consumption, high performance, while having all modern features. Built on pure Rust & [SDL3](https://www.libsdl.org/).

Features
===
- flexible layout: movable tabs, resizable panes
- extensive state, restored on startup
- cross-platform (Linux, Mac, Windows)
- configurable hotkeys and mouse tricks
- scrollable output history
- primary buffer support on Linux
- smart Ctrl+C & Ctrl+V (works when it doesn't interfere with terminal apps)
- sequential hotkeys (built-in example: Alt-G-P -- go to prompt)
- emojis, including modifiers and combined emojis
- multiterminal typing (send input to several terminals simultaneously)
- terminal keeps own output history in state

Next steps
===
- keep terminal's own input history in state
- AI-agent to assist working with OS (not for coding)
- further expand settings
- voice input
- JSON themes
- ligatures support
- Kitty protocol support

State
===
State includes layout structure, relative pane sizes, current working directory, tab names, a few of last commands of each terminal and some amount of previous output of each terminal. State is saved and loaded automatically.

Settings
===
Click on CPU load indicator on the top right corner to open settings in your default text editor. There are only a few settings for now, more will come soon.

Sending input to several terminals simultaneously
===
Holding Ctrl key and mouse clicking on a terminal pane adds it to the list of terminals to send input to. Ctrl-clicking on a selected terminal removes it from the list. When you have several active terminals, everything you type will be sent to all of them simultaneously.

Current platform support
===
- Linux -- low bugs probability
- macOS -- low bugs probability; GUI is a bit unstable, the rest is okay
- Windows -- average bugs probability due to a non-UNIX-like environment

The builds included in "releases" directory may be slightly outdated. If you want to get the most recent version, please build it from source. I'll add details on that later.
