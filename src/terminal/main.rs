use crate::history;
use crate::screen_buffer::ScreenBuffer;
use crate::terminal::config::ShellConfig;
use crate::terminal::sequences::process_output;
use crate::terminal::utils::{create_shell_init_file, MAX_COMMAND_HISTORY, MAX_OUTPUT_HISTORY};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
    pub(crate) cursor_visible: Arc<Mutex<bool>>,
    pub(crate) default_cursor_style: Arc<Mutex<crate::screen_buffer::CursorStyle>>,
    pub(crate) command_history: Arc<Mutex<Vec<String>>>,
    pub(crate) output_history: Arc<Mutex<Vec<String>>>,
    pub(crate) current_command: Arc<Mutex<String>>,
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

    pub fn normalized(&self) -> (usize, usize, usize, usize) {
        if self.start_row < self.end_row || (self.start_row == self.end_row && self.start_col <= self.end_col) {
            (self.start_col, self.start_row, self.end_col, self.end_row)
        } else {
            (self.end_col, self.end_row, self.start_col, self.start_row)
        }
    }

    pub fn contains(&self, col: usize, row: usize) -> bool {
        let (start_col, start_row, end_col, end_row) = self.normalized();

        if row < start_row || row > end_row {
            return false;
        }

        if row == start_row && row == end_row {
            col >= start_col && col <= end_col
        } else if row == start_row {
            col >= start_col
        } else if row == end_row {
            col <= end_col
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MouseTrackingMode {
    Disabled,
    X10,
    VT200Normal,
    VT200Highlight,
    ButtonEvent,
    AnyEvent,
}

impl Terminal {
    pub(crate) fn new_with_scrollback(
        initial_width: u32,
        initial_height: u32,
        shell_config: ShellConfig,
        scrollback_limit: usize,
        start_directory: Option<std::path::PathBuf>,
        cursor_style: crate::screen_buffer::CursorStyle,
    ) -> Self {
        let pty_system = native_pty_system();

        let pty_size = PtySize {
            rows: initial_height as u16,
            cols: initial_width as u16,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_pair = pty_system.openpty(pty_size).expect("Failed to create PTY pair");

        eprintln!("[TERMINAL] PTY created with initial size: {}x{}", initial_width, initial_height);

        let mut cmd = CommandBuilder::new(&shell_config.command);

        let temp_init_file = create_shell_init_file(&shell_config.command);

        match shell_config.command.as_str() {
            "bash" => {
                if let Some(ref init_file) = temp_init_file {
                    cmd.arg("--rcfile");
                    cmd.arg(init_file);
                } else {
                    for arg in &shell_config.args {
                        cmd.arg(arg);
                    }
                }
            }
            "zsh" => {
                if let Some(ref init_file) = temp_init_file {
                    let parent_dir = init_file.parent().unwrap();
                    cmd.env("ZDOTDIR", parent_dir.to_str().unwrap());
                }
                for arg in &shell_config.args {
                    cmd.arg(arg);
                }
            }
            _ => {
                for arg in &shell_config.args {
                    cmd.arg(arg);
                }
            }
        }

        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", initial_width.to_string());
        cmd.env("LINES", initial_height.to_string());

        if let Some(dir) = start_directory {
            cmd.cwd(dir);
        }

        let child = pty_pair.slave.spawn_command(cmd).expect("Failed to spawn shell process");

        eprintln!("[TERMINAL] Shell process spawned: {}", shell_config.command);

        let screen_buffer = Arc::new(Mutex::new(ScreenBuffer::new_with_scrollback(
            initial_width as usize,
            initial_height as usize,
            scrollback_limit,
            cursor_style,
        )));

        let screen_buffer_clone = Arc::clone(&screen_buffer);
        let saved_screen_buffer = Arc::new(Mutex::new(Vec::new()));
        let saved_screen_buffer_clone = Arc::clone(&saved_screen_buffer);

        let application_cursor_keys = Arc::new(Mutex::new(false));
        let mouse_tracking_mode = Arc::new(Mutex::new(MouseTrackingMode::Disabled));
        let mouse_sgr_mode = Arc::new(Mutex::new(false));
        let bracketed_paste_mode = Arc::new(Mutex::new(false));
        let cursor_visible = Arc::new(Mutex::new(true));

        let application_cursor_keys_clone = Arc::clone(&application_cursor_keys);
        let mouse_tracking_mode_clone = Arc::clone(&mouse_tracking_mode);
        let mouse_sgr_mode_clone = Arc::clone(&mouse_sgr_mode);
        let bracketed_paste_mode_clone = Arc::clone(&bracketed_paste_mode);
        let cursor_visible_clone = Arc::clone(&cursor_visible);

        let last_command_exit_code = Arc::new(Mutex::new(None));
        let last_command_exit_code_clone = Arc::clone(&last_command_exit_code);

        let mut reader = pty_pair.master.try_clone_reader().expect("Failed to clone PTY reader");

        let writer = pty_pair.master.take_writer().expect("Failed to get PTY writer");
        let writer = Arc::new(Mutex::new(writer));
        let thread_writer = Arc::clone(&writer);

        let default_cursor_style = Arc::new(Mutex::new(cursor_style));
        let default_cursor_style_clone = Arc::clone(&default_cursor_style);

        let master = pty_pair.master;

        thread::spawn(move || {
            let mut buffer = [0; 20000];
            let mut incomplete_sequence = String::new();

            loop {
                match reader.read(&mut buffer) {
                    Ok(bytes_read) if bytes_read > 0 => {
                        let mut text = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

                        if !incomplete_sequence.is_empty() {
                            text = incomplete_sequence.clone() + &text;
                            incomplete_sequence.clear();
                        }

                        Self::parse_mode_sequences(
                            &text,
                            &application_cursor_keys_clone,
                            &mouse_tracking_mode_clone,
                            &mouse_sgr_mode_clone,
                            &bracketed_paste_mode_clone,
                            &cursor_visible_clone,
                        );

                        incomplete_sequence = process_output(
                            &text,
                            &screen_buffer_clone,
                            &saved_screen_buffer_clone,
                            &thread_writer,
                            &last_command_exit_code_clone,
                            &default_cursor_style_clone,
                        );

                        if !incomplete_sequence.is_empty() {
                            eprintln!(
                                "[TERMINAL] Saved incomplete sequence: {:?} (len={})",
                                incomplete_sequence.chars().take(20).collect::<String>(),
                                incomplete_sequence.len()
                            );
                        }
                    }
                    Ok(_) => {
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
            cursor_visible,
            default_cursor_style,
            command_history: Arc::new(Mutex::new(Vec::new())),
            output_history: Arc::new(Mutex::new(Vec::new())),
            current_command: Arc::new(Mutex::new(String::new())),
        }
    }

    pub(crate) fn set_size(&mut self, new_width: u32, new_height: u32, clear_screen: bool) {
        self.width = new_width;
        self.height = new_height;

        if let Ok(mut sb) = self.screen_buffer.lock() {
            sb.resize(new_width as usize, new_height as usize);

            if clear_screen {
                sb.clear_screen();
                eprintln!("[TERMINAL] Cleared screen buffer after resize");
            }
        }

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
    }

    pub(crate) fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub(crate) fn kill(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.child.kill()?;
        Ok(())
    }

    pub(crate) fn send_key(&mut self, keys: &[u8]) {
        let is_enter = keys.len() == 1 && keys[0] == b'\r';

        if is_enter {
            if let Ok(mut current_cmd) = self.current_command.lock() {
                current_cmd.clear();
            }
        }

        let app_cursor_mode = *self.application_cursor_keys.lock().unwrap();

        let is_arrow_key = keys.len() == 3 && keys[0] == 27 && keys[1] == b'[' && (keys[2] == b'A' || keys[2] == b'B' || keys[2] == b'C' || keys[2] == b'D');

        if let Ok(mut writer) = self.writer.lock() {
            if app_cursor_mode && is_arrow_key {
                let translated = [27, b'O', keys[2]];
                if let Err(err) = writer.write_all(&translated) {
                    eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
                }
            } else {
                if let Err(err) = writer.write_all(keys) {
                    eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
                }
            }
            if let Err(err) = writer.flush() {
                eprintln!("[TERMINAL] Failed to flush PTY writer: {}", err);
            }
        }
    }

    pub(crate) fn send_text(&mut self, text: &str) {
        if text.contains('\n') || text.contains('\r') {
            if let Ok(mut current_cmd) = self.current_command.lock() {
                current_cmd.clear();
            }
        }

        if let Ok(mut writer) = self.writer.lock() {
            let converted = text.replace('\n', "\r");
            if let Err(err) = writer.write_all(converted.as_bytes()) {
                eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
            }
            if let Err(err) = writer.flush() {
                eprintln!("[TERMINAL] Failed to flush PTY writer: {}", err);
            }
        }
    }

    pub(crate) fn send_paste(&mut self, text: &str) {
        if let Ok(mut writer) = self.writer.lock() {
            let bracketed_paste = self.bracketed_paste_mode.lock().map(|mode| *mode).unwrap_or(false);

            if bracketed_paste {
                if let Err(err) = writer.write_all(b"\x1b[200~") {
                    eprintln!("[TERMINAL] Failed to write bracketed paste start: {}", err);
                    return;
                }

                let converted = text.replace('\n', "\r");
                if let Err(err) = writer.write_all(converted.as_bytes()) {
                    eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
                    return;
                }

                if let Err(err) = writer.write_all(b"\x1b[201~") {
                    eprintln!("[TERMINAL] Failed to write bracketed paste end: {}", err);
                }
            } else {
                let converted = text.replace('\n', "\r");
                if let Err(err) = writer.write_all(converted.as_bytes()) {
                    eprintln!("[TERMINAL] Failed to write text to PTY: {}", err);
                }
            }
            if let Err(err) = writer.flush() {
                eprintln!("[TERMINAL] Failed to flush PTY writer: {}", err);
            }
        }
    }

    pub(crate) fn has_process_exited(&mut self) -> bool {
        !self.is_alive()
    }

    pub(crate) fn send_mouse_event(&mut self, button: u8, col: u32, row: u32, pressed: bool) {
        let Ok(tracking_mode_guard) = self.mouse_tracking_mode.try_lock() else {
            return;
        };
        let tracking_mode = *tracking_mode_guard;
        drop(tracking_mode_guard);

        let Ok(sgr_mode_guard) = self.mouse_sgr_mode.try_lock() else {
            return;
        };
        let sgr_mode = *sgr_mode_guard;
        drop(sgr_mode_guard);

        if tracking_mode == MouseTrackingMode::Disabled {
            return;
        }

        let col = col.max(1).min(if sgr_mode { 9999 } else { 223 });
        let row = row.max(1).min(if sgr_mode { 9999 } else { 223 });

        let sequence = if sgr_mode {
            let terminator = if pressed { 'M' } else { 'm' };
            format!("\x1b[<{};{};{}{}", button, col, row, terminator)
        } else {
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
            if let Err(err) = writer.flush() {
                eprintln!("[TERMINAL] Failed to flush PTY writer: {}", err);
            }
        }
    }

    pub(crate) fn start_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection::new(col, row));
        }
    }

    pub(crate) fn update_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut selection) = self.selection.try_lock() {
            if let Some(ref mut sel) = *selection {
                sel.update_end(col, row);
            }
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = None;
        }
    }

    pub(crate) fn select_word_at(&mut self, col: usize, row: usize) {
        let screen_buffer = match self.screen_buffer.try_lock() {
            Ok(buf) => buf,
            Err(_) => return,
        };

        if row >= screen_buffer.height() {
            return;
        }

        let is_word_char = |ch: char| -> bool { ch.is_alphanumeric() || ch == '_' };

        let clicked_cell = match screen_buffer.get_cell(col, row) {
            Some(cell) => cell,
            None => return,
        };

        if !is_word_char(clicked_cell.ch) || clicked_cell.ch == ' ' || clicked_cell.ch == '\0' {
            return;
        }

        let mut start_col = col;
        while start_col > 0 {
            if let Some(cell) = screen_buffer.get_cell(start_col - 1, row) {
                if is_word_char(cell.ch) && cell.ch != ' ' && cell.ch != '\0' {
                    start_col -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let mut end_col = col;
        let width = screen_buffer.width();
        while end_col < width - 1 {
            if let Some(cell) = screen_buffer.get_cell(end_col + 1, row) {
                if is_word_char(cell.ch) && cell.ch != ' ' && cell.ch != '\0' {
                    end_col += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        drop(screen_buffer);

        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection {
                start_col,
                start_row: row,
                end_col,
                end_row: row,
            });
        }
    }

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

                let mut line = String::new();
                for col in line_start..=line_end {
                    if let Some(cell) = screen_buffer.get_cell_with_scrollback(col, row) {
                        if cell.width == 0 || cell.ch == '\0' {
                            continue;
                        }

                        if let Some(ref extended) = cell.extended {
                            line.push_str(extended);
                        } else {
                            line.push(cell.ch);
                        }
                    }
                }

                let trimmed_line = line.trim_end();
                text.push_str(trimmed_line);

                if row < end_row {
                    text.push('\n');
                }
            }

            Some(text)
        } else {
            None
        }
    }

    pub(crate) fn get_cwd(&self) -> Option<std::path::PathBuf> {
        #[cfg(target_os = "linux")]
        {
            if let Some(pid) = self.child.process_id() {
                let cwd_path = format!("/proc/{}/cwd", pid);
                if let Ok(path) = std::fs::read_link(&cwd_path) {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(pid) = self.child.process_id() {
                use libproc::libproc::proc_pid::pidcwd;

                if let Ok(path) = pidcwd(pid as i32) {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
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

    fn parse_mode_sequences(
        text: &str,
        application_cursor_keys: &Arc<Mutex<bool>>,
        mouse_tracking_mode: &Arc<Mutex<MouseTrackingMode>>,
        mouse_sgr_mode: &Arc<Mutex<bool>>,
        bracketed_paste_mode: &Arc<Mutex<bool>>,
        cursor_visible: &Arc<Mutex<bool>>,
    ) {
        let bytes = text.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if i + 4 < bytes.len() && bytes[i] == 27 && bytes[i + 1] == b'[' && bytes[i + 2] == b'?' {
                i += 3;

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

                    if i < bytes.len() {
                        if bytes[i] == b';' {
                            i += 1;
                            continue;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                if i < bytes.len() {
                    let command = bytes[i] as char;

                    for num_str in mode_numbers {
                        match num_str.as_str() {
                            "1" => match command {
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
                            },
                            "9" => match command {
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
                            },
                            "1000" => match command {
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
                            },
                            "1001" => match command {
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
                            },
                            "1002" => match command {
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
                            },
                            "1003" => match command {
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
                            },
                            "1006" => match command {
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
                            },
                            "25" => match command {
                                'h' => {
                                    if let Ok(mut visible) = cursor_visible.try_lock() {
                                        *visible = true;
                                    }
                                }
                                'l' => {
                                    if let Ok(mut visible) = cursor_visible.try_lock() {
                                        *visible = false;
                                    }
                                }
                                _ => {}
                            },
                            "2004" => match command {
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
                            },
                            _ => {}
                        }
                    }
                }
            }
            i += 1;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn add_command_to_history(&self, command: String) {
        if let Ok(mut history) = self.command_history.lock() {
            if !command.trim().is_empty() && (history.is_empty() || history.last() != Some(&command)) {
                history.push(command);
                if history.len() > MAX_COMMAND_HISTORY {
                    history.remove(0);
                }
            }
        }
    }

    pub(crate) fn capture_output_history(&self) {
        if let Ok(sb) = self.screen_buffer.lock() {
            let mut lines = Vec::new();

            let scrollback = sb.get_scrollback_buffer();
            for row in scrollback.iter() {
                let line: String = row
                    .iter()
                    .map(|cell| {
                        if let Some(ref extended) = cell.extended {
                            extended.to_string()
                        } else {
                            cell.ch.to_string()
                        }
                    })
                    .collect();
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }

            for y in 0..sb.height() {
                let mut line = String::new();
                for x in 0..sb.width() {
                    if let Some(cell) = sb.get_cell(x, y) {
                        if let Some(ref extended) = cell.extended {
                            line.push_str(extended);
                        } else {
                            line.push(cell.ch);
                        }
                    }
                }
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }

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

    pub(crate) fn get_command_history(&self) -> Vec<String> {
        history::read_shell_history(MAX_COMMAND_HISTORY)
    }

    pub(crate) fn set_command_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.command_history.lock() {
            *h = history;
        }
    }

    pub(crate) fn get_output_history(&self) -> Vec<String> {
        self.output_history.lock().ok().map(|h| h.clone()).unwrap_or_default()
    }

    pub(crate) fn set_output_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.output_history.lock() {
            *h = history.clone();
        }

        self.restore_output_to_scrollback(history);
    }

    fn restore_output_to_scrollback(&self, lines: Vec<String>) {
        if let Ok(mut sb) = self.screen_buffer.lock() {
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
        let _ = self.kill();
    }
}
