use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

use crate::screen_buffer::ScreenBuffer;
use crate::terminal_config::ShellConfig;

// History persistence limits
const MAX_COMMAND_HISTORY: usize = 5; // Maximum number of commands to keep in history
const MAX_OUTPUT_HISTORY: usize = 100; // Maximum number of output lines to keep in history

pub(crate) struct Terminal {
    master: Box<dyn portable_pty::MasterPty>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    child: Box<dyn portable_pty::Child>,
    pub(crate) screen_buffer: Arc<Mutex<ScreenBuffer>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) shell_config: ShellConfig,
    pub(crate) application_cursor_keys: Arc<Mutex<bool>>,
    pub(crate) mouse_tracking_mode: Arc<Mutex<MouseTrackingMode>>,
    pub(crate) mouse_sgr_mode: Arc<Mutex<bool>>,
    pub(crate) selection: Arc<Mutex<Option<Selection>>>,
    pub(crate) bracketed_paste_mode: Arc<Mutex<bool>>,
    // History tracking for state persistence
    pub(crate) command_history: Arc<Mutex<Vec<String>>>, // Last MAX_COMMAND_HISTORY commands
    pub(crate) output_history: Arc<Mutex<Vec<String>>>,  // Last MAX_OUTPUT_HISTORY output lines
    pub(crate) current_command: Arc<Mutex<String>>,      // Current command being typed
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Selection {
    pub start_col: usize,
    pub start_row: usize,
    pub end_col: usize,
    pub end_row: usize,
}

impl Selection {
    pub fn new(col: usize, row: usize) -> Self {
        Selection {
            start_col: col,
            start_row: row,
            end_col: col,
            end_row: row,
        }
    }

    pub fn update_end(&mut self, col: usize, row: usize) {
        self.end_col = col;
        self.end_row = row;
    }

    /// Get normalized selection bounds (start always before end)
    pub fn normalized(&self) -> (usize, usize, usize, usize) {
        if self.start_row < self.end_row || (self.start_row == self.end_row && self.start_col <= self.end_col) {
            (self.start_col, self.start_row, self.end_col, self.end_row)
        } else {
            (self.end_col, self.end_row, self.start_col, self.start_row)
        }
    }

    /// Check if a cell is within the selection
    pub fn contains(&self, col: usize, row: usize) -> bool {
        let (start_col, start_row, end_col, end_row) = self.normalized();

        if row < start_row || row > end_row {
            return false;
        }

        if row == start_row && row == end_row {
            // Single line selection
            col >= start_col && col <= end_col
        } else if row == start_row {
            // First line of multi-line selection
            col >= start_col
        } else if row == end_row {
            // Last line of multi-line selection
            col <= end_col
        } else {
            // Middle lines of multi-line selection
            true
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MouseTrackingMode {
    Disabled,       // No mouse tracking
    X10,            // ESC[?9h - X10 mouse tracking
    VT200Normal,    // ESC[?1000h - Normal tracking (press/release)
    VT200Highlight, // ESC[?1001h - Highlight tracking
    ButtonEvent,    // ESC[?1002h - Button-event tracking (drag)
    AnyEvent,       // ESC[?1003h - Any-event tracking (motion)
}

impl Terminal {
    pub(crate) fn new_with_scrollback(
        initial_width: u32,
        initial_height: u32,
        shell_config: ShellConfig,
        scrollback_limit: usize,
        start_directory: Option<std::path::PathBuf>,
    ) -> Self {
        let pty_system = native_pty_system();

        // Create PTY with initial size
        let pty_size = PtySize {
            rows: initial_height as u16,
            cols: initial_width as u16,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_pair = pty_system.openpty(pty_size).expect("Failed to create PTY pair");

        eprintln!("[TERMINAL] PTY created with initial size: {}x{}", initial_width, initial_height);

        // Set up the command
        let mut cmd = CommandBuilder::new(&shell_config.command);

        // Add command arguments (e.g., -NoLogo for PowerShell)
        for arg in &shell_config.args {
            cmd.arg(arg);
        }

        // Set environment variables
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", initial_width.to_string());
        cmd.env("LINES", initial_height.to_string());

        // Set working directory if specified
        if let Some(dir) = start_directory {
            cmd.cwd(dir);
        }

        // Spawn the child process
        let child = pty_pair.slave.spawn_command(cmd).expect("Failed to spawn shell process");

        eprintln!("[TERMINAL] Shell process spawned: {}", shell_config.command);

        // Create screen buffer with scrollback
        let screen_buffer = Arc::new(Mutex::new(ScreenBuffer::new_with_scrollback(
            initial_width as usize,
            initial_height as usize,
            scrollback_limit,
        )));

        let screen_buffer_clone = Arc::clone(&screen_buffer);
        let saved_screen_buffer = Arc::new(Mutex::new(Vec::new()));
        let saved_screen_buffer_clone = Arc::clone(&saved_screen_buffer);

        // Create shared state
        let application_cursor_keys = Arc::new(Mutex::new(false));
        let mouse_tracking_mode = Arc::new(Mutex::new(MouseTrackingMode::Disabled));
        let mouse_sgr_mode = Arc::new(Mutex::new(false));
        let bracketed_paste_mode = Arc::new(Mutex::new(false));

        let application_cursor_keys_clone = Arc::clone(&application_cursor_keys);
        let mouse_tracking_mode_clone = Arc::clone(&mouse_tracking_mode);
        let mouse_sgr_mode_clone = Arc::clone(&mouse_sgr_mode);
        let bracketed_paste_mode_clone = Arc::clone(&bracketed_paste_mode);

        // Get a reader from the master side
        let mut reader = pty_pair.master.try_clone_reader().expect("Failed to clone PTY reader");

        // Get a writer from the master side and wrap in Arc<Mutex> for sharing
        let writer = pty_pair.master.take_writer().expect("Failed to get PTY writer");
        let writer = Arc::new(Mutex::new(writer));
        let thread_writer = Arc::clone(&writer);

        // Store the master separately
        let master = pty_pair.master;

        // Spawn thread to read PTY output
        thread::spawn(move || {
            let mut buffer = [0; 20000];
            let mut incomplete_sequence = String::new();

            loop {
                match reader.read(&mut buffer) {
                    Ok(bytes_read) if bytes_read > 0 => {
                        let mut text = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

                        // Prepend any incomplete sequence from previous read
                        if !incomplete_sequence.is_empty() {
                            text = incomplete_sequence.clone() + &text;
                            incomplete_sequence.clear();
                        }

                        // Parse escape sequences to detect mode changes
                        Self::parse_mode_sequences(
                            &text,
                            &application_cursor_keys_clone,
                            &mouse_tracking_mode_clone,
                            &mouse_sgr_mode_clone,
                            &bracketed_paste_mode_clone,
                        );

                        // Process the output and get any incomplete sequence (pass writer for DSR responses)
                        incomplete_sequence = Self::process_output(&text, &screen_buffer_clone, &saved_screen_buffer_clone, &thread_writer);

                        if !incomplete_sequence.is_empty() {
                            eprintln!(
                                "[TERMINAL] Saved incomplete sequence: {:?} (len={})",
                                incomplete_sequence.chars().take(20).collect::<String>(),
                                incomplete_sequence.len()
                            );
                        }
                    }
                    Ok(_) => {
                        // 0 bytes read, likely EOF
                        eprintln!("[TERMINAL] PTY reader received EOF");
                        break;
                    }
                    Err(err) => {
                        eprintln!("[TERMINAL] Error reading from PTY: {}", err);
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });

        Terminal {
            master,
            writer,
            child,
            screen_buffer,
            width: initial_width,
            height: initial_height,
            shell_config,
            application_cursor_keys,
            mouse_tracking_mode,
            mouse_sgr_mode,
            selection: Arc::new(Mutex::new(None)),
            bracketed_paste_mode,
            command_history: Arc::new(Mutex::new(Vec::new())),
            output_history: Arc::new(Mutex::new(Vec::new())),
            current_command: Arc::new(Mutex::new(String::new())),
        }
    }

    pub(crate) fn set_size(&mut self, new_width: u32, new_height: u32, clear_screen: bool) {
        self.width = new_width;
        self.height = new_height;

        let new_size = PtySize {
            rows: new_height as u16,
            cols: new_width as u16,
            pixel_width: 0,
            pixel_height: 0,
        };

        if let Err(err) = self.master.resize(new_size) {
            eprintln!("[TERMINAL] Failed to resize PTY: {}", err);
        } else {
            eprintln!("[TERMINAL] Resized PTY to {}x{}", new_width, new_height);
        }

        // Update screen buffer size
        if let Ok(mut sb) = self.screen_buffer.lock() {
            sb.resize(new_width as usize, new_height as usize);

            // Clear the screen buffer if requested (e.g., after pane split)
            // This prevents stale content from appearing in the newly resized pane
            if clear_screen {
                sb.clear_screen();
                eprintln!("[TERMINAL] Cleared screen buffer after resize");
            }
        }
    }

    pub(crate) fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false, // Process has exited
            Ok(None) => true,     // Process is still running
            Err(_) => false,      // Error checking status, assume dead
        }
    }

    pub(crate) fn kill(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.child.kill()?;
        Ok(())
    }

    pub(crate) fn send_key(&mut self, keys: &[u8]) {
        // Detect Enter key to capture command for history
        let is_enter = keys.len() == 1 && keys[0] == b'\r';

        if is_enter {
            // Save current command to history and clear the buffer
            if let Ok(mut current_cmd) = self.current_command.lock() {
                let cmd = current_cmd.trim().to_string();
                if !cmd.is_empty() {
                    self.add_command_to_history(cmd);
                }
                current_cmd.clear();
            }
        }

        // Check if we need to translate arrow keys for application cursor keys mode
        let app_cursor_mode = *self.application_cursor_keys.lock().unwrap();

        // Detect if this is an arrow key sequence: ESC [ A/B/C/D
        let is_arrow_key = keys.len() == 3
            && keys[0] == 27  // ESC
            && keys[1] == b'['
            && (keys[2] == b'A' || keys[2] == b'B' || keys[2] == b'C' || keys[2] == b'D');

        if let Ok(mut writer) = self.writer.lock() {
            if app_cursor_mode && is_arrow_key {
                // Translate ESC [ X to ESC O X for application cursor keys mode
                let translated = [27, b'O', keys[2]];
                if let Err(err) = writer.write_all(&translated) {
                    eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
                }
            } else {
                // Send keys as-is
                if let Err(err) = writer.write_all(keys) {
                    eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
                }
            }
        }
    }

    pub(crate) fn send_text(&mut self, text: &str) {
        // Accumulate text in current command buffer (before converting \n to \r)
        if let Ok(mut current_cmd) = self.current_command.lock() {
            // Only accumulate if text doesn't contain newline (Enter)
            if !text.contains('\n') && !text.contains('\r') {
                current_cmd.push_str(text);
            } else {
                // If text contains Enter, save the command and clear buffer
                // Split on newlines and process each part
                let parts: Vec<&str> = text.split(|c| c == '\n' || c == '\r').collect();
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 && !current_cmd.is_empty() {
                        // Save previous command before newline
                        let cmd = current_cmd.trim().to_string();
                        if !cmd.is_empty() {
                            self.add_command_to_history(cmd);
                        }
                        current_cmd.clear();
                    }
                    if !part.is_empty() {
                        current_cmd.push_str(part);
                    }
                }
            }
        }

        if let Ok(mut writer) = self.writer.lock() {
            // Convert \n to \r for terminal input (newline -> carriage return)
            let converted = text.replace('\n', "\r");
            if let Err(err) = writer.write_all(converted.as_bytes()) {
                eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
            }
        }
    }

    pub(crate) fn send_paste(&mut self, text: &str) {
        if let Ok(mut writer) = self.writer.lock() {
            // Check if bracketed paste mode is enabled
            let bracketed_paste = self.bracketed_paste_mode.lock().map(|mode| *mode).unwrap_or(false);

            if bracketed_paste {
                // Wrap text with bracketed paste sequences
                // Start: ESC[200~
                if let Err(err) = writer.write_all(b"\x1b[200~") {
                    eprintln!("[TERMINAL] Failed to write bracketed paste start: {}", err);
                    return;
                }

                // Send the text with newlines converted to \r
                let converted = text.replace('\n', "\r");
                if let Err(err) = writer.write_all(converted.as_bytes()) {
                    eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
                    return;
                }

                // End: ESC[201~
                if let Err(err) = writer.write_all(b"\x1b[201~") {
                    eprintln!("[TERMINAL] Failed to write bracketed paste end: {}", err);
                }
            } else {
                // No bracketed paste mode - just send with newlines converted
                let converted = text.replace('\n', "\r");
                if let Err(err) = writer.write_all(converted.as_bytes()) {
                    eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
                }
            }
        }
    }

    pub(crate) fn has_process_exited(&mut self) -> bool {
        !self.is_alive()
    }

    /// Send mouse event to the terminal in the appropriate format
    pub(crate) fn send_mouse_event(&mut self, button: u8, col: u32, row: u32, pressed: bool) {
        let Ok(tracking_mode_guard) = self.mouse_tracking_mode.try_lock() else {
            return; // Lock busy, skip mouse event
        };
        let tracking_mode = *tracking_mode_guard;
        drop(tracking_mode_guard);

        let Ok(sgr_mode_guard) = self.mouse_sgr_mode.try_lock() else {
            return; // Lock busy, skip mouse event
        };
        let sgr_mode = *sgr_mode_guard;
        drop(sgr_mode_guard);

        if tracking_mode == MouseTrackingMode::Disabled {
            return;
        }

        // Ensure coordinates are within valid range (1-based, max 223 for non-SGR)
        let col = col.max(1).min(if sgr_mode { 9999 } else { 223 });
        let row = row.max(1).min(if sgr_mode { 9999 } else { 223 });

        let sequence = if sgr_mode {
            // SGR extended format: ESC[<button;col;row;M or m
            // M for press, m for release
            let terminator = if pressed { 'M' } else { 'm' };
            format!("\x1b[<{};{};{}{}", button, col, row, terminator)
        } else {
            // Normal format: ESC[M<button><col><row>
            // Encode button and coordinates as (value + 32) for printable ASCII
            let btn_char = (button + 32) as char;
            let col_char = (col as u8 + 32) as char;
            let row_char = (row as u8 + 32) as char;
            format!("\x1b[M{}{}{}", btn_char, col_char, row_char)
        };

        eprintln!("[MOUSE] Sending sequence: {:?}", sequence);
        if let Ok(mut writer) = self.writer.lock() {
            if let Err(e) = writer.write_all(sequence.as_bytes()) {
                eprintln!("[TERMINAL] Failed to write mouse event to PTY: {}", e);
            }
        }
    }

    /// Start a new text selection at the given position
    pub(crate) fn start_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection::new(col, row));
        }
    }

    /// Update the end position of the current selection
    pub(crate) fn update_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut selection) = self.selection.try_lock() {
            if let Some(ref mut sel) = *selection {
                sel.update_end(col, row);
            }
        }
    }

    /// Clear the current selection
    pub(crate) fn clear_selection(&mut self) {
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = None;
        }
    }

    /// Select a word at the given position (for double-click)
    pub(crate) fn select_word_at(&mut self, col: usize, row: usize) {
        let screen_buffer = match self.screen_buffer.try_lock() {
            Ok(buf) => buf,
            Err(_) => return,
        };

        // Check if the clicked position is valid
        if row >= screen_buffer.height() {
            return;
        }

        // Helper function to check if a character is part of a word
        let is_word_char = |ch: &str| -> bool { ch.chars().next().map_or(false, |c| c.is_alphanumeric() || c == '_') };

        // Get the character at the clicked position
        let clicked_cell = match screen_buffer.get_cell(col, row) {
            Some(cell) => cell,
            None => return,
        };

        // If clicked on a non-word character, don't select anything
        if !is_word_char(&clicked_cell.ch) || clicked_cell.ch.trim().is_empty() {
            return;
        }

        // Find the start of the word
        let mut start_col = col;
        while start_col > 0 {
            if let Some(cell) = screen_buffer.get_cell(start_col - 1, row) {
                if is_word_char(&cell.ch) && !cell.ch.trim().is_empty() {
                    start_col -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Find the end of the word
        let mut end_col = col;
        let width = screen_buffer.width();
        while end_col < width - 1 {
            if let Some(cell) = screen_buffer.get_cell(end_col + 1, row) {
                if is_word_char(&cell.ch) && !cell.ch.trim().is_empty() {
                    end_col += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Release the screen buffer lock before acquiring selection lock
        drop(screen_buffer);

        // Set the selection
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection {
                start_col,
                start_row: row,
                end_col,
                end_row: row,
            });
        }
    }

    /// Get the selected text as a string
    pub(crate) fn get_selected_text(&self) -> Option<String> {
        let selection = self.selection.try_lock().ok()?;
        if let Some(sel) = *selection {
            let screen_buffer = self.screen_buffer.try_lock().ok()?;
            let (start_col, start_row, end_col, end_row) = sel.normalized();

            let mut text = String::new();

            for row in start_row..=end_row {
                if row >= screen_buffer.height() {
                    break;
                }

                let line_start = if row == start_row { start_col } else { 0 };
                let line_end = if row == end_row {
                    end_col.min(screen_buffer.width() - 1)
                } else {
                    screen_buffer.width() - 1
                };

                // Collect the line content
                let mut line = String::new();
                for col in line_start..=line_end {
                    if let Some(cell) = screen_buffer.get_cell_with_scrollback(col, row) {
                        line.push_str(&cell.ch);
                    }
                }

                // Trim trailing whitespace from the line (terminals pad to full width with spaces)
                let trimmed_line = line.trim_end();
                text.push_str(trimmed_line);

                // Add newline for multi-line selections (except for the last line)
                // Using LF (\n) as the standard Unix line ending
                if row < end_row {
                    text.push('\n');
                }
            }

            Some(text)
        } else {
            None
        }
    }

    /// Get the current working directory of the shell process
    ///
    /// This is implemented differently for each platform:
    /// - **Linux**: Reads `/proc/<pid>/cwd` symlink
    /// - **macOS**: Uses `libproc` to query process information
    /// - **Windows**: Uses `sysinfo` crate to get process current directory
    ///
    /// Returns `None` if the CWD cannot be determined or on unsupported platforms.
    pub(crate) fn get_cwd(&self) -> Option<std::path::PathBuf> {
        #[cfg(target_os = "linux")]
        {
            // On Linux, read /proc/<pid>/cwd symlink
            if let Some(pid) = self.child.process_id() {
                let cwd_path = format!("/proc/{}/cwd", pid);
                if let Ok(path) = std::fs::read_link(&cwd_path) {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, use libproc to get the current working directory
            if let Some(pid) = self.child.process_id() {
                use libproc::libproc::proc_pid::pidcwd;

                if let Ok(path) = pidcwd(pid as i32) {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, use sysinfo to get the current working directory
            if let Some(pid) = self.child.process_id() {
                use sysinfo::{Pid, System};

                let mut system = System::new();
                system.refresh_process(Pid::from_u32(pid));

                if let Some(process) = system.process(Pid::from_u32(pid)) {
                    return process.cwd().map(|p| p.to_path_buf());
                }
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // For other platforms, this is not yet implemented
            // Return None to use default directory
        }

        None
    }

    // Parse escape sequences from application output to detect mode changes
    fn parse_mode_sequences(
        text: &str,
        application_cursor_keys: &Arc<Mutex<bool>>,
        mouse_tracking_mode: &Arc<Mutex<MouseTrackingMode>>,
        mouse_sgr_mode: &Arc<Mutex<bool>>,
        bracketed_paste_mode: &Arc<Mutex<bool>>,
    ) {
        let bytes = text.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            // Look for ESC[?<num>[;<num>...](h|l) (DEC private mode set/reset)
            // This handles both single modes like ESC[?1000h and combined modes like ESC[?1000;1006h
            if i + 4 < bytes.len() && bytes[i] == 27 && bytes[i + 1] == b'[' && bytes[i + 2] == b'?' {
                i += 3;

                // Parse all semicolon-separated mode numbers
                let mut mode_numbers = Vec::new();
                loop {
                    let mut num_str = String::new();
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        num_str.push(bytes[i] as char);
                        i += 1;
                    }

                    if !num_str.is_empty() {
                        mode_numbers.push(num_str);
                    }

                    // Check if we have more modes (semicolon) or the command (h/l)
                    if i < bytes.len() {
                        if bytes[i] == b';' {
                            i += 1; // Skip semicolon and continue parsing
                            continue;
                        } else {
                            // We've reached the command character
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // Now process the command for all parsed mode numbers
                if i < bytes.len() {
                    let command = bytes[i] as char;

                    for num_str in mode_numbers {
                        match num_str.as_str() {
                            "1" => {
                                // DECCKM (cursor key mode)
                                match command {
                                    'h' => {
                                        if let Ok(mut keys) = application_cursor_keys.try_lock() {
                                            *keys = true;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut keys) = application_cursor_keys.try_lock() {
                                            *keys = false;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "9" => {
                                // X10 mouse tracking
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::X10;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::Disabled;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "1000" => {
                                // VT200 normal mouse tracking
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::VT200Normal;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::Disabled;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "1001" => {
                                // VT200 highlight mouse tracking
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::VT200Highlight;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::Disabled;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "1002" => {
                                // Button-event tracking (drag)
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::ButtonEvent;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::Disabled;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "1003" => {
                                // Any-event tracking (motion)
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::AnyEvent;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_tracking_mode.try_lock() {
                                            *mode = MouseTrackingMode::Disabled;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "1006" => {
                                // SGR extended mouse mode
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = mouse_sgr_mode.try_lock() {
                                            *mode = true;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = mouse_sgr_mode.try_lock() {
                                            *mode = false;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "2004" => {
                                // Bracketed paste mode
                                match command {
                                    'h' => {
                                        if let Ok(mut mode) = bracketed_paste_mode.try_lock() {
                                            *mode = true;
                                        }
                                    }
                                    'l' => {
                                        if let Ok(mut mode) = bracketed_paste_mode.try_lock() {
                                            *mode = false;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            i += 1;
        }
    }

    // Parse mode sequences like "?1049h", "?1049l", "?25h", "?25l"
    fn parse_mode_sequences_old(sequence: &str, debug: bool) -> Vec<String> {
        let bytes = sequence.as_bytes();
        let mut mode_numbers = Vec::new();
        let mut i = 0;

        if debug {
            eprintln!("[TERMINAL] Parsing mode sequence: {:?}", sequence);
        }

        while i < bytes.len() {
            // Skip non-digit characters until we find a digit or reach end
            while i < bytes.len() && !bytes[i].is_ascii_digit() && bytes[i] != b'?' {
                i += 1;
            }

            if i >= bytes.len() {
                break;
            }

            // Handle optional '?' prefix
            let mut mode_str = String::new();
            if i < bytes.len() && bytes[i] == b'?' {
                mode_str.push('?');
                i += 1;
            }

            // Collect digits
            loop {
                let mut num_str = String::new();
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    num_str.push(bytes[i] as char);
                    i += 1;
                }

                if !num_str.is_empty() {
                    mode_numbers.push(mode_str.clone() + &num_str);
                }

                // Check for semicolon separator
                if i < bytes.len() && bytes[i] == b';' {
                    i += 1; // Skip semicolon
                    continue;
                } else {
                    break;
                }
            }
        }

        if debug {
            eprintln!("[TERMINAL] Parsed mode numbers: {:?}", mode_numbers);
        }

        mode_numbers
    }

    fn process_output(
        text: &str,
        screen_buffer: &Arc<Mutex<ScreenBuffer>>,
        saved_screen_buffer: &Arc<Mutex<Vec<ScreenBuffer>>>,
        writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    ) -> String {
        let mut incomplete_sequence = String::new();

        let mut sb = screen_buffer.lock().unwrap();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\x1b' => {
                    // Start of escape sequence
                    let mut sequence = String::new();
                    sequence.push(ch);

                    // Look ahead to see what type of sequence this is
                    if let Some(&next_ch) = chars.peek() {
                        match next_ch {
                            '[' => {
                                // CSI sequence
                                sequence.push(chars.next().unwrap()); // consume '['

                                let mut found_end = false;
                                while let Some(&_peek_ch) = chars.peek() {
                                    let ch = chars.next().unwrap();
                                    sequence.push(ch);

                                    // CSI sequences end with a letter in range 0x40-0x7E
                                    if ('@'..='~').contains(&ch) {
                                        found_end = true;
                                        break;
                                    }
                                }

                                if !found_end {
                                    // Incomplete sequence, save it for next iteration
                                    incomplete_sequence = sequence;
                                    break;
                                }

                                // Process complete CSI sequence
                                Self::process_csi_sequence(&sequence, &mut sb, saved_screen_buffer, writer);
                            }
                            ']' => {
                                // OSC (Operating System Command) sequence
                                // Format: ESC ] <number> ; <text> BEL (or ESC \)
                                // Example: ESC ] 0 ; title BEL (set window title)
                                sequence.push(chars.next().unwrap()); // consume ']'

                                let mut found_end = false;
                                while let Some(&_peek_ch) = chars.peek() {
                                    let ch = chars.next().unwrap();
                                    sequence.push(ch);

                                    // OSC sequences end with BEL (0x07) or ST (ESC \)
                                    if ch == '\x07' {
                                        found_end = true;
                                        break;
                                    }
                                    // Check for ST (ESC \) - need to check if previous char was ESC
                                    if ch == '\\' && sequence.len() >= 2 && sequence.chars().nth(sequence.len() - 2) == Some('\x1b') {
                                        found_end = true;
                                        break;
                                    }
                                }

                                if !found_end {
                                    // Incomplete sequence, save it for next iteration
                                    incomplete_sequence = sequence;
                                    break;
                                }

                                // OSC sequences are for terminal control (titles, etc.), not for display
                                // We ignore them for now - they should not be rendered
                            }
                            '(' | ')' | '*' | '+' => {
                                // Character set designation sequences
                                sequence.push(chars.next().unwrap()); // consume the designation char
                                if let Some(charset_ch) = chars.next() {
                                    sequence.push(charset_ch);
                                    // We can ignore these for now
                                } else {
                                    incomplete_sequence = sequence;
                                    break;
                                }
                            }
                            'c' => {
                                // RIS (Reset to Initial State)
                                chars.next(); // consume 'c'
                                sb.clear_screen();
                            }
                            '7' => {
                                // Save cursor position (DECSC)
                                chars.next(); // consume '7'
                                sb.save_cursor();
                            }
                            '8' => {
                                // Restore cursor position (DECRC)
                                chars.next(); // consume '8'
                                sb.restore_cursor();
                            }
                            'D' => {
                                // IND (Index) - move cursor down one line
                                // If at bottom of scroll region, scroll up instead
                                chars.next(); // consume 'D'

                                let scroll_bottom = if let Some((_, bottom)) = sb.get_scroll_region() {
                                    bottom
                                } else {
                                    sb.height() - 1
                                };

                                if sb.cursor_y == scroll_bottom {
                                    // At bottom of scroll region, scroll up
                                    sb.scroll_up(1);
                                } else {
                                    sb.move_cursor_down(1);
                                }
                            }
                            'M' => {
                                // RI (Reverse Index) - move cursor up one line
                                // If at top of scroll region, scroll down instead
                                chars.next(); // consume 'M'

                                let scroll_top = if let Some((top, _)) = sb.get_scroll_region() { top } else { 0 };

                                if sb.cursor_y == scroll_top {
                                    // At top of scroll region, scroll down
                                    sb.scroll_down(1);
                                } else {
                                    sb.move_cursor_up(1);
                                }
                            }
                            'E' => {
                                // NEL (Next Line)
                                chars.next(); // consume 'E'
                                sb.cursor_x = 0;
                                sb.move_cursor_down(1);
                            }
                            'H' => {
                                // Set tab stop at current column
                                chars.next(); // consume 'H'
                                              // We can ignore this for now
                            }
                            '=' => {
                                // DECKPAM (Keypad Application Mode)
                                chars.next(); // consume '='
                                              // We can ignore this for now
                            }
                            '>' => {
                                // DECKPNM (Keypad Numeric Mode)
                                chars.next(); // consume '>'
                                              // We can ignore this for now
                            }
                            _ => {
                                // Unknown escape sequence, just consume the next character
                                chars.next();
                            }
                        }
                    } else {
                        // Incomplete escape sequence
                        incomplete_sequence = sequence;
                        break;
                    }
                }
                '\r' => {
                    // Carriage return
                    sb.pending_wrap = false;
                    sb.cursor_x = 0;
                }
                '\n' => {
                    // Line feed
                    sb.newline();
                }
                '\t' => {
                    // Tab
                    sb.tab();
                }
                '\x08' => {
                    // Backspace
                    sb.move_cursor_left(1);
                }
                '\x0c' => {
                    // Form feed - clear screen
                    sb.clear_screen();
                }
                '\x07' => {
                    // Bell - we can ignore this or implement a visual bell
                }
                ch if ch.is_control() => {
                    // Other control characters - ignore for now
                }
                _ => {
                    // Regular character - collect into a buffer to handle grapheme clusters
                    // We need to collect all text until the next control character
                    let mut text_buffer = String::new();
                    text_buffer.push(ch);

                    // Peek ahead and collect more characters until we hit a control char
                    while let Some(&peek_ch) = chars.peek() {
                        if peek_ch.is_control() || peek_ch == '\x1b' {
                            break;
                        }
                        text_buffer.push(chars.next().unwrap());
                    }

                    // Now process the buffer as grapheme clusters
                    for grapheme in text_buffer.graphemes(true) {
                        sb.put_grapheme(grapheme);
                    }
                }
            }
        }

        incomplete_sequence
    }

    fn process_csi_sequence(
        sequence: &str,
        sb: &mut ScreenBuffer,
        saved_screen_buffer: &Arc<Mutex<Vec<ScreenBuffer>>>,
        writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    ) {
        use crate::ansi;

        let debug = false; // Set to true for debugging

        if debug {
            eprintln!("[TERMINAL] Processing CSI: {:?}", sequence);
        }

        // Extract the final character and arguments
        let chars: Vec<char> = sequence.chars().collect();
        if chars.len() < 3 {
            return;
        }

        let final_char = chars[chars.len() - 1];
        let args_str = sequence.trim_start_matches("\x1b[").trim_end_matches(final_char);
        let args: Vec<&str> = if args_str.is_empty() { vec![] } else { args_str.split(';').collect() };

        match final_char {
            'A' => {
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.move_cursor_up(n);
            }
            'B' => {
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.move_cursor_down(n);
            }
            'C' => {
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.move_cursor_right(n);
            }
            'D' => {
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.move_cursor_left(n);
            }
            'G' => {
                // CHA (Cursor Horizontal Absolute)
                let col = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.pending_wrap = false;
                sb.cursor_x = col.saturating_sub(1).min(sb.width() - 1);
            }
            'H' | 'f' => {
                // CUP (Cursor Position) and HVP (Horizontal and Vertical Position)
                // Both work identically: CSI row ; col H/f
                if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                    sb.move_cursor_to(0, 0);
                } else {
                    let [row, col] = ansi::parse_capital_h(sequence);
                    let target_row = (row as usize).saturating_sub(1);
                    let target_col = (col as usize).saturating_sub(1);
                    sb.move_cursor_to(target_col, target_row);
                }
            }
            'J' => {
                // ED (Erase in Display)
                let arg = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<i32>().unwrap_or(0)
                };
                match arg {
                    0 => sb.clear_from_cursor_to_end(),
                    1 => sb.clear_from_start_to_cursor(),
                    2 | 3 => sb.clear_screen(),
                    _ => {}
                }
            }
            'K' => {
                // EL (Erase in Line)
                let arg = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<i32>().unwrap_or(0)
                };
                match arg {
                    0 => sb.clear_line_from_cursor(),
                    1 => sb.clear_line_to_cursor(),
                    2 => sb.clear_line(),
                    _ => {}
                }
            }
            'r' => {
                // DECSTBM - Set scrolling region (top;bottom)
                // CSI Ps ; Ps r - Sets the top and bottom margins for scrolling
                // This is used by applications like vim, less, mc, etc.
                // Per VT100 spec, this command also moves cursor to home position
                if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                    // No arguments means reset to full screen
                    sb.reset_scroll_region();
                    sb.move_cursor_to(0, 0);
                } else {
                    let [top, bottom] = ansi::parse_scroll_region(sequence);
                    if debug {
                        eprintln!("[TERMINAL] Setting scroll region: top={}, bottom={}", top, bottom);
                    }
                    // Convert from 1-based ANSI coordinates to 0-based indices
                    let top_idx = (top - 1).max(0) as usize;
                    let bottom_idx = if bottom == -1 {
                        sb.height().saturating_sub(1)
                    } else {
                        (bottom - 1).max(0) as usize
                    };
                    sb.set_scroll_region(top_idx, bottom_idx);
                    sb.move_cursor_to(0, 0);
                }
            }
            'h' | 'l' => {
                // Set Mode (h) or Reset Mode (l)
                let mode_numbers = Self::parse_mode_sequences_old(&sequence[3..sequence.len() - 1], debug);

                for mode_str in mode_numbers {
                    if debug {
                        eprintln!("[TERMINAL] Processing mode: {} ({})", mode_str, if final_char == 'h' { "set" } else { "reset" });
                    }

                    match mode_str.as_str() {
                        "1049" => {
                            // Alternate screen buffer (supports stacking for nested alternate screens)
                            if final_char == 'h' {
                                // Save current screen to stack and switch to alternate
                                let mut saved_stack = saved_screen_buffer.lock().unwrap();
                                // Save the current (main) buffer
                                saved_stack.push(sb.clone());

                                // Create a BRAND NEW empty buffer for alternate screen
                                // This prevents any content from the main screen bleeding through
                                let scrollback_limit = sb.scrollback_limit();
                                *sb = ScreenBuffer::new_with_scrollback(sb.width(), sb.height(), scrollback_limit);
                            } else {
                                // Restore screen from stack
                                let mut saved_stack = saved_screen_buffer.lock().unwrap();
                                if let Some(mut saved_sb) = saved_stack.pop() {
                                    // Check if dimensions match, resize saved buffer if needed
                                    if saved_sb.width() != sb.width() || saved_sb.height() != sb.height() {
                                        saved_sb.resize(sb.width(), sb.height());
                                    }
                                    *sb = saved_sb;
                                }
                            }
                        }
                        "25" => {
                            // Cursor visibility
                            // Cursor visibility - we don't implement this yet
                        }
                        "1" => {
                            // Application cursor keys mode
                            // We can ignore this for now
                        }
                        "6" => {
                            // DECOM - Origin mode
                            // When enabled, cursor positioning is relative to scroll region
                            if final_char == 'h' {
                                sb.set_origin_mode(true);
                                sb.move_cursor_to(0, 0); // Move to home position (top-left of scroll region)
                            } else {
                                sb.set_origin_mode(false);
                                sb.move_cursor_to(0, 0); // Move to home position (top-left of screen)
                            }
                        }
                        "?7" => {
                            // Auto-wrap mode
                            // Auto-wrap mode - we don't implement this yet
                        }

                        "?1000" | "?1002" | "?1003" => {
                            // Mouse reporting modes - we can ignore for now
                        }
                        "?1006" => {
                            // SGR mouse mode - we can ignore for now
                        }
                        "?2004" => {
                            // Bracketed paste mode - we can ignore for now
                        }
                        _ => {
                            if debug {
                                eprintln!("[TERMINAL] Ignoring unknown mode: {}", mode_str);
                            }
                        }
                    }
                }
            }
            'L' => {
                // Insert Line(s) - CSI Ps L
                // Insert Ps blank lines at cursor position
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.insert_lines(n);
            }
            'M' => {
                // Delete Line(s) - CSI Ps M
                // Delete Ps lines starting at cursor position
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.delete_lines(n);
            }
            'S' => {
                // Scroll Up - CSI Ps S
                // Scroll up Ps lines (default = 1)
                // This scrolls the content within the scrolling region
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.scroll_up(n);
            }
            'T' => {
                // Scroll Down - CSI Ps T
                // Scroll down Ps lines (default = 1)
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.scroll_down(n);
            }
            '@' => {
                // Insert Characters - CSI Ps @
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.insert_chars(n);
            }
            'P' => {
                // Delete Characters - CSI Ps P
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.delete_chars(n);
            }
            'X' => {
                // Erase Characters - CSI Ps X
                let n = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.erase_chars(n);
            }
            'd' => {
                // VPA (Vertical Position Absolute) - CSI Ps d
                let row = if args.is_empty() || args[0].is_empty() {
                    1
                } else {
                    args[0].parse::<usize>().unwrap_or(1)
                };
                sb.cursor_y = row.saturating_sub(1).min(sb.height() - 1);
            }
            'm' => {
                // SGR (Select Graphic Rendition) - colors and text attributes
                let [fg, bg] = ansi::parse_m(sequence);
                if let Some(color) = fg {
                    sb.fg_color = color;
                }
                if let Some(color) = bg {
                    sb.bg_color = color;
                }
            }
            'n' => {
                // Device Status Report (DSR)
                // CSI 6 n - Request cursor position
                // Response: CSI row ; col R (1-based)
                let param = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<u32>().unwrap_or(0)
                };

                eprintln!("[DSR] Received DSR query with param={}", param);

                if param == 6 {
                    // Cursor Position Report (CPR)
                    let row = sb.cursor_y + 1; // 1-based
                    let col = sb.cursor_x + 1; // 1-based
                    let response = format!("\x1b[{};{}R", row, col);

                    // Send response back through PTY to the application
                    eprintln!("[DSR] Sending cursor position report: row={}, col={} (response: {:?})", row, col, response);
                    if let Ok(mut w) = writer.lock() {
                        if let Err(e) = w.write_all(response.as_bytes()) {
                            eprintln!("[DSR] Failed to send cursor position report: {}", e);
                        } else if let Err(e) = w.flush() {
                            eprintln!("[DSR] Failed to flush cursor position report: {}", e);
                        } else {
                            eprintln!("[DSR] Successfully sent cursor position report");
                        }
                    } else {
                        eprintln!("[DSR] Failed to acquire writer lock");
                    }
                } else {
                    eprintln!("[DSR] Ignoring non-CPR DSR query (param={})", param);
                }
            }
            'c' => {
                // Device Attributes (DA)
                // We ignore this for now as it requires sending responses back
            }
            's' => {
                // Save cursor position (ANSI.SYS style)
                sb.save_cursor();
            }
            'u' => {
                // Restore cursor position (ANSI.SYS style)
                sb.restore_cursor();
            }
            _ => {
                if debug {
                    eprintln!("[TERMINAL] Ignoring unknown CSI sequence: {:?}", sequence);
                }
            }
        }
    }

    /// Add a command to the history (keeps last MAX_COMMAND_HISTORY commands)
    pub(crate) fn add_command_to_history(&self, command: String) {
        if let Ok(mut history) = self.command_history.lock() {
            // Don't add empty commands or duplicates of the last command
            if !command.trim().is_empty() && (history.is_empty() || history.last() != Some(&command)) {
                history.push(command);
                // Keep only last MAX_COMMAND_HISTORY commands
                if history.len() > MAX_COMMAND_HISTORY {
                    history.remove(0);
                }
            }
        }
    }

    /// Capture current output lines (keeps last MAX_OUTPUT_HISTORY lines)
    pub(crate) fn capture_output_history(&self) {
        if let Ok(sb) = self.screen_buffer.lock() {
            let mut lines = Vec::new();

            // Get lines from scrollback buffer
            let scrollback = sb.get_scrollback_buffer();
            for row in scrollback.iter() {
                let line: String = row.iter().map(|cell| cell.ch.as_str()).collect();
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }

            // Get lines from current screen
            for y in 0..sb.height() {
                let mut line = String::new();
                for x in 0..sb.width() {
                    if let Some(cell) = sb.get_cell(x, y) {
                        line.push_str(&cell.ch);
                    }
                }
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }

            // Keep only last MAX_OUTPUT_HISTORY lines
            let start = if lines.len() > MAX_OUTPUT_HISTORY {
                lines.len() - MAX_OUTPUT_HISTORY
            } else {
                0
            };
            let last_lines = lines[start..].to_vec();

            if let Ok(mut output_history) = self.output_history.lock() {
                *output_history = last_lines;
            }
        }
    }

    /// Get command history
    pub(crate) fn get_command_history(&self) -> Vec<String> {
        self.command_history.lock().ok().map(|h| h.clone()).unwrap_or_default()
    }

    /// Set command history (for loading from state)
    pub(crate) fn set_command_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.command_history.lock() {
            *h = history;
        }
    }

    /// Get output history
    pub(crate) fn get_output_history(&self) -> Vec<String> {
        self.output_history.lock().ok().map(|h| h.clone()).unwrap_or_default()
    }

    /// Set output history and restore to scrollback (for loading from state)
    pub(crate) fn set_output_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.output_history.lock() {
            *h = history.clone();
        }

        // Restore output to scrollback buffer
        self.restore_output_to_scrollback(history);
    }

    /// Restore saved output lines to scrollback buffer
    fn restore_output_to_scrollback(&self, lines: Vec<String>) {
        if let Ok(mut sb) = self.screen_buffer.lock() {
            // Remove the last line to avoid duplicate prompts after restoration.
            // When output history is captured, it includes the current screen state,
            // which typically ends with the active prompt line. When we restore this
            // history, the terminal will generate a new prompt, resulting in two
            // prompts if we don't strip the last captured line. By removing it here,
            // we ensure the restored scrollback contains only completed output, and
            // the new terminal session starts with exactly one fresh prompt.
            let mut lines_to_restore = lines;
            if !lines_to_restore.is_empty() {
                lines_to_restore.pop();
            }
            sb.restore_to_scrollback(lines_to_restore);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Kill the child process when the terminal is dropped
        let _ = self.kill();
    }
}
