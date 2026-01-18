use crate::pane_layout::SplitDirection;
use crate::screen_buffer::ScreenBuffer;
use crate::tab_gui::TabBarGui;
use crate::terminal::{Terminal, TerminalLibrary};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

// Default scrollback buffer size for test terminals
const DEFAULT_SCROLLBACK_LINES: usize = 10000;

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum TestCommand {
    #[serde(rename = "key")]
    Key { bytes: Vec<u8> },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "paste")]
    Paste { text: String },
    #[serde(rename = "get_buffer")]
    GetBuffer,
    #[serde(rename = "resize")]
    Resize { width: u32, height: u32 },
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "add_tab")]
    AddTab { name: Option<String> },
    #[serde(rename = "list_tabs")]
    ListTabs,
    #[serde(rename = "close_tab")]
    CloseTab { index: usize },
    #[serde(rename = "switch_tab")]
    SwitchTab { index: usize },
    #[serde(rename = "reorder_tab")]
    ReorderTab { from_index: usize, to_index: usize },
    #[serde(rename = "rename_tab")]
    RenameTab { index: usize, name: String },
    #[serde(rename = "start_editing_tab")]
    StartEditingTab { index: usize },
    #[serde(rename = "finish_editing_tab")]
    FinishEditingTab { index: usize, save: bool },
    #[serde(rename = "simulate_tab_edit_enter")]
    SimulateTabEditEnter { index: usize, new_name: String },
    #[serde(rename = "send_tab_edit_key")]
    SendTabEditKey { key: String },
    #[serde(rename = "send_tab_edit_text")]
    SendTabEditText { text: String },
    #[serde(rename = "split_pane")]
    SplitPane { direction: String }, // "horizontal" or "vertical"
    #[serde(rename = "list_panes")]
    ListPanes,
    #[serde(rename = "close_pane")]
    ClosePane { pane_id: usize },
    #[serde(rename = "switch_pane")]
    SwitchPane { pane_id: usize },
    #[serde(rename = "get_active_pane")]
    GetActivePane,
    #[serde(rename = "mouse_click")]
    MouseClick {
        button: u8,    // 0=left, 1=middle, 2=right
        col: u32,      // 1-based column
        row: u32,      // 1-based row
        pressed: bool, // true for press, false for release
    },
    #[serde(rename = "mouse_move")]
    MouseMove {
        col: u32, // 1-based column
        row: u32, // 1-based row
    },
    #[serde(rename = "get_cwd")]
    GetCwd,
    #[serde(rename = "get_selection")]
    GetSelection,
    #[serde(rename = "scroll_view")]
    ScrollView { lines: i32 }, // Positive = scroll up (back in history), negative = scroll down (forward)
    #[serde(rename = "send_keypress")]
    SendKeypress {
        key: String, // Key name like "G", "P", "PageUp", etc.
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        alt: bool,
    },
    #[serde(rename = "ctrl_mouse_wheel")]
    CtrlMouseWheel { delta: i32 }, // 1 for scroll up (zoom in), -1 for scroll down (zoom out)
}

#[derive(Serialize, Debug)]
pub struct ScreenBufferSnapshot {
    pub width: usize,
    pub height: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub lines: Vec<String>,
    pub cells: Vec<Vec<CellSnapshot>>,
    pub scroll_offset: usize,
    pub scrollback: Vec<Vec<CellSnapshot>>,
    pub cursor_style: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct CellSnapshot {
    pub ch: String,
    pub width: u8,
    pub fg_r: u8,
    pub fg_g: u8,
    pub fg_b: u8,
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub blink: bool,
    pub reverse: bool,
    pub invisible: bool,
}

#[derive(Serialize, Debug)]
pub struct TabInfo {
    pub index: usize,
    pub name: String,
    pub is_active: bool,
}

#[derive(Serialize, Debug)]
pub struct PaneInfo {
    pub pane_id: usize,
    pub is_active: bool,
    pub cols: u32,
    pub rows: u32,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum TestResponse {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "buffer")]
    Buffer { buffer: ScreenBufferSnapshot },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "tab_created")]
    TabCreated { index: usize },
    #[serde(rename = "tabs")]
    Tabs { tabs: Vec<TabInfo> },
    #[serde(rename = "pane_created")]
    PaneCreated { pane_id: usize },
    #[serde(rename = "panes")]
    Panes { panes: Vec<PaneInfo> },
    #[serde(rename = "active_pane")]
    ActivePane { pane_id: usize },
    #[serde(rename = "cwd")]
    Cwd { path: String },
    #[serde(rename = "selection")]
    Selection { text: Option<String> },
}

impl ScreenBufferSnapshot {
    pub fn from_screen_buffer(sb: &ScreenBuffer) -> Self {
        let mut lines = Vec::new();
        let mut cells = Vec::new();

        for y in 0..sb.height() {
            let mut line = String::new();
            let mut row = Vec::new();

            for x in 0..sb.width() {
                if let Some(cell) = sb.get_cell_with_scrollback(x, y) {
                    // Use extended grapheme if present, otherwise use single char
                    let cell_text = if let Some(ref extended) = cell.extended {
                        extended.to_string()
                    } else {
                        cell.ch.to_string()
                    };
                    // Skip continuation cells (width=0) when building the line string
                    // These are the second cell of double-width characters (CJK, emojis, etc.)
                    if cell.width > 0 {
                        line.push_str(&cell_text);
                    }
                    row.push(CellSnapshot {
                        ch: cell_text,
                        width: cell.width,
                        fg_r: cell.fg_color.r,
                        fg_g: cell.fg_color.g,
                        fg_b: cell.fg_color.b,
                        bg_r: cell.bg_color.r,
                        bg_g: cell.bg_color.g,
                        bg_b: cell.bg_color.b,
                        bold: cell.bold,
                        italic: cell.italic,
                        underline: cell.underline,
                        strikethrough: cell.strikethrough,
                        blink: cell.blink,
                        reverse: cell.reverse,
                        invisible: cell.invisible,
                    });
                } else {
                    line.push_str(" ");
                    row.push(CellSnapshot {
                        ch: " ".to_string(),
                        width: 1,
                        fg_r: 255,
                        fg_g: 255,
                        fg_b: 255,
                        bg_r: 0,
                        bg_g: 0,
                        bg_b: 0,
                        bold: false,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                        blink: false,
                        reverse: false,
                        invisible: false,
                    });
                }
            }

            lines.push(line);
            cells.push(row);
        }

        // Capture scrollback buffer
        let mut scrollback = Vec::new();
        for scrollback_row in sb.get_scrollback_buffer() {
            let mut row = Vec::new();
            for cell in scrollback_row {
                let cell_text = if let Some(ref extended) = cell.extended {
                    extended.to_string()
                } else {
                    cell.ch.to_string()
                };
                row.push(CellSnapshot {
                    ch: cell_text,
                    width: cell.width,
                    fg_r: cell.fg_color.r,
                    fg_g: cell.fg_color.g,
                    fg_b: cell.fg_color.b,
                    bg_r: cell.bg_color.r,
                    bg_g: cell.bg_color.g,
                    bg_b: cell.bg_color.b,
                    bold: cell.bold,
                    italic: cell.italic,
                    underline: cell.underline,
                    strikethrough: cell.strikethrough,
                    blink: cell.blink,
                    reverse: cell.reverse,
                    invisible: cell.invisible,
                });
            }
            scrollback.push(row);
        }

        ScreenBufferSnapshot {
            width: sb.width(),
            height: sb.height(),
            cursor_x: sb.cursor_x,
            cursor_y: sb.cursor_y,
            lines,
            cells,
            scroll_offset: sb.scroll_offset,
            scrollback,
            cursor_style: format!("{:?}", sb.cursor_style),
        }
    }
}

pub struct TestServer {
    listener: TcpListener,
    terminals: Arc<Mutex<Vec<Arc<Mutex<Terminal>>>>>,
    active_tab: Arc<Mutex<usize>>,
    tab_bar_gui: Arc<Mutex<TabBarGui>>,
    char_width: f32,
    char_height: f32,
    _tab_bar_height: u32,
    window_width: Arc<Mutex<u32>>,
    window_height: Arc<Mutex<u32>>,
}

impl TestServer {
    pub fn new(
        port: u16,
        terminals: Vec<Arc<Mutex<Terminal>>>,
        tab_bar_gui: Arc<Mutex<TabBarGui>>,
        char_width: f32,
        char_height: f32,
        tab_bar_height: u32,
        window_width: u32,
        window_height: u32,
    ) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
        listener.set_nonblocking(true)?;

        eprintln!("[TEST_SERVER] Listening on 127.0.0.1:{}", port);

        Ok(TestServer {
            listener,
            terminals: Arc::new(Mutex::new(terminals)),
            active_tab: Arc::new(Mutex::new(0)),
            tab_bar_gui,
            char_width,
            char_height,
            _tab_bar_height: tab_bar_height,
            window_width: Arc::new(Mutex::new(window_width)),
            window_height: Arc::new(Mutex::new(window_height)),
        })
    }

    pub fn update_tabs(&self, terminals: Vec<Arc<Mutex<Terminal>>>) {
        if let Ok(mut tabs) = self.terminals.lock() {
            *tabs = terminals;
        }
    }

    pub fn handle_connections(&self) -> Result<bool, std::io::Error> {
        match self.listener.accept() {
            Ok((stream, addr)) => {
                eprintln!("[TEST_SERVER] Connection from {}", addr);
                let should_shutdown = self.handle_client(stream)?;
                Ok(should_shutdown) // Return shutdown flag
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connections pending
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    fn handle_client(&self, mut stream: TcpStream) -> Result<bool, std::io::Error> {
        let mut buffer = vec![0u8; 8192];

        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    eprintln!("[TEST_SERVER] Client disconnected");
                    break;
                }
                Ok(n) => {
                    let data = &buffer[..n];

                    // Try to parse as JSON
                    match serde_json::from_slice::<TestCommand>(data) {
                        Ok(cmd) => {
                            eprintln!("[TEST_SERVER] Received command: {:?}", cmd);

                            let is_shutdown_cmd = matches!(cmd, TestCommand::Shutdown);
                            let response = self.process_command(cmd);

                            let json = serde_json::to_string(&response).unwrap();
                            stream.write_all(json.as_bytes())?;
                            stream.write_all(b"\n")?;
                            stream.flush()?;

                            if is_shutdown_cmd {
                                eprintln!("[TEST_SERVER] Shutdown requested");
                                return Ok(true); // Signal shutdown to main loop
                            }
                        }
                        Err(e) => {
                            eprintln!("[TEST_SERVER] Failed to parse command: {}", e);
                            let response = TestResponse::Error {
                                message: format!("Invalid JSON: {}", e),
                            };
                            let json = serde_json::to_string(&response).unwrap();
                            stream.write_all(json.as_bytes())?;
                            stream.write_all(b"\n")?;
                            stream.flush()?;
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, continue
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    eprintln!("[TEST_SERVER] Error reading from client: {}", e);
                    break;
                }
            }
        }

        Ok(false) // No shutdown requested
    }

    fn process_command(&self, cmd: TestCommand) -> TestResponse {
        match cmd {
            TestCommand::Key { bytes } => {
                if let Ok(gui) = self.tab_bar_gui.lock() {
                    if let Some(terminal) = gui.get_active_terminal() {
                        if let Ok(mut t) = terminal.lock() {
                            t.send_key(&bytes);
                            thread::sleep(std::time::Duration::from_millis(50));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to access terminal".to_string(),
                }
            }
            TestCommand::Text { text } => {
                if let Ok(gui) = self.tab_bar_gui.lock() {
                    if let Some(terminal) = gui.get_active_terminal() {
                        if let Ok(mut t) = terminal.lock() {
                            t.send_text(&text);
                            thread::sleep(std::time::Duration::from_millis(50));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to access terminal".to_string(),
                }
            }
            TestCommand::Paste { text } => {
                if let Ok(gui) = self.tab_bar_gui.lock() {
                    if let Some(terminal) = gui.get_active_terminal() {
                        if let Ok(mut t) = terminal.lock() {
                            t.send_paste(&text);
                            thread::sleep(std::time::Duration::from_millis(50));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to access terminal".to_string(),
                }
            }
            TestCommand::GetBuffer => {
                // Get buffer from active pane (not from tab list)
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        let active_pane_id = pane_layout.active_pane();
                        if let Some(terminal) = pane_layout.root.find_terminal(active_pane_id) {
                            if let Ok(t) = terminal.lock() {
                                if let Ok(screen_buffer) = t.screen_buffer.lock() {
                                    let snapshot = ScreenBufferSnapshot::from_screen_buffer(&screen_buffer);
                                    return TestResponse::Buffer { buffer: snapshot };
                                }
                            }
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to access buffer".to_string(),
                }
            }
            TestCommand::Resize { width, height } => {
                let active_idx = *self.active_tab.lock().unwrap();
                if let Ok(terminals) = self.terminals.lock() {
                    if let Some(terminal) = terminals.get(active_idx) {
                        if let Ok(mut t) = terminal.lock() {
                            t.set_size(width, height, false);
                            thread::sleep(std::time::Duration::from_millis(100));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to resize terminal".to_string(),
                }
            }
            TestCommand::AddTab { name } => {
                let term_library = TerminalLibrary::new();
                let shell_config = term_library.get_default_shell().clone();

                // Use existing terminal dimensions if available
                let (width, height) = if let Ok(terminals) = self.terminals.lock() {
                    if let Some(existing) = terminals.first() {
                        if let Ok(t) = existing.lock() {
                            (t.width, t.height)
                        } else {
                            (80, 24)
                        }
                    } else {
                        (80, 24)
                    }
                } else {
                    (80, 24)
                };

                // Get cwd from active terminal before creating new tab
                let start_dir = if let Ok(gui) = self.tab_bar_gui.lock() {
                    let terminal = gui.get_active_terminal();
                    drop(gui); // Release GUI lock before locking terminal
                    terminal.and_then(|t| t.lock().unwrap().get_cwd())
                } else {
                    None
                };

                let new_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                    width,
                    height,
                    shell_config,
                    DEFAULT_SCROLLBACK_LINES,
                    start_dir,
                    crate::screen_buffer::CursorStyle::default(),
                )));

                // Determine tab name
                let tab_name = if let Some(name) = name {
                    name
                } else if let Ok(gui) = self.tab_bar_gui.lock() {
                    format!("Tab {}", gui.tab_states.len() + 1)
                } else {
                    "New Tab".to_string()
                };

                // Add to TabBarGui
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    gui.add_tab(new_terminal.clone(), tab_name);
                    let new_idx = gui.tab_states.len() - 1;

                    // Update terminals list
                    if let Ok(mut terminals) = self.terminals.lock() {
                        terminals.push(new_terminal);
                    }

                    return TestResponse::TabCreated { index: new_idx };
                }

                TestResponse::Error {
                    message: "Failed to create tab".to_string(),
                }
            }
            TestCommand::RenameTab { index, name } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index < gui.tab_states.len() {
                        gui.tab_states[index].name = name;
                        gui.tab_states[index].temp_name = gui.tab_states[index].name.clone();
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Invalid tab index".to_string(),
                }
            }
            TestCommand::StartEditingTab { index } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index < gui.tab_states.len() {
                        gui.tab_states[index].start_editing();
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Invalid tab index".to_string(),
                }
            }
            TestCommand::FinishEditingTab { index, save } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index < gui.tab_states.len() {
                        gui.tab_states[index].finish_editing(save);
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Invalid tab index".to_string(),
                }
            }
            TestCommand::SendTabEditKey { key } => {
                // Find the tab being edited and apply the key action
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    // Find which tab is being edited
                    let editing_tab = gui.tab_states.iter_mut().find(|tab| tab.is_editing);

                    if let Some(tab) = editing_tab {
                        match key.as_str() {
                            "Left" => tab.move_cursor_left(),
                            "Right" => tab.move_cursor_right(),
                            "Delete" => tab.delete_char_at_cursor(),
                            "Backspace" => tab.backspace_at_cursor(),
                            "Return" | "Enter" => {
                                tab.finish_editing(true);
                                return TestResponse::Ok;
                            }
                            "Escape" => {
                                tab.finish_editing(false);
                                return TestResponse::Ok;
                            }
                            _ => {
                                return TestResponse::Error {
                                    message: format!("Unknown key: {}", key),
                                };
                            }
                        }
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "No tab is being edited".to_string(),
                }
            }
            TestCommand::SendTabEditText { text } => {
                // Find the tab being edited and insert text at cursor
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    let editing_tab = gui.tab_states.iter_mut().find(|tab| tab.is_editing);

                    if let Some(tab) = editing_tab {
                        tab.insert_text_at_cursor(&text);
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "No tab is being edited".to_string(),
                }
            }
            TestCommand::SimulateTabEditEnter { index, new_name } => {
                // This simulates the sequence: start editing -> type new name -> press Enter
                // The bug is that after this sequence, text input is stopped but not restarted
                // This command helps us test by directly setting the final state
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index < gui.tab_states.len() {
                        // Start editing
                        gui.tab_states[index].start_editing();
                        // Set the new name as if user typed it
                        gui.tab_states[index].temp_name = new_name.clone();
                        // Finish editing (save) - as if Enter was pressed
                        gui.tab_states[index].finish_editing(true);
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Invalid tab index".to_string(),
                }
            }
            TestCommand::ListTabs => {
                if let Ok(gui) = self.tab_bar_gui.lock() {
                    let tabs: Vec<TabInfo> = gui
                        .tab_states
                        .iter()
                        .enumerate()
                        .map(|(idx, tab_state)| TabInfo {
                            index: idx,
                            name: tab_state.name.clone(),
                            is_active: idx == gui.active_tab,
                        })
                        .collect();
                    return TestResponse::Tabs { tabs };
                }
                TestResponse::Error {
                    message: "Failed to list tabs".to_string(),
                }
            }
            TestCommand::CloseTab { index } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index >= gui.tab_states.len() {
                        return TestResponse::Error {
                            message: "Invalid tab index".to_string(),
                        };
                    }

                    // Remove from GUI
                    if gui.remove_tab(index) {
                        // Last tab closed
                        eprintln!("[TEST_SERVER] Last tab closed, shutting down");
                        std::process::exit(0);
                    }

                    // Update terminals list
                    if let Ok(mut terminals) = self.terminals.lock() {
                        if index < terminals.len() {
                            terminals.remove(index);
                        }
                    }

                    let mut active_idx = self.active_tab.lock().unwrap();
                    if *active_idx >= gui.tab_states.len() {
                        *active_idx = gui.tab_states.len() - 1;
                    }

                    return TestResponse::Ok;
                }
                TestResponse::Error {
                    message: "Failed to close tab".to_string(),
                }
            }
            TestCommand::SwitchTab { index } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if index >= gui.tab_states.len() {
                        return TestResponse::Error {
                            message: "Invalid tab index".to_string(),
                        };
                    }

                    gui.set_active_tab(index);
                    *self.active_tab.lock().unwrap() = index;

                    // Resize terminals in the newly active tab to match their pane dimensions
                    // This ensures terminals that were inactive get properly sized
                    if let Some(pane_layout) = gui.tab_states.get(gui.active_tab) {
                        let window_width = *self.window_width.lock().unwrap();
                        let window_height = *self.window_height.lock().unwrap();
                        let tab_bar_height = self._tab_bar_height;
                        let pane_area_height = window_height.saturating_sub(tab_bar_height);
                        let pane_rects = pane_layout.pane_layout.get_pane_rects(0, tab_bar_height as i32, window_width, pane_area_height);

                        for (_pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
                            let (cols, rows) = crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), self.char_width, self.char_height);

                            if let Ok(mut t) = terminal.lock() {
                                if t.width != cols || t.height != rows {
                                    eprintln!("[TEST_SERVER] SwitchTab: Resizing terminal from {}x{} to {}x{}", t.width, t.height, cols, rows);
                                    t.set_size(cols, rows, false);
                                }
                            }
                        }
                    }

                    return TestResponse::Ok;
                }
                TestResponse::Error {
                    message: "Failed to switch tab".to_string(),
                }
            }
            TestCommand::ReorderTab { from_index, to_index } => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if from_index >= gui.tab_states.len() || to_index >= gui.tab_states.len() {
                        return TestResponse::Error {
                            message: "Invalid tab index".to_string(),
                        };
                    }

                    gui.reorder_tab(from_index, to_index);
                    return TestResponse::Ok;
                }
                TestResponse::Error {
                    message: "Failed to reorder tab".to_string(),
                }
            }
            TestCommand::Shutdown => TestResponse::Ok,
            TestCommand::SplitPane { direction } => {
                let split_dir = match direction.as_str() {
                    "horizontal" => SplitDirection::Horizontal,
                    "vertical" => SplitDirection::Vertical,
                    _ => {
                        return TestResponse::Error {
                            message: format!("Invalid direction: {}", direction),
                        }
                    }
                };

                if let Ok(gui) = self.tab_bar_gui.lock() {
                    let term_library = TerminalLibrary::new();
                    let shell_config = term_library.get_default_shell().clone();

                    // Use actual window dimensions instead of calculating from terminal size
                    let (width, height, window_width, window_height) = if let Some(active_term) = gui.get_active_terminal() {
                        if let Ok(t) = active_term.lock() {
                            let term_cols = t.width;
                            let term_rows = t.height;
                            // Use stored window dimensions
                            let win_width_pixels = *self.window_width.lock().unwrap();
                            let win_height_pixels = *self.window_height.lock().unwrap();
                            eprintln!(
                                "[TEST_SERVER] Split: terminal={}x{}, char_size={:.2}x{:.2}, window={}x{}",
                                term_cols, term_rows, self.char_width, self.char_height, win_width_pixels, win_height_pixels
                            );
                            (term_cols, term_rows, win_width_pixels, win_height_pixels)
                        } else {
                            let win_width = *self.window_width.lock().unwrap();
                            let win_height = *self.window_height.lock().unwrap();
                            (80, 24, win_width, win_height)
                        }
                    } else {
                        let win_width = *self.window_width.lock().unwrap();
                        let win_height = *self.window_height.lock().unwrap();
                        (80, 24, win_width, win_height)
                    };

                    // Get cwd from active terminal before splitting
                    let start_dir = {
                        let terminal = gui.get_active_terminal();
                        drop(gui); // Release GUI lock before locking terminal
                        terminal.and_then(|t| t.lock().unwrap().get_cwd())
                    };
                    let mut gui = self.tab_bar_gui.lock().unwrap();

                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        // Check if the pane is large enough to split
                        let tab_bar_height = self._tab_bar_height;
                        let pane_area_height = window_height.saturating_sub(tab_bar_height);
                        let pane_rects = pane_layout.get_pane_rects(0, tab_bar_height as i32, window_width, pane_area_height);

                        // Find the active pane's dimensions
                        let can_split = if let Some((_, rect, _, _, _)) = pane_rects.iter().find(|(id, _, _, _, _)| *id == pane_layout.active_pane) {
                            let (current_cols, current_rows) =
                                crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), self.char_width, self.char_height);
                            eprintln!(
                                "[TEST_SERVER] Split: pane rect={}x{}, calculated={}x{} chars",
                                rect.width(),
                                rect.height(),
                                current_cols,
                                current_rows
                            );

                            // Calculate dimensions after split (accounting for 2-pixel divider)
                            let divider_chars_h = (2.0 / self.char_width).ceil() as u32;
                            let divider_chars_v = (2.0 / self.char_height).ceil() as u32;

                            match split_dir {
                                SplitDirection::Horizontal => {
                                    // Each pane will be roughly half width
                                    let split_width = (current_cols.saturating_sub(divider_chars_h)) / 2;
                                    eprintln!(
                                        "[TEST_SERVER] Horizontal split: current_cols={}, divider_chars_h={}, split_width={}",
                                        current_cols, divider_chars_h, split_width
                                    );
                                    if split_width >= 10 && current_rows >= 5 {
                                        true
                                    } else {
                                        eprintln!(
                                            "[TEST_SERVER] Cannot split horizontally: resulting width {} would be less than 10 chars",
                                            split_width
                                        );
                                        false
                                    }
                                }
                                SplitDirection::Vertical => {
                                    // Each pane will be roughly half height
                                    let split_height = (current_rows.saturating_sub(divider_chars_v)) / 2;
                                    eprintln!(
                                        "[TEST_SERVER] Vertical split: current_rows={}, divider_chars_v={}, split_height={}",
                                        current_rows, divider_chars_v, split_height
                                    );
                                    if split_height >= 5 && current_cols >= 10 {
                                        true
                                    } else {
                                        eprintln!(
                                            "[TEST_SERVER] Cannot split vertically: resulting height {} would be less than 5 chars",
                                            split_height
                                        );
                                        false
                                    }
                                }
                            }
                        } else {
                            false
                        };

                        if !can_split {
                            return TestResponse::Error {
                                message: "Pane too small to split (minimum: 10 chars wide, 5 chars tall)".to_string(),
                            };
                        }

                        let new_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                            width,
                            height,
                            shell_config,
                            DEFAULT_SCROLLBACK_LINES,
                            start_dir,
                            crate::screen_buffer::CursorStyle::default(),
                        )));

                        pane_layout.split_active_pane(split_dir, new_terminal.clone());
                        // Update terminals list
                        if let Ok(mut terminals) = self.terminals.lock() {
                            terminals.push(new_terminal.clone());
                        }

                        // Get the active pane ID (which is the newly created pane after split)
                        let new_pane_id = pane_layout.active_pane();

                        // Resize all terminals to match their new pane dimensions
                        let tab_bar_height = self._tab_bar_height;
                        let pane_area_height = window_height.saturating_sub(tab_bar_height);
                        let pane_rects = pane_layout.get_pane_rects(0, tab_bar_height as i32, window_width, pane_area_height);
                        eprintln!("[TEST_SERVER] Resizing {} terminals after split", pane_rects.len());

                        for (pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
                            let cols = (rect.width() as f32 / self.char_width).floor() as u32;
                            let rows = (rect.height() as f32 / self.char_height).floor() as u32;

                            if let Ok(mut t) = terminal.lock() {
                                if t.width != cols || t.height != rows {
                                    // Only clear screen for the newly created pane, not existing ones
                                    let clear_screen = pane_id == new_pane_id;
                                    eprintln!(
                                        "[TEST_SERVER] Pane {:?}: {}x{} -> {}x{} (clear={})",
                                        pane_id, t.width, t.height, cols, rows, clear_screen
                                    );
                                    t.set_size(cols, rows, clear_screen);
                                } else {
                                    eprintln!("[TEST_SERVER] Pane {:?}: already {}x{}", pane_id, cols, rows);
                                }
                            }
                        }

                        // Return the new pane ID
                        return TestResponse::PaneCreated { pane_id: new_pane_id.0 };
                    }
                }
                TestResponse::Error {
                    message: "Failed to split pane".to_string(),
                }
            }
            TestCommand::ListPanes => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        let active_pane_id = pane_layout.active_pane();
                        let terminals_with_ids = pane_layout.root.collect_terminals_with_ids();
                        let panes: Vec<PaneInfo> = terminals_with_ids
                            .iter()
                            .map(|(id, terminal)| {
                                let (cols, rows) = if let Ok(t) = terminal.lock() { (t.width, t.height) } else { (80, 24) };
                                PaneInfo {
                                    pane_id: id.0,
                                    is_active: *id == active_pane_id,
                                    cols,
                                    rows,
                                }
                            })
                            .collect();
                        return TestResponse::Panes { panes };
                    }
                }
                TestResponse::Panes { panes: vec![] }
            }
            TestCommand::ClosePane { pane_id } => {
                use crate::pane_layout::PaneId;

                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        let was_last = pane_layout.close_pane(PaneId(pane_id));
                        if was_last {
                            return TestResponse::Error {
                                message: "Cannot close last pane".to_string(),
                            };
                        }
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Failed to close pane".to_string(),
                }
            }
            TestCommand::SwitchPane { pane_id } => {
                use crate::pane_layout::PaneId;

                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        pane_layout.set_active_pane(PaneId(pane_id));
                        return TestResponse::Ok;
                    }
                }
                TestResponse::Error {
                    message: "Failed to switch pane".to_string(),
                }
            }
            TestCommand::GetActivePane => {
                if let Ok(mut gui) = self.tab_bar_gui.lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        let active_pane_id = pane_layout.active_pane();
                        return TestResponse::ActivePane { pane_id: active_pane_id.0 };
                    }
                }
                TestResponse::ActivePane { pane_id: 0 }
            }
            TestCommand::MouseClick { button, col, row, pressed } => {
                let active_idx = *self.active_tab.lock().unwrap();
                if let Ok(terminals) = self.terminals.lock() {
                    if let Some(terminal) = terminals.get(active_idx) {
                        if let Ok(mut t) = terminal.lock() {
                            // Handle selection for left mouse button (button 0)
                            if button == 0 {
                                let cell_col = (col - 1) as usize;
                                let cell_row = (row - 1) as usize;
                                if pressed {
                                    // Start selection on mouse down
                                    t.start_selection(cell_col, cell_row);
                                } else {
                                    // Mouse up - check if this is a single click (no drag)
                                    let selection = *t.selection.lock().unwrap();
                                    if let Some(sel) = selection {
                                        if sel.start_col == cell_col && sel.start_row == cell_row && sel.end_col == cell_col && sel.end_row == cell_row {
                                            // Single point selection (no drag) - clear it
                                            t.clear_selection();
                                        } else {
                                            // Actual drag - update the end point
                                            t.update_selection(cell_col, cell_row);

                                            // Copy selection to PRIMARY clipboard on Linux
                                            #[cfg(target_os = "linux")]
                                            {
                                                if let Some(text) = t.get_selected_text() {
                                                    if !text.is_empty() {
                                                        use arboard::{Clipboard, LinuxClipboardKind, SetExtLinux};
                                                        drop(t); // Drop terminal lock

                                                        // Store clipboard in PaneLayout to keep PRIMARY selection alive
                                                        if let Ok(mut gui) = self.tab_bar_gui.lock() {
                                                            if let Some(pane_layout) = gui.get_active_pane_layout() {
                                                                match Clipboard::new() {
                                                                    Ok(mut clipboard) => {
                                                                        let text_copy = text.clone();
                                                                        if let Err(e) = clipboard.set().clipboard(LinuxClipboardKind::Primary).text(text_copy) {
                                                                            eprintln!("[TEST_SERVER] Failed to copy to PRIMARY: {}", e);
                                                                        } else {
                                                                            pane_layout.primary_clipboard = Some(clipboard);
                                                                            eprintln!("[TEST_SERVER] Copied to PRIMARY clipboard: {} chars", text.len());
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        eprintln!("[TEST_SERVER] Failed to create clipboard: {}", e);
                                                                    }
                                                                }
                                                            }
                                                        }

                                                        thread::sleep(std::time::Duration::from_millis(50));
                                                        return TestResponse::Ok;
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        // No selection active, just update
                                        t.update_selection(cell_col, cell_row);
                                    }
                                }
                            }

                            // Also send mouse event for applications that use mouse tracking
                            t.send_mouse_event(button, col, row, pressed);
                            thread::sleep(std::time::Duration::from_millis(50));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to send mouse click".to_string(),
                }
            }
            TestCommand::MouseMove { col, row } => {
                // Mouse move is used during selection to update the selection end point
                // We'll handle this by getting the active terminal and updating its selection
                let active_idx = *self.active_tab.lock().unwrap();
                if let Ok(terminals) = self.terminals.lock() {
                    if let Some(terminal) = terminals.get(active_idx) {
                        if let Ok(mut t) = terminal.lock() {
                            // Convert terminal coordinates to cell coordinates (0-based)
                            let cell_col = (col - 1) as usize;
                            let cell_row = (row - 1) as usize;
                            t.update_selection(cell_col, cell_row);
                            thread::sleep(std::time::Duration::from_millis(10));
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to handle mouse move".to_string(),
                }
            }
            TestCommand::GetSelection => {
                let active_idx = *self.active_tab.lock().unwrap();
                if let Ok(terminals) = self.terminals.lock() {
                    if let Some(terminal) = terminals.get(active_idx) {
                        if let Ok(t) = terminal.lock() {
                            let text = t.get_selected_text();
                            return TestResponse::Selection { text };
                        }
                    }
                }
                TestResponse::Selection { text: None }
            }
            TestCommand::GetCwd => {
                let active_idx = *self.active_tab.lock().unwrap();
                if let Ok(terminals) = self.terminals.lock() {
                    if let Some(terminal) = terminals.get(active_idx) {
                        if let Ok(t) = terminal.lock() {
                            if let Some(cwd) = t.get_cwd() {
                                if let Some(path_str) = cwd.to_str() {
                                    return TestResponse::Cwd { path: path_str.to_string() };
                                }
                            }
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to get cwd".to_string(),
                }
            }

            TestCommand::ScrollView { lines } => {
                if let Ok(gui) = self.tab_bar_gui.lock() {
                    if let Some(terminal) = gui.get_active_terminal() {
                        if let Ok(t) = terminal.lock() {
                            let mut screen_buffer = t.screen_buffer.lock().unwrap();
                            if lines > 0 {
                                screen_buffer.scroll_view_up(lines as usize);
                            } else if lines < 0 {
                                screen_buffer.scroll_view_down((-lines) as usize);
                            }
                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to scroll view".to_string(),
                }
            }
            TestCommand::SendKeypress { key, ctrl, shift, alt } => {
                // This command simulates a keypress with modifiers
                // It's useful for testing sequential hotkeys and other keyboard shortcuts
                // Note: This doesn't actually inject SDL events, it directly calls the hotkey logic

                // For now, we'll directly manipulate scroll_offset for testing the Go To Prompt functionality
                // A full implementation would require injecting SDL events into the event loop

                // Check if this is the Alt-G-P sequence for "go to prompt"
                if alt && !ctrl && !shift && key == "G" {
                    // First key of sequence - just return ok
                    return TestResponse::Ok;
                } else if key == "P" {
                    // Second key - reset scroll offset
                    if let Ok(gui) = self.tab_bar_gui.lock() {
                        if let Some(terminal) = gui.get_active_terminal() {
                            if let Ok(t) = terminal.lock() {
                                t.screen_buffer.lock().unwrap().reset_view_offset();
                                return TestResponse::Ok;
                            }
                        }
                    }
                    return TestResponse::Error {
                        message: "Failed to reset view offset".to_string(),
                    };
                }

                TestResponse::Error {
                    message: format!("Keypress simulation not fully implemented for key: {}", key),
                }
            }
            TestCommand::CtrlMouseWheel { delta } => {
                // Simulate font size change by resizing terminal
                // When zooming in (delta > 0), font gets bigger, so terminal gets smaller (fewer cols/rows)
                // When zooming out (delta < 0), font gets smaller, so terminal gets larger (more cols/rows)

                if let Ok(gui) = self.tab_bar_gui.lock() {
                    if let Some(terminal) = gui.get_active_terminal() {
                        if let Ok(mut t) = terminal.lock() {
                            let current_width = t.width;
                            let current_height = t.height;

                            // Simulate font size change: reduce dimensions by ~10% per zoom level
                            // In reality, increasing font by 1pt might reduce cols/rows by various amounts
                            // depending on the font size, but we'll use a fixed percentage for testing
                            let scale_factor = if delta > 0 { 0.9 } else { 1.1 };

                            let new_width = ((current_width as f32) * scale_factor).max(20.0) as u32;
                            let new_height = ((current_height as f32) * scale_factor).max(10.0) as u32;

                            eprintln!(
                                "[TEST_SERVER] Simulating zoom: {}x{} -> {}x{}",
                                current_width, current_height, new_width, new_height
                            );

                            // Don't clear screen when resizing due to zoom
                            t.set_size(new_width, new_height, false);

                            return TestResponse::Ok;
                        }
                    }
                }
                TestResponse::Error {
                    message: "Failed to simulate zoom".to_string(),
                }
            }
        }
    }
}
