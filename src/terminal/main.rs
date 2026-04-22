use crate::ghostty_buffer::{CursorStyle, GhosttyBuffer, MouseTrackingMode};
use crate::terminal::config::ShellConfig;
use crate::terminal::utils::{create_shell_init_file, MAX_COMMAND_HISTORY, MAX_OUTPUT_HISTORY};
use crate::history;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub(crate) struct Terminal {
    master: Box<dyn portable_pty::MasterPty>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    child: Box<dyn portable_pty::Child>,
    pub(crate) ghostty_buffer: Arc<Mutex<GhosttyBuffer>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) shell_config: ShellConfig,
    pub(crate) selection: Arc<Mutex<Option<Selection>>>,
    pub(crate) command_history: Arc<Mutex<Vec<String>>>,
    pub(crate) output_history: Arc<Mutex<Vec<String>>>,
    pub(crate) current_command: Arc<Mutex<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Selection {
    pub start_col: usize,
    pub start_row: usize, // Absolute row position (scrollback_len + screen_row)
    pub end_col: usize,
    pub end_row: usize,
}

impl Selection {
    pub fn new(col: usize, screen_row: usize, scroll_offset: usize, scrollback_len: usize) -> Self {
        let absolute_row = scrollback_len.saturating_sub(scroll_offset) + screen_row;
        Selection {
            start_col: col,
            start_row: absolute_row,
            end_col: col,
            end_row: absolute_row,
        }
    }

    pub fn update_end(
        &mut self,
        col: usize,
        screen_row: usize,
        scroll_offset: usize,
        scrollback_len: usize,
    ) {
        let absolute_row = scrollback_len.saturating_sub(scroll_offset) + screen_row;
        self.end_col = col;
        self.end_row = absolute_row;
    }

    pub fn normalized(&self) -> (usize, usize, usize, usize) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_col, self.start_row, self.end_col, self.end_row)
        } else {
            (self.end_col, self.end_row, self.start_col, self.start_row)
        }
    }

    pub fn contains(
        &self,
        col: usize,
        screen_row: usize,
        current_scroll_offset: usize,
        scrollback_len: usize,
    ) -> bool {
        let absolute_row =
            scrollback_len.saturating_sub(current_scroll_offset) + screen_row;
        let (start_col, start_row, end_col, end_row) = self.normalized();

        if absolute_row < start_row || absolute_row > end_row {
            return false;
        }

        if absolute_row == start_row && absolute_row == end_row {
            col >= start_col && col <= end_col
        } else if absolute_row == start_row {
            col >= start_col
        } else if absolute_row == end_row {
            col <= end_col
        } else {
            true
        }
    }
}


impl Terminal {
    pub(crate) fn new_with_scrollback(
        initial_width: u32,
        initial_height: u32,
        shell_config: ShellConfig,
        scrollback_limit: usize,
        start_directory: Option<std::path::PathBuf>,
        default_cursor: CursorStyle,
    ) -> Self {
        let pty_system = native_pty_system();

        let pty_size = PtySize {
            rows: initial_height as u16,
            cols: initial_width as u16,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_pair = pty_system
            .openpty(pty_size)
            .expect("Failed to create PTY pair");

        eprintln!(
            "[TERMINAL] PTY created with initial size: {}x{}",
            initial_width, initial_height
        );

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

        if let Some(ref dir) = start_directory {
            cmd.cwd(dir);
        }

        let child = pty_pair
            .slave
            .spawn_command(cmd)
            .expect("Failed to spawn shell process");

        eprintln!(
            "[TERMINAL] Shell process spawned: {}",
            shell_config.command
        );

        let writer = pty_pair
            .master
            .take_writer()
            .expect("Failed to get PTY writer");
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(writer));

        // Bounded channel for PTY response writes (terminal query replies, kitty acks, etc.).
        // The on_pty_write callback uses try_send (non-blocking) so vt_write() can never
        // stall the main thread.  This dedicated thread does the actual write_all/flush.
        let (pty_write_tx, pty_write_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64);
        let writer_for_pty_thread = Arc::clone(&writer);
        thread::spawn(move || {
            while let Ok(data) = pty_write_rx.recv() {
                if let Ok(mut w) = writer_for_pty_thread.lock() {
                    let _ = w.write_all(&data);
                    let _ = w.flush();
                }
            }
        });

        let (ghostty_buf, incoming_bytes_clone) = GhosttyBuffer::new(
            initial_width as u16,
            initial_height as u16,
            scrollback_limit,
            pty_write_tx,
            default_cursor,
        );

        let ghostty_buffer = Arc::new(Mutex::new(ghostty_buf));

        // Background PTY reader thread: just collects raw bytes into
        // incoming_bytes; the main thread processes them via is_dirty().
        let mut reader = pty_pair
            .master
            .try_clone_reader()
            .expect("Failed to clone PTY reader");

        thread::spawn(move || {
            let mut buffer = [0u8; 20_000];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        eprintln!("[TERMINAL] PTY reader received EOF");
                        break;
                    }
                    Ok(n) => {
                        if let Ok(mut incoming) = incoming_bytes_clone.lock() {
                            incoming.extend(buffer[..n].iter().copied());
                        }
                        crate::pty_waker::wake();
                    }
                    Err(err) => {
                        eprintln!("[TERMINAL] Error reading from PTY: {}", err);
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });

        Terminal {
            master: pty_pair.master,
            writer,
            child,
            ghostty_buffer,
            width: initial_width,
            height: initial_height,
            shell_config,
            selection: Arc::new(Mutex::new(None)),
            command_history: Arc::new(Mutex::new(Vec::new())),
            output_history: Arc::new(Mutex::new(Vec::new())),
            current_command: Arc::new(Mutex::new(String::new())),
        }
    }

    pub(crate) fn set_size(&mut self, new_width: u32, new_height: u32, cell_width_px: u32, cell_height_px: u32) {
        self.width = new_width;
        self.height = new_height;

        if let Ok(mut gb) = self.ghostty_buffer.lock() {
            gb.resize(new_width as usize, new_height as usize);
        }

        let new_size = PtySize {
            rows: new_height as u16,
            cols: new_width as u16,
            pixel_width: (new_width * cell_width_px) as u16,
            pixel_height: (new_height * cell_height_px) as u16,
        };

        if let Err(err) = self.master.resize(new_size) {
            eprintln!("[TERMINAL] Failed to resize PTY: {}", err);
        } else {
            eprintln!("[TERMINAL] Resized PTY to {}x{} ({}x{} px)", new_width, new_height, new_size.pixel_width, new_size.pixel_height);
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

        let app_cursor_mode = self
            .ghostty_buffer
            .lock()
            .map(|gb| gb.application_cursor_keys())
            .unwrap_or(false);

        let is_arrow_key = keys.len() == 3
            && keys[0] == 27
            && keys[1] == b'['
            && matches!(keys[2], b'A' | b'B' | b'C' | b'D');

        if let Ok(mut writer) = self.writer.lock() {
            if app_cursor_mode && is_arrow_key {
                let translated = [27, b'O', keys[2]];
                if let Err(err) = writer.write_all(&translated) {
                    eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
                }
            } else if let Err(err) = writer.write_all(keys) {
                eprintln!("[TERMINAL] Failed to write key to PTY: {}", err);
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
        let bracketed = self
            .ghostty_buffer
            .lock()
            .map(|gb| gb.bracketed_paste())
            .unwrap_or(false);

        if let Ok(mut writer) = self.writer.lock() {
            if bracketed {
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

    pub(crate) fn send_mouse_event(
        &mut self,
        button: u8,
        col: u32,
        row: u32,
        pressed: bool,
    ) {
        let (tracking_mode, sgr_mode) = {
            let Ok(gb) = self.ghostty_buffer.lock() else {
                return;
            };
            (gb.mouse_tracking_mode(), gb.mouse_sgr_mode())
        };

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

    pub(crate) fn is_mouse_tracking_enabled(&self) -> bool {
        self.ghostty_buffer
            .lock()
            .map(|gb| gb.is_mouse_tracking())
            .unwrap_or(false)
    }

    pub(crate) fn start_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut sel) = self.selection.try_lock() {
            if let Ok(gb) = self.ghostty_buffer.try_lock() {
                let scroll_offset = gb.scroll_offset();
                let scrollback_len = gb.scrollback_len();
                *sel = Some(Selection::new(col, row, scroll_offset, scrollback_len));
            }
        }
    }

    pub(crate) fn update_selection(&mut self, col: usize, row: usize) {
        if let Ok(mut selection) = self.selection.try_lock() {
            if let Some(ref mut sel) = *selection {
                if let Ok(gb) = self.ghostty_buffer.try_lock() {
                    let scroll_offset = gb.scroll_offset();
                    let scrollback_len = gb.scrollback_len();
                    sel.update_end(col, row, scroll_offset, scrollback_len);
                }
            }
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = None;
        }
    }

    pub(crate) fn select_word_at(&mut self, col: usize, row: usize) {
        let (scroll_offset, scrollback_len, width, height) = {
            let Ok(gb) = self.ghostty_buffer.try_lock() else {
                return;
            };
            (gb.scroll_offset(), gb.scrollback_len(), gb.width(), gb.height())
        };

        if row >= height {
            return;
        }

        let abs_row = scrollback_len.saturating_sub(scroll_offset) + row;

        let is_word_char = |ch: char| ch.is_alphanumeric() || ch == '_';

        // Read the clicked cell
        let clicked_chars = {
            let Ok(gb) = self.ghostty_buffer.try_lock() else {
                return;
            };
            gb.graphemes_at(col, abs_row)
        };
        let clicked_ch = clicked_chars.first().copied().unwrap_or('\0');

        if clicked_ch == '\0' || clicked_ch == ' ' || !is_word_char(clicked_ch) {
            return;
        }

        let mut start_col = col;
        while start_col > 0 {
            let chars = {
                let Ok(gb) = self.ghostty_buffer.try_lock() else { break };
                gb.graphemes_at(start_col - 1, abs_row)
            };
            let ch = chars.first().copied().unwrap_or('\0');
            if is_word_char(ch) && ch != ' ' && ch != '\0' {
                start_col -= 1;
            } else {
                break;
            }
        }

        let mut end_col = col;
        while end_col < width.saturating_sub(1) {
            let chars = {
                let Ok(gb) = self.ghostty_buffer.try_lock() else { break };
                gb.graphemes_at(end_col + 1, abs_row)
            };
            let ch = chars.first().copied().unwrap_or('\0');
            if is_word_char(ch) && ch != ' ' && ch != '\0' {
                end_col += 1;
            } else {
                break;
            }
        }

        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection {
                start_col,
                start_row: abs_row,
                end_col,
                end_row: abs_row,
            });
        }
    }

    pub(crate) fn select_line_at(&mut self, row: usize) {
        let (scroll_offset, scrollback_len, width, height) = {
            let Ok(gb) = self.ghostty_buffer.try_lock() else {
                return;
            };
            (gb.scroll_offset(), gb.scrollback_len(), gb.width(), gb.height())
        };

        if row >= height {
            return;
        }

        let abs_row = scrollback_len.saturating_sub(scroll_offset) + row;

        if let Ok(mut sel) = self.selection.try_lock() {
            *sel = Some(Selection {
                start_col: 0,
                start_row: abs_row,
                end_col: width.saturating_sub(1),
                end_row: abs_row,
            });
        }
    }

    pub(crate) fn extend_selection_up(&mut self) -> bool {
        self.extend_selection_vertical(-1)
    }

    pub(crate) fn extend_selection_down(&mut self) -> bool {
        self.extend_selection_vertical(1)
    }

    pub(crate) fn extend_selection_left(&mut self) -> bool {
        self.extend_selection_horizontal(-1)
    }

    pub(crate) fn extend_selection_right(&mut self) -> bool {
        self.extend_selection_horizontal(1)
    }

    pub(crate) fn extend_selection_page_up(&mut self) -> bool {
        let height = self.height as i32;
        self.extend_selection_vertical(-height)
    }

    pub(crate) fn extend_selection_page_down(&mut self) -> bool {
        let height = self.height as i32;
        self.extend_selection_vertical(height)
    }

    fn extend_selection_vertical(&mut self, delta_rows: i32) -> bool {
        let (scroll_offset, scrollback_len, screen_height, cursor_x, cursor_y) = {
            let Ok(gb) = self.ghostty_buffer.try_lock() else {
                return false;
            };
            (
                gb.scroll_offset(),
                gb.scrollback_len(),
                gb.height(),
                gb.cursor_x(),
                gb.cursor_y(),
            )
        };

        let mut selection = match self.selection.try_lock() {
            Ok(sel) => sel,
            Err(_) => return false,
        };

        if selection.is_none() {
            let absolute_row = scrollback_len.saturating_sub(scroll_offset) + cursor_y;
            *selection = Some(Selection {
                start_col: cursor_x,
                start_row: absolute_row,
                end_col: cursor_x,
                end_row: absolute_row,
            });
        }

        let mut needs_render = false;
        if let Some(ref mut sel) = *selection {
            let new_absolute_row =
                (sel.end_row as i32 + delta_rows).max(0) as usize;
            let max_absolute_row = scrollback_len + screen_height.saturating_sub(1);
            sel.end_row = new_absolute_row.min(max_absolute_row);
            needs_render = true;

            let visible_base = scrollback_len.saturating_sub(scroll_offset);
            let selection_screen_row =
                sel.end_row as i32 - visible_base as i32;

            let needs_scroll_up = selection_screen_row < 0;
            let needs_scroll_down = selection_screen_row >= screen_height as i32;

            drop(selection);

            if let Ok(mut gb) = self.ghostty_buffer.try_lock() {
                if needs_scroll_up {
                    gb.scroll_view_up((-selection_screen_row) as usize);
                } else if needs_scroll_down {
                    gb.scroll_view_down(
                        (selection_screen_row - screen_height as i32 + 1) as usize,
                    );
                }
            }
        }

        needs_render
    }

    fn extend_selection_horizontal(&mut self, delta_cols: i32) -> bool {
        let (scroll_offset, scrollback_len, screen_width, cursor_x, cursor_y) = {
            let Ok(gb) = self.ghostty_buffer.try_lock() else {
                return false;
            };
            (
                gb.scroll_offset(),
                gb.scrollback_len(),
                gb.width(),
                gb.cursor_x(),
                gb.cursor_y(),
            )
        };

        let mut selection = match self.selection.try_lock() {
            Ok(sel) => sel,
            Err(_) => return false,
        };

        if selection.is_none() {
            let absolute_row = scrollback_len.saturating_sub(scroll_offset) + cursor_y;
            *selection = Some(Selection {
                start_col: cursor_x,
                start_row: absolute_row,
                end_col: cursor_x,
                end_row: absolute_row,
            });
        }

        let mut needs_render = false;
        if let Some(ref mut sel) = *selection {
            let new_col = (sel.end_col as i32 + delta_cols).max(0) as usize;
            sel.end_col = new_col.min(screen_width.saturating_sub(1));
            needs_render = true;
        }

        needs_render
    }

    pub(crate) fn get_selected_text(&self) -> Option<String> {
        let selection = self.selection.try_lock().ok()?;
        let sel = (*selection)?;
        drop(selection);

        let gb = self.ghostty_buffer.try_lock().ok()?;
        let (start_col, start_row, end_col, end_row) = sel.normalized();
        let width = gb.width();
        let total_rows = gb.scrollback_len() + gb.height();

        let mut text = String::new();
        for row in start_row..=end_row {
            if row >= total_rows {
                break;
            }
            let line_start = if row == start_row { start_col } else { 0 };
            let line_end = if row == end_row {
                end_col.min(width.saturating_sub(1))
            } else {
                width.saturating_sub(1)
            };

            let mut line = String::new();
            for col in line_start..=line_end {
                let chars = gb.graphemes_at(col, row);
                if chars.is_empty() {
                    continue;
                }
                // Skip wide-char spacers (cell_width 0)
                if gb.cell_width_at(col, row) == 0 {
                    continue;
                }
                for ch in chars {
                    if ch != '\0' {
                        line.push(ch);
                    }
                }
            }

            let trimmed_line = line.trim_end_matches(|c: char| c == ' ' || c == '\0');
            text.push_str(trimmed_line);
            if row < end_row {
                text.push('\n');
            }
        }

        Some(text)
    }

    pub(crate) fn get_cwd(&self) -> Option<std::path::PathBuf> {
        if let Some(pid) = self.child.process_id() {
            #[cfg(target_os = "linux")]
            {
                let proc_path = format!("/proc/{}/cwd", pid);
                if let Ok(cwd) = std::fs::read_link(&proc_path) {
                    return Some(cwd);
                }
            }

            #[cfg(target_os = "macos")]
            {
                if let Some(cwd) = macos_get_proc_cwd(pid) {
                    return Some(cwd);
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                use sysinfo::{Pid, System};
                let mut system = System::new();
                system.refresh_processes(
                    sysinfo::ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
                    false,
                );
                if let Some(process) = system.process(Pid::from_u32(pid)) {
                    return process.cwd().map(|p| p.to_path_buf());
                }
            }
        }
        None
    }

    #[allow(dead_code)]
    pub(crate) fn add_command_to_history(&self, command: String) {
        if let Ok(mut history) = self.command_history.lock() {
            if !command.trim().is_empty()
                && (history.is_empty() || history.last() != Some(&command))
            {
                history.push(command);
                if history.len() > MAX_COMMAND_HISTORY {
                    history.remove(0);
                }
            }
        }
    }

    pub(crate) fn capture_output_history(&self) {
        let Ok(gb) = self.ghostty_buffer.lock() else {
            return;
        };
        let scrollback_rows = gb.scrollback_len();
        let rows = gb.height();
        let width = gb.width();
        let total_rows = scrollback_rows + rows;

        let mut lines = Vec::new();
        for y in 0..total_rows {
            let mut line = String::new();
            let mut x = 0;
            while x < width {
                let w = gb.cell_width_at(x, y);
                let chars = gb.graphemes_at(x, y);
                for ch in &chars {
                    if *ch != '\0' {
                        line.push(*ch);
                    }
                }
                x += w.max(1) as usize;
            }
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
        drop(gb);

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

    pub(crate) fn get_command_history(&self) -> Vec<String> {
        history::read_shell_history(MAX_COMMAND_HISTORY)
    }

    pub(crate) fn set_command_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.command_history.lock() {
            *h = history;
        }
    }

    pub(crate) fn get_output_history(&self) -> Vec<String> {
        self.output_history
            .lock()
            .ok()
            .map(|h| h.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_output_history(&self, history: Vec<String>) {
        if let Ok(mut h) = self.output_history.lock() {
            *h = history.clone();
        }
        if history.is_empty() {
            return;
        }
        if let Ok(mut gb) = self.ghostty_buffer.lock() {
            for line in &history {
                gb.terminal.vt_write(line.as_bytes());
                gb.terminal.vt_write(b"\r\n");
            }
            gb.dirty = true;
        }
    }

    pub(crate) fn is_foreground_process_running(&self) -> bool {
        let Some(shell_pid) = self.child.process_id() else {
            eprintln!("[TERMINAL] Could not get shell PID");
            return false;
        };

        use sysinfo::{Pid, System};
        let mut system = System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, false);
        let shell_pid_obj = Pid::from_u32(shell_pid);

        for (_pid, process) in system.processes() {
            if let Some(parent_pid) = process.parent() {
                if parent_pid == shell_pid_obj {
                    return true;
                }
            }
        }
        false
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(target_os = "macos")]
fn macos_get_proc_cwd(pid: u32) -> Option<std::path::PathBuf> {
    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut core::ffi::c_void,
            buffersize: i32,
        ) -> i32;
    }

    const PROC_PIDVNODEPATHINFO: i32 = 9;
    const BUF_SIZE: usize = 2352;
    const PATH_OFFSET: usize = 152;
    const MAXPATHLEN: usize = 1024;

    let mut buf = vec![0u8; BUF_SIZE];
    let ret = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDVNODEPATHINFO,
            0,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            BUF_SIZE as i32,
        )
    };

    if ret > 0 {
        let path_bytes = &buf[PATH_OFFSET..PATH_OFFSET + MAXPATHLEN];
        let null_pos = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAXPATHLEN);
        if null_pos > 0 {
            if let Ok(path_str) = std::str::from_utf8(&path_bytes[..null_pos]) {
                return Some(std::path::PathBuf::from(path_str));
            }
        }
    }
    None
}
