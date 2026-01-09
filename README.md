Nisdos Terminal
===
A terminal we use at Nisdos and decided to open-source. It aims at low CPU and memory consumption, high performance, while having all modern features. Built on pure Rust & [SDL3](https://www.libsdl.org/).

Features
===
- flexible layout: movable tabs, resizable panes
- state is saved and loaded automatically
- cross-platform (Linux, Mac, Windows)
- configurable hotkeys and mouse tricks
- explicitly scrollable history
- primary buffer support on Linux
- smart Ctrl+C & Ctrl+V (works when it doesn't interfere with terminal apps)
- sequential hotkeys (built-in example: Alt-G-P -- go to prompt)
- emojis, including modifiers and combined emojis

Next steps
===
- private pane history items
- AI-agent to assist working with OS (not for coding)
- expand settings
- themes
- voice input
- ligatures support
- Kitty protocol support

Settings
===
Click on CPU load indicator on the top right corner to open settings in your default text editor. There are veeery few settings for now, they will come soon.

Current platform support
===
- Linux -- low bugs probability
- macOS -- low bugs probability; GUI is a bit unstable, the rest is okay
- Windows -- average bugs probability due to a non-UNIX-like environment

The builds included in "releases" directory may be slightly outdated. If you want to get the most recent version, please build it from source.
