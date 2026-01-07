use crate::ansi::{DEFAULT_BG_COLOR, DEFAULT_FG_COLOR};
use sdl3::pixels::Color;

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg_color: Color,
    pub bg_color: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
        }
    }
}

#[derive(Clone)]
pub struct ScreenBuffer {
    cells: Vec<Vec<Cell>>,
    width: usize,
    height: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub fg_color: Color,
    pub bg_color: Color,
    // Scrolling region (top and bottom margins, 0-based, inclusive)
    // None means the entire screen is the scrolling region
    scroll_region: Option<(usize, usize)>,
    // Saved cursor position for CSI s/u (save/restore cursor)
    saved_cursor_x: usize,
    saved_cursor_y: usize,
    // Dirty flag to track if content has changed since last render
    dirty: bool,
    // Scrollback buffer - stores historical lines that scrolled off the screen
    scrollback_buffer: Vec<Vec<Cell>>,
    // Maximum number of lines to keep in scrollback (0 means disabled)
    scrollback_limit: usize,
    // Current scroll offset (0 means viewing the live terminal, positive means scrolled back)
    pub scroll_offset: usize,
    // Origin mode (DECOM) - when enabled, cursor positioning is relative to scroll region
    origin_mode: bool,
    // Pending wrap state - cursor is past last column, wrap on next character
    pub(crate) pending_wrap: bool,
}

impl ScreenBuffer {
    pub fn new_with_scrollback(width: usize, height: usize, scrollback_limit: usize) -> Self {
        Self {
            cells: vec![vec![Cell::default(); width]; height],
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
            scroll_region: None,
            saved_cursor_x: 0,
            saved_cursor_y: 0,
            dirty: true,
            scrollback_buffer: Vec::new(),
            scrollback_limit,
            scroll_offset: 0,
            origin_mode: false,
            pending_wrap: false,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        // Create new buffer
        let mut new_cells = vec![vec![Cell::default(); width]; height];

        // Copy old content
        let copy_height = self.height.min(height);
        let copy_width = self.width.min(width);
        for (y, row) in new_cells.iter_mut().enumerate().take(copy_height) {
            row[..copy_width].copy_from_slice(&self.cells[y][..copy_width]);
        }

        self.cells = new_cells;
        self.width = width;
        self.height = height;

        // Keep cursor in bounds
        self.cursor_x = self.cursor_x.min(width.saturating_sub(1));
        self.cursor_y = self.cursor_y.min(height.saturating_sub(1));
        self.dirty = true;
    }

    pub fn put_char(&mut self, ch: char) {
        // Handle pending wrap from previous character
        if self.pending_wrap {
            self.cursor_x = 0;
            self.cursor_y += 1;
            self.pending_wrap = false;

            // Scroll if we've gone past the bottom
            if self.cursor_y >= self.height {
                self.cursor_y = self.height - 1;
                self.scroll_up(1);
            }
        }

        if self.cursor_y < self.height && self.cursor_x < self.width {
            self.cells[self.cursor_y][self.cursor_x] = Cell {
                ch,
                fg_color: self.fg_color,
                bg_color: self.bg_color,
            };
            self.cursor_x += 1;
            self.dirty = true;

            // Set pending wrap if we're past the last column
            if self.cursor_x >= self.width {
                self.cursor_x = self.width - 1;
                self.pending_wrap = true;
            }
        }
    }

    pub fn newline(&mut self) {
        self.pending_wrap = false;
        self.cursor_y += 1;

        // Get the scrolling region bounds
        let (_scroll_top, scroll_bottom) = self.scroll_region.unwrap_or((0, self.height - 1));

        // Only scroll if we're past the bottom of the scrolling region
        if self.cursor_y > scroll_bottom {
            self.cursor_y = scroll_bottom;
            self.scroll_up(1);
        } else if self.cursor_y >= self.height {
            self.cursor_y = self.height - 1;
            self.scroll_up(1);
        }
        self.dirty = true;
    }

    pub fn tab(&mut self) {
        self.pending_wrap = false;
        // Tab to next multiple of 8
        let next_tab = ((self.cursor_x / 8) + 1) * 8;
        self.cursor_x = next_tab.min(self.width - 1);
        self.dirty = true;
    }

    pub fn move_cursor_to(&mut self, x: usize, y: usize) {
        self.pending_wrap = false;
        self.cursor_x = x.min(self.width.saturating_sub(1));

        // In origin mode, y is relative to the scroll region's top
        if self.origin_mode {
            if let Some((top, bottom)) = self.scroll_region {
                self.cursor_y = (top + y).min(bottom);
            } else {
                self.cursor_y = y.min(self.height.saturating_sub(1));
            }
        } else {
            self.cursor_y = y.min(self.height.saturating_sub(1));
        }
        self.dirty = true;
    }

    pub fn move_cursor_up(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_y = self.cursor_y.saturating_sub(n);
        self.dirty = true;
    }

    pub fn move_cursor_down(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_y = (self.cursor_y + n).min(self.height - 1);
        self.dirty = true;
    }

    pub fn move_cursor_right(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_x = (self.cursor_x + n).min(self.width - 1);
        self.dirty = true;
    }

    pub fn move_cursor_left(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_x = self.cursor_x.saturating_sub(n);
        self.dirty = true;
    }

    pub fn save_cursor(&mut self) {
        self.saved_cursor_x = self.cursor_x;
        self.saved_cursor_y = self.cursor_y;
    }

    pub fn restore_cursor(&mut self) {
        self.cursor_x = self.saved_cursor_x.min(self.width.saturating_sub(1));
        self.cursor_y = self.saved_cursor_y.min(self.height.saturating_sub(1));
        self.dirty = true;
    }

    pub fn clear_screen(&mut self) {
        self.pending_wrap = false;
        for row in &mut self.cells {
            for cell in row {
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        // Reset scrolling region when clearing screen
        self.scroll_region = None;
        self.dirty = true;
    }

    pub fn clear_from_cursor_to_end(&mut self) {
        // Clear from cursor to end of line
        if self.cursor_y < self.height {
            for x in self.cursor_x..self.width {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }

            // Clear all lines below
            for y in (self.cursor_y + 1)..self.height {
                for x in 0..self.width {
                    let cell = &mut self.cells[y][x];
                    cell.ch = ' ';
                    cell.fg_color = self.fg_color;
                    cell.bg_color = self.bg_color;
                }
            }
        }
        self.dirty = true;
    }

    pub fn clear_from_start_to_cursor(&mut self) {
        // Clear all lines above cursor
        for y in 0..self.cursor_y {
            for x in 0..self.width {
                let cell = &mut self.cells[y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }

        // Clear from start of current line to cursor
        if self.cursor_y < self.height {
            for x in 0..=self.cursor_x.min(self.width - 1) {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn clear_line(&mut self) {
        if self.cursor_y < self.height {
            for x in 0..self.width {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn clear_line_from_cursor(&mut self) {
        if self.cursor_y < self.height {
            for x in self.cursor_x..self.width {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn clear_line_to_cursor(&mut self) {
        if self.cursor_y < self.height {
            for x in 0..=self.cursor_x.min(self.width - 1) {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn erase_chars(&mut self, n: usize) {
        // Erase n characters starting at cursor position (ECH - Erase Character)
        // Characters are replaced with spaces, cursor doesn't move
        if self.cursor_y < self.height {
            let end_x = (self.cursor_x + n).min(self.width);
            for x in self.cursor_x..end_x {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn clear_region(&mut self, top: usize, bottom: usize) {
        // Clear rows from top to bottom (inclusive, 0-based)
        for y in top..=bottom.min(self.height - 1) {
            for x in 0..self.width {
                self.cells[y][x] = Cell {
                    ch: ' ',
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                };
            }
        }
        self.dirty = true;
    }

    pub fn insert_chars(&mut self, n: usize) {
        // ICH - Insert Character(s)
        // Insert n blank characters at cursor position
        // Characters from cursor to end of line shift right
        // Characters shifted past end of line are lost
        if self.cursor_y >= self.height {
            return;
        }

        // Clamp cursor_x to valid range
        let cursor_x = self.cursor_x.min(self.width);

        let n = n.min(self.width.saturating_sub(cursor_x)); // Can't insert beyond line width
        if n == 0 {
            return;
        }

        // Shift existing characters to the right
        let row = &mut self.cells[self.cursor_y];

        // Move characters from right to left to avoid overwriting
        // Start from the rightmost position that will be affected
        for x in (cursor_x..self.width.saturating_sub(n)).rev() {
            let new_pos = x + n;
            if new_pos < self.width {
                row[new_pos] = row[x];
            }
        }

        // Fill inserted positions with blank characters
        let end = (cursor_x + n).min(self.width);
        for cell in row.iter_mut().take(end).skip(cursor_x) {
            *cell = Cell {
                ch: ' ',
                fg_color: self.fg_color,
                bg_color: self.bg_color,
            };
        }

        self.dirty = true;
    }

    pub fn delete_chars(&mut self, n: usize) {
        // DCH - Delete Character(s)
        // Delete n characters starting at cursor position
        // Characters to the right of deleted characters shift left
        // Blank characters are added at end of line
        if self.cursor_y >= self.height {
            return;
        }

        // Clamp cursor_x to valid range
        let cursor_x = self.cursor_x.min(self.width);

        let n = n.min(self.width.saturating_sub(cursor_x)); // Can't delete beyond line width
        if n == 0 {
            return;
        }

        let row = &mut self.cells[self.cursor_y];

        // Shift characters from right side to the left
        for x in cursor_x..self.width {
            let source_pos = x + n;
            if source_pos < self.width {
                row[x] = row[source_pos];
            } else {
                // Fill with blank at the end
                row[x] = Cell {
                    ch: ' ',
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                };
            }
        }

        self.dirty = true;
    }

    pub fn scroll_up(&mut self, n: usize) {
        // Get the scrolling region bounds
        let (scroll_top, scroll_bottom) = self.scroll_region.unwrap_or((0, self.height - 1));

        let region_height = scroll_bottom - scroll_top + 1;
        if n >= region_height {
            self.clear_region(scroll_top, scroll_bottom);
            return;
        }

        // If scrollback is enabled and we're scrolling the full screen, save to scrollback
        if self.scrollback_limit > 0 && scroll_top == 0 && scroll_bottom == self.height - 1 {
            // Save the top n lines to scrollback buffer
            for i in 0..n {
                if scroll_top + i < self.cells.len() {
                    self.scrollback_buffer.push(self.cells[scroll_top + i].clone());
                }
            }

            // Trim scrollback buffer if it exceeds the limit
            if self.scrollback_buffer.len() > self.scrollback_limit {
                let excess = self.scrollback_buffer.len() - self.scrollback_limit;
                self.scrollback_buffer.drain(0..excess);
            }
        }

        // Move lines up within the scrolling region
        for y in scroll_top..=(scroll_bottom - n) {
            self.cells[y] = self.cells[y + n].clone();
        }

        // Clear bottom lines of the scrolling region
        for y in (scroll_bottom - n + 1)..=scroll_bottom {
            for x in 0..self.width {
                let cell = &mut self.cells[y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }

        // When terminal scrolls (app writes), reset to live view
        self.scroll_offset = 0;
        self.dirty = true;
    }

    pub fn scroll_down(&mut self, n: usize) {
        // Get the scrolling region bounds
        let (scroll_top, scroll_bottom) = self.scroll_region.unwrap_or((0, self.height - 1));

        let region_height = scroll_bottom - scroll_top + 1;
        if n >= region_height {
            self.clear_region(scroll_top, scroll_bottom);
            return;
        }

        // Move lines down within the scrolling region (iterate in reverse to avoid overwriting)
        for y in (scroll_top + n..=scroll_bottom).rev() {
            self.cells[y] = self.cells[y - n].clone();
        }

        // Clear top lines of the scrolling region
        for y in scroll_top..(scroll_top + n) {
            for x in 0..self.width {
                let cell = &mut self.cells[y][x];
                cell.ch = ' ';
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }
        self.dirty = true;
    }

    pub fn insert_lines(&mut self, n: usize) {
        // Insert n blank lines at the cursor position
        // Lines below cursor are pushed down, lines pushed off bottom of scrolling region are lost
        if self.cursor_y >= self.height {
            return;
        }

        // Get the scrolling region bounds
        let (scroll_top, scroll_bottom) = self.scroll_region.unwrap_or((0, self.height - 1));

        // Only operate within the scrolling region
        if self.cursor_y < scroll_top || self.cursor_y > scroll_bottom {
            return;
        }

        let n = n.min(scroll_bottom - self.cursor_y + 1);

        // Move lines down from cursor position to bottom of scrolling region
        for y in (self.cursor_y..=(scroll_bottom - n)).rev() {
            self.cells[y + n] = self.cells[y].clone();
        }

        // Clear the newly inserted lines at cursor position
        for y in self.cursor_y..(self.cursor_y + n) {
            for x in 0..self.width {
                self.cells[y][x] = Cell {
                    ch: ' ',
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                };
            }
        }
        self.dirty = true;
    }

    pub fn delete_lines(&mut self, n: usize) {
        // Delete n lines starting at the cursor position
        // Lines below are pulled up, blank lines are added at bottom of scrolling region
        if self.cursor_y >= self.height {
            return;
        }

        // Get the scrolling region bounds
        let (scroll_top, scroll_bottom) = self.scroll_region.unwrap_or((0, self.height - 1));

        // Only operate within the scrolling region
        if self.cursor_y < scroll_top || self.cursor_y > scroll_bottom {
            return;
        }

        let n = n.min(scroll_bottom - self.cursor_y + 1);

        // Move lines up from below cursor within scrolling region
        for y in self.cursor_y..=(scroll_bottom - n) {
            self.cells[y] = self.cells[y + n].clone();
        }

        // Clear the lines at the bottom of scrolling region
        for y in (scroll_bottom - n + 1)..=scroll_bottom {
            for x in 0..self.width {
                self.cells[y][x] = Cell {
                    ch: ' ',
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                };
            }
        }
        self.dirty = true;
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        // Set the scrolling region (1-based to 0-based conversion happens in terminal.rs)
        // If top and bottom define the entire screen, disable scrolling region
        if top == 0 && bottom >= self.height - 1 {
            self.scroll_region = None;
        } else {
            let top = top.min(self.height - 1);
            let bottom = bottom.min(self.height - 1);
            if top <= bottom {
                self.scroll_region = Some((top, bottom));
            }
        }
    }

    pub fn reset_scroll_region(&mut self) {
        self.scroll_region = None;
    }

    pub fn set_origin_mode(&mut self, enabled: bool) {
        self.origin_mode = enabled;
    }

    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        if y < self.height && x < self.width {
            Some(&self.cells[y][x])
        } else {
            None
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn scrollback_limit(&self) -> usize {
        self.scrollback_limit
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    // Scrollback control methods

    /// Check if we're viewing live content (not scrolled back)
    pub fn is_at_bottom(&self) -> bool {
        self.scroll_offset == 0
    }

    /// Scroll the view up (backward in time) by n lines
    pub fn scroll_view_up(&mut self, n: usize) {
        // Limit scroll to show scrollback but never hide ALL current screen content
        // Allow scrolling back through the entire scrollback buffer
        let max_scroll = self.scrollback_buffer.len();
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
        self.dirty = true;
    }

    /// Scroll the view down (forward in time) by n lines
    pub fn scroll_view_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.dirty = true;
    }

    /// Jump to the bottom (live view)

    /// Get a cell from the scrollback buffer or current screen
    /// y is relative to the current view (accounting for scroll offset)
    pub fn get_cell_with_scrollback(&self, x: usize, y: usize) -> Option<&Cell> {
        if self.scroll_offset == 0 {
            // Normal view - just return from current cells
            return self.get_cell(x, y);
        }

        // Safety check: if scroll_offset is invalid, fall back to current screen
        if self.scroll_offset > self.scrollback_buffer.len() {
            return self.get_cell(x, y);
        }

        // When scrolled back, we show scrollback lines at the top, then current screen below
        // But we need to ensure we don't show empty space if scrollback is smaller than screen
        let lines_from_scrollback = self.scroll_offset.min(self.height);

        if y < lines_from_scrollback {
            // This row should come from the scrollback buffer
            let scrollback_y = self.scrollback_buffer.len().saturating_sub(self.scroll_offset) + y;
            if scrollback_y < self.scrollback_buffer.len() && x < self.width {
                return Some(&self.scrollback_buffer[scrollback_y][x]);
            }
        } else {
            // This row should come from the current screen buffer
            let screen_y = y - lines_from_scrollback;
            if screen_y < self.height {
                return self.get_cell(x, screen_y);
            }
        }

        None
    }
}
