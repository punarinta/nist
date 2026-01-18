use crate::ansi::{DEFAULT_BG_COLOR, DEFAULT_FG_COLOR};
use sdl3::pixels::Color;
use unicode_width::UnicodeWidthChar;

/// Translate a character through DEC Special Graphics character set
/// This is used for box drawing and special symbols
/// Based on VT220 DEC Special Graphics table
fn translate_dec_special_graphics(ch: char) -> char {
    match ch {
        '_' => ' ',        // Blank (space)
        '`' => '◆',        // Diamond
        'a' => '▒',        // Checkerboard (medium shade)
        'b' => '\u{2409}', // HT symbol
        'c' => '\u{240C}', // FF symbol
        'd' => '\u{240D}', // CR symbol
        'e' => '\u{240A}', // LF symbol
        'f' => '°',        // Degree symbol
        'g' => '±',        // Plus/minus
        'h' => '\u{2424}', // NL symbol
        'i' => '\u{240B}', // VT symbol
        'j' => '┘',        // Lower right corner
        'k' => '┐',        // Upper right corner
        'l' => '┌',        // Upper left corner
        'm' => '└',        // Lower left corner
        'n' => '┼',        // Crossing lines
        'o' => '⎺',        // Scan line 1
        'p' => '⎻',        // Scan line 3
        'q' => '─',        // Horizontal line (scan 5)
        'r' => '⎼',        // Scan line 7
        's' => '⎽',        // Scan line 9
        't' => '├',        // Left tee
        'u' => '┤',        // Right tee
        'v' => '┴',        // Bottom tee
        'w' => '┬',        // Top tee
        'x' => '│',        // Vertical bar
        'y' => '≤',        // Less than or equal
        'z' => '≥',        // Greater than or equal
        '{' => 'π',        // Pi
        '|' => '≠',        // Not equal
        '}' => '£',        // UK pound sign
        '~' => '·',        // Centered dot (bullet)
        _ => ch,           // Pass through all other characters unchanged
    }
}

/// Character set designation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharSet {
    Ascii, // 'B' - US ASCII (default)
    DecSpecialGraphics, // '0' - DEC Special Graphics (box drawing)
           // Other charsets can be added as needed
}

impl Default for CharSet {
    fn default() -> Self {
        CharSet::Ascii
    }
}

/// Cursor style as set by DECSCUSR escape sequences
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl Default for CursorStyle {
    fn default() -> Self {
        CursorStyle::BlinkingBlock
    }
}

impl CursorStyle {
    /// Convert from settings string ("pipe", "block", etc.) to CursorStyle
    pub fn from_settings_string(s: &str) -> Self {
        match s {
            "pipe" | "bar" => CursorStyle::SteadyBar,
            "underline" => CursorStyle::SteadyUnderline,
            "block" => CursorStyle::SteadyBlock,
            "blinking_block" => CursorStyle::BlinkingBlock,
            "blinking_bar" | "blinking_pipe" => CursorStyle::BlinkingBar,
            "blinking_underline" => CursorStyle::BlinkingUnderline,
            _ => CursorStyle::SteadyBar, // Default to pipe/bar for backwards compatibility
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub ch: char,                   // Primary character (4 bytes)
    pub extended: Option<Box<str>>, // For complex graphemes (emojis with modifiers)
    pub fg_color: Color,
    pub bg_color: Color,
    pub width: u8, // 1 for normal chars, 2 for wide/emoji chars
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub blink: bool,
    pub reverse: bool,
    pub invisible: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            extended: None,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
            width: 1,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            blink: false,
            reverse: false,
            invisible: false,
        }
    }
}

/// Check if a character is a special symbol that needs scaling in rendering
#[inline]
pub fn is_special_symbol(ch: char) -> bool {
    let codepoint = ch as u32;
    // Exclude Block Elements (0x2580..=0x259F) and Box Drawing (0x2500..=0x257F)
    // as they need to fill exactly one cell without scaling for ASCII art
    matches!(codepoint,
        0x2190..=0x21FF |  // Arrows (includes →, ←, ↑, ↓)
        0x2200..=0x22FF |  // Mathematical Operators (includes ∀, ∃, ∈, ∞)
        0x2300..=0x23FF |  // Miscellaneous Technical (includes ⎿)
        0x2400..=0x243F |  // Control Pictures (includes ␀, ␣)
        0x2460..=0x24FF |  // Enclosed Alphanumerics (includes ①, ②, ③)
        0x25A0..=0x25FF |  // Geometric Shapes (includes ■)
        0x2700..=0x27BF |  // Dingbats (includes ❯, ❌)
        0x27C0..=0x27EF |  // Miscellaneous Mathematical Symbols-A
        0x27F0..=0x27FF |  // Supplemental Arrows-A
        0x2800..=0x28FF |  // Braille Patterns (includes ⠴)
        0x2900..=0x297F |  // Supplemental Arrows-B
        0x2980..=0x29FF |  // Miscellaneous Mathematical Symbols-B
        0x2A00..=0x2AFF |  // Supplemental Mathematical Operators
        0x2B00..=0x2BFF |  // Miscellaneous Symbols and Arrows
        0xFF00..=0xFFEF    // Halfwidth and Fullwidth Forms (includes ･)
    )
}

/// Check if a character is a block or box drawing character that needs cell-filling
#[inline]
pub fn is_block_or_box_drawing(ch: char) -> bool {
    let codepoint = ch as u32;
    matches!(codepoint,
        0x2500..=0x257F |  // Box Drawing (includes ┃, ╹, etc.)
        0x2580..=0x259F    // Block Elements (includes █, ▀, ▄, etc.)
    )
}

/// Check if a character is likely an emoji based on Unicode ranges
#[inline]
pub fn is_emoji_char(ch: char) -> bool {
    let codepoint = ch as u32;
    matches!(codepoint,
        // Emoticons
        0x1F600..=0x1F64F |
        // Miscellaneous Symbols and Pictographs
        0x1F300..=0x1F5FF |
        // Transport and Map Symbols
        0x1F680..=0x1F6FF |
        // Supplemental Symbols and Pictographs
        0x1F900..=0x1F9FF |
        // Symbols and Pictographs Extended-A
        0x1FA00..=0x1FA6F |
        0x1FA70..=0x1FAFF |
        // Miscellaneous Symbols (including weather, zodiac)
        0x2600..=0x26FF |
        // Enclosed Alphanumeric Supplement (includes circled numbers and regional indicators for flags)
        0x1F100..=0x1F1FF |
        // Enclosed Ideographic Supplement
        0x1F200..=0x1F2FF |
        // Variation Selectors (emoji presentation)
        0xFE00..=0xFE0F |
        // Mahjong Tiles, Domino Tiles
        0x1F000..=0x1F02F |
        // Playing Cards
        0x1F0A0..=0x1F0FF
    )
}

/// Check if a string contains an emoji (including combined emojis with modifiers)
#[inline]
pub fn is_emoji_grapheme(s: &str) -> bool {
    // Check if any character in the grapheme cluster is an emoji
    s.chars().any(is_emoji_char)
}

/// Check if a character is a CJK (Chinese, Japanese, Korean) character
#[inline]
pub fn is_cjk_char(ch: char) -> bool {
    let codepoint = ch as u32;
    matches!(codepoint,
        // CJK Unified Ideographs (most common Chinese characters)
        0x4E00..=0x9FFF |
        // CJK Extension A
        0x3400..=0x4DBF |
        // CJK Extension B
        0x20000..=0x2A6DF |
        // CJK Extension C
        0x2A700..=0x2B73F |
        // CJK Extension D
        0x2B740..=0x2B81F |
        // CJK Extension E
        0x2B820..=0x2CEAF |
        // CJK Extension F
        0x2CEB0..=0x2EBEF |
        // CJK Extension G
        0x30000..=0x3134F |
        // CJK Compatibility Ideographs
        0xF900..=0xFAFF |
        // CJK Compatibility Ideographs Supplement
        0x2F800..=0x2FA1F |
        // Hiragana (Japanese)
        0x3040..=0x309F |
        // Katakana (Japanese)
        0x30A0..=0x30FF |
        // Katakana Phonetic Extensions
        0x31F0..=0x31FF |
        // Hangul Syllables (Korean)
        0xAC00..=0xD7AF |
        // Hangul Jamo (Korean)
        0x1100..=0x11FF |
        // Hangul Jamo Extended-A
        0xA960..=0xA97F |
        // Hangul Jamo Extended-B
        0xD7B0..=0xD7FF
    )
}

/// Check if a string contains CJK characters
#[inline]
pub fn is_cjk_grapheme(s: &str) -> bool {
    // Check if any character in the grapheme cluster is CJK
    s.chars().any(is_cjk_char)
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
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub blink: bool,
    pub reverse: bool,
    pub invisible: bool,
    // Last character printed (for REP - Repeat command)
    last_char: Option<char>,
    // Tab stops (by default every 8 columns, but can be customized)
    // None means use default tab stops, Some(set) means custom tab stops
    tab_stops: Option<std::collections::HashSet<usize>>,
    // Reverse video mode (DECSCNM) - swap all foreground/background colors globally
    pub reverse_video_mode: bool,
    // Scrolling region (top and bottom margins, 0-based, inclusive)
    // None means the entire screen is the scrolling region
    scroll_region: Option<(usize, usize)>,
    // Saved cursor position for CSI s/u (save/restore cursor)
    saved_cursor_x: usize,
    saved_cursor_y: usize,
    // Dirty flag to track if content has changed since last render
    pub(crate) dirty: bool,
    // Scrollback buffer - stores historical lines that scrolled off the screen
    scrollback_buffer: Vec<Vec<Cell>>,
    // Maximum number of lines to keep in scrollback (0 means disabled)
    scrollback_limit: usize,
    // Current scroll offset (0 means viewing the live terminal, positive means scrolled back)
    pub scroll_offset: usize,
    // Origin mode (DECOM) - when enabled, cursor positioning is relative to scroll region
    origin_mode: bool,
    // Auto-wrap mode (DECAWM) - when enabled, cursor wraps at right margin
    auto_wrap_mode: bool,
    // Pending wrap state - cursor is past last column, wrap on next character
    pub(crate) pending_wrap: bool,
    // Cursor style (DECSCUSR)
    pub cursor_style: CursorStyle,
    // Character set designation - G0, G1, G2, G3
    g0_charset: CharSet,
    g1_charset: CharSet,
    g2_charset: CharSet,
    g3_charset: CharSet,
    // Current active character set (GL - typically G0 or G1)
    active_charset: usize, // 0 = G0, 1 = G1, 2 = G2, 3 = G3
    // Single shift state (for SS2/SS3 - affects next character only)
    single_shift: Option<usize>, // Some(2) = SS2 (G2), Some(3) = SS3 (G3)
    // Insert mode (IRM) - when enabled, inserting characters pushes existing ones to the right
    insert_mode: bool,
    // Automatic newline mode (LNM) - when enabled, CR (Ctrl-M) acts as CR+LF
    automatic_newline: bool,
}

impl ScreenBuffer {
    pub fn new_with_scrollback(width: usize, height: usize, scrollback_limit: usize, cursor_style: CursorStyle) -> Self {
        Self {
            cells: vec![vec![Cell::default(); width]; height],
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            blink: false,
            reverse: false,
            invisible: false,
            last_char: None,
            tab_stops: None,
            reverse_video_mode: false,
            g0_charset: CharSet::Ascii,
            g1_charset: CharSet::Ascii,
            g2_charset: CharSet::Ascii,
            g3_charset: CharSet::Ascii,
            active_charset: 0, // G0 is active by default
            single_shift: None,
            scroll_region: None,
            saved_cursor_x: 0,
            saved_cursor_y: 0,
            dirty: true,
            scrollback_buffer: Vec::new(),
            scrollback_limit,
            scroll_offset: 0,
            origin_mode: false,
            auto_wrap_mode: true,
            pending_wrap: false,
            cursor_style,
            insert_mode: false,
            automatic_newline: false,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        // Ensure minimum size to prevent buffer underflow
        let width = width.max(2);
        let height = height.max(2);

        let old_width = self.width;
        let old_height = self.height;
        let old_cursor_x = self.cursor_x;
        let old_cursor_y = self.cursor_y;

        eprintln!("[SCREEN_BUFFER] Resize: {}x{} -> {}x{}", old_width, old_height, width, height);
        eprintln!("[SCREEN_BUFFER] Old cursor: ({}, {})", old_cursor_x, old_cursor_y);

        // Create new buffer
        let mut new_cells = vec![vec![Cell::default(); width]; height];

        // Check if we need to rewrap content due to width change
        let needs_rewrap = old_width != width && width < old_width;

        // If width decreased, rewrap all content before handling height changes
        let (working_cells, rewrap_cursor_x, rewrap_cursor_y) = if needs_rewrap {
            self.rewrap_content(width, old_height)
        } else {
            (self.cells.clone(), old_cursor_x, old_cursor_y)
        };

        // Update old_height and cursor position if rewrapping changed them
        let old_height = working_cells.len();

        // Decide whether to use rewrapped cursor position or simple clamping
        // If buffer has minimal content, use clamping to avoid cursor jumping to (0,0)
        let has_meaningful_content = if needs_rewrap {
            // Count non-space characters in working cells
            let mut non_space_count = 0;
            let mut total_cells = 0;
            for row in &working_cells {
                for cell in row {
                    total_cells += 1;
                    if cell.ch != ' ' {
                        non_space_count += 1;
                    }
                }
            }
            // Consider it meaningful if >5% of cells have content
            non_space_count > (total_cells / 20)
        } else {
            true
        };

        let old_cursor_x = if has_meaningful_content {
            rewrap_cursor_x
        } else {
            old_cursor_x.min(width.saturating_sub(1))
        };
        let old_cursor_y = if has_meaningful_content {
            rewrap_cursor_y
        } else {
            old_cursor_y.min(height.saturating_sub(1))
        };

        // Handle height changes (or if rewrapping created more lines than fit)
        if old_height > height {
            // Terminal is getting shorter - need to decide which content to keep visible
            let lines_to_remove = old_height - height;

            // Strategy: Keep cursor visible and preserve content around it
            // After rewrapping, cursor position tells us what to keep visible
            // If cursor is in the bottom part, keep bottom content (recent output)
            // If cursor is in the top part, keep top content
            let cursor_in_bottom_half = old_cursor_y >= old_height / 2;

            let lines_to_scrollback = if cursor_in_bottom_half || needs_rewrap {
                // If rewrapping created more lines, OR cursor in bottom half -
                // keep bottom content, move top to scrollback
                lines_to_remove
            } else {
                // Cursor in top half and no rewrap - keep top content
                // Don't move anything to scrollback in this case
                0
            };

            // Save lines to scrollback if we're keeping bottom content
            if lines_to_scrollback > 0 && self.scrollback_limit > 0 {
                for i in 0..lines_to_scrollback {
                    if i < working_cells.len() {
                        self.scrollback_buffer.push(working_cells[i].clone());
                    }
                }

                // Trim scrollback buffer if it exceeds the limit
                if self.scrollback_buffer.len() > self.scrollback_limit {
                    let excess = self.scrollback_buffer.len() - self.scrollback_limit;
                    self.scrollback_buffer.drain(0..excess);
                }

                eprintln!("[SCREEN_BUFFER] Moved {} lines to scrollback (cursor in bottom half)", lines_to_scrollback);
            }

            // Copy content to new buffer
            if lines_to_scrollback > 0 {
                // Keep bottom content - copy from (lines_to_scrollback) onwards
                for (new_y, old_y) in (lines_to_scrollback..old_height).enumerate() {
                    if old_y < working_cells.len() && new_y < height {
                        for x in 0..width {
                            if x < working_cells[old_y].len() {
                                new_cells[new_y][x] = working_cells[old_y][x].clone();
                            }
                        }
                    }
                }

                // Adjust cursor position
                if old_cursor_y >= lines_to_scrollback {
                    self.cursor_y = old_cursor_y - lines_to_scrollback;
                } else {
                    self.cursor_y = 0;
                }
            } else {
                // Keep top content - copy what fits
                for y in 0..height.min(old_height) {
                    if y < working_cells.len() {
                        for x in 0..width {
                            if x < working_cells[y].len() {
                                new_cells[y][x] = working_cells[y][x].clone();
                            }
                        }
                    }
                }

                // Keep cursor at same position
                self.cursor_y = old_cursor_y;
            }
        } else {
            // Terminal is same size or growing - simple copy from working_cells
            let copy_height = old_height.min(height);
            for (y, row) in new_cells.iter_mut().enumerate().take(copy_height) {
                if y < working_cells.len() {
                    for x in 0..width {
                        if x < working_cells[y].len() {
                            row[x] = working_cells[y][x].clone();
                        }
                    }
                }
            }

            self.cursor_y = old_cursor_y;
        }

        // Keep cursor in bounds (use updated position from rewrap if it happened)
        self.cursor_x = old_cursor_x.min(width.saturating_sub(1));
        self.cursor_y = self.cursor_y.min(height.saturating_sub(1));

        self.cells = new_cells;
        self.width = width;
        self.height = height;
        self.dirty = true;
    }

    /// Rewrap content to fit a new width, preserving all text
    /// Returns (rewrapped_lines, new_cursor_x, new_cursor_y)
    fn rewrap_content(&self, new_width: usize, _old_height: usize) -> (Vec<Vec<Cell>>, usize, usize) {
        eprintln!("[SCREEN_BUFFER] Rewrapping content to width {}", new_width);

        // Collect all text content from all lines, trimming trailing spaces
        // Also track the cursor position in the flattened text
        let mut all_text: Vec<(char, Color, Color)> = Vec::new();
        let mut cursor_char_index: Option<usize> = None;
        let mut current_char_index = 0;

        for (row_idx, row) in self.cells.iter().enumerate() {
            // Find the last non-space character in this row
            let mut last_content_idx = 0;
            for (i, cell) in row.iter().enumerate() {
                if cell.ch != ' ' || cell.extended.is_some() {
                    last_content_idx = i + 1;
                }
            }

            // Collect content up to last non-space character
            for i in 0..last_content_idx {
                if i < row.len() {
                    let cell = &row[i];

                    // Track cursor position in flattened text
                    if row_idx == self.cursor_y && i == self.cursor_x {
                        cursor_char_index = Some(current_char_index);
                    }

                    all_text.push((cell.ch, cell.fg_color, cell.bg_color));
                    current_char_index += 1;

                    // Handle extended graphemes
                    if let Some(ref ext) = cell.extended {
                        for ch in ext.chars() {
                            all_text.push((ch, cell.fg_color, cell.bg_color));
                            current_char_index += 1;
                        }
                    }
                }
            }

            // If cursor is on this line but past the content (in trailing spaces),
            // mark it at the end of the line's content
            if row_idx == self.cursor_y && self.cursor_x >= last_content_idx && cursor_char_index.is_none() {
                cursor_char_index = Some(current_char_index);
            }

            // Add a newline marker to preserve line breaks
            // (represented as space with special flag - we'll handle this during rewrap)
            if last_content_idx > 0 {
                all_text.push(('\n', Color::default(), Color::default()));
                current_char_index += 1;
            }
        }

        // If cursor wasn't found (e.g., on empty line), set it to end
        let cursor_char_index = cursor_char_index.unwrap_or(current_char_index);

        // Now rewrap the text to fit new_width, tracking cursor position
        let mut new_rows: Vec<Vec<Cell>> = Vec::new();
        let mut current_row = vec![Cell::default(); new_width];
        let mut x = 0;
        let mut char_index = 0;
        let mut new_cursor_x = 0;
        let mut new_cursor_y = 0;
        let mut cursor_found = false;

        for (ch, fg, bg) in all_text {
            // Check if this is where the cursor should be
            if !cursor_found && char_index == cursor_char_index {
                new_cursor_x = x;
                new_cursor_y = new_rows.len();
                cursor_found = true;
            }

            char_index += 1;
            if ch == '\n' {
                // Line break - finish current row and start new one
                if x > 0 || new_rows.is_empty() {
                    new_rows.push(current_row);
                    current_row = vec![Cell::default(); new_width];
                    x = 0;
                }
                continue;
            }

            // Check if we need to wrap to next line
            if x >= new_width {
                new_rows.push(current_row);
                current_row = vec![Cell::default(); new_width];
                x = 0;
            }

            // Add character to current row
            current_row[x] = Cell {
                ch,
                fg_color: fg,
                bg_color: bg,
                extended: None,
                width: 1, // Default to 1 for normal characters
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                blink: false,
                reverse: false,
                invisible: false,
            };
            x += 1;
        }

        // Add the last row if it has content
        if x > 0 || new_rows.is_empty() {
            new_rows.push(current_row);
        }

        // If cursor wasn't placed yet (was at or after end), place it at the end
        if !cursor_found {
            new_cursor_x = x;
            new_cursor_y = new_rows.len().saturating_sub(1);
        }

        eprintln!("[SCREEN_BUFFER] Rewrapped {} old lines into {} new lines", self.cells.len(), new_rows.len());
        eprintln!(
            "[SCREEN_BUFFER] Cursor position: ({}, {}) -> ({}, {})",
            self.cursor_x, self.cursor_y, new_cursor_x, new_cursor_y
        );

        (new_rows, new_cursor_x, new_cursor_y)
    }

    /// Put a grapheme cluster (potentially multi-character emoji with modifiers)
    pub fn put_grapheme(&mut self, grapheme: &str) {
        // Handle pending wrap from previous character
        if self.pending_wrap && self.auto_wrap_mode {
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
            // If insert mode is enabled, shift characters to the right before writing
            if self.insert_mode {
                // Shift all characters from cursor position to the right by char_width
                // First, determine the width of the character we're about to insert
                let is_emoji = is_emoji_grapheme(grapheme);
                let first_char = grapheme.chars().next().unwrap_or(' ');
                let unicode_width = first_char.width().unwrap_or(1);
                let char_width = if is_emoji { 2 } else { unicode_width };

                // Shift characters to the right
                if self.cursor_x + char_width < self.width {
                    // Shift characters within the line
                    let y = self.cursor_y;
                    for x in (self.cursor_x..self.width - char_width).rev() {
                        self.cells[y][x + char_width] = self.cells[y][x].clone();
                    }
                    // Clear the vacated cells
                    for i in 0..char_width {
                        if self.cursor_x + i < self.width {
                            self.cells[y][self.cursor_x + i] = Cell::default();
                        }
                    }
                }
            }
            // Determine character width
            // First check if this grapheme contains an emoji (including combined emojis)
            let is_emoji = is_emoji_grapheme(grapheme);

            // For non-emoji characters, use Unicode East Asian Width property
            let first_char = grapheme.chars().next().unwrap_or(' ');
            let unicode_width = first_char.width().unwrap_or(1);

            // Use the larger of emoji detection or Unicode width
            let char_width = if is_emoji { 2 } else { unicode_width };

            // Write the grapheme cluster
            // For simple single-char graphemes, use just the char field
            // For complex graphemes (emojis with modifiers), store in extended field
            let extended_data = if grapheme.chars().count() > 1 { Some(grapheme.into()) } else { None };

            // Translate character through active charset if it's a simple single character
            let translated_char = if grapheme.chars().count() == 1 {
                self.translate_charset(first_char)
            } else {
                first_char
            };

            self.cells[self.cursor_y][self.cursor_x] = Cell {
                ch: translated_char,
                extended: extended_data,
                fg_color: self.fg_color,
                bg_color: self.bg_color,
                width: char_width as u8,
                bold: self.bold,
                italic: self.italic,
                underline: self.underline,
                strikethrough: self.strikethrough,
                blink: self.blink,
                reverse: self.reverse,
                invisible: self.invisible,
            };

            // For double-width characters, mark the next cell as a continuation
            if char_width == 2 && self.cursor_x + 1 < self.width {
                self.cells[self.cursor_y][self.cursor_x + 1] = Cell {
                    ch: '\0', // Null char indicates continuation of previous cell
                    extended: None,
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                    width: 0, // Width 0 means this is a continuation cell
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    strikethrough: self.strikethrough,
                    blink: self.blink,
                    reverse: self.reverse,
                    invisible: self.invisible,
                };
            }

            // Track the last character for REP command
            self.last_char = Some(first_char);

            self.cursor_x += char_width;
            self.dirty = true;

            // Set pending wrap if we're past the last column
            if self.cursor_x >= self.width {
                self.cursor_x = self.width - 1;
                // Only set pending wrap if auto-wrap mode is enabled
                if self.auto_wrap_mode {
                    self.pending_wrap = true;
                }
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

        // Use custom tab stops if defined, otherwise use default (every 8 columns)
        if let Some(ref stops) = self.tab_stops {
            // Find next tab stop after current position
            let mut next_tab = self.width - 1; // Default to end of line
            for &stop in stops.iter() {
                if stop > self.cursor_x && stop < next_tab {
                    next_tab = stop;
                }
            }
            self.cursor_x = next_tab;
        } else {
            // Default tab stops every 8 columns
            let next_tab = ((self.cursor_x / 8) + 1) * 8;
            self.cursor_x = next_tab.min(self.width - 1);
        }
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

        // Respect scroll region boundaries if one is set
        if let Some((top, _bottom)) = self.scroll_region {
            // Cursor should not move above the top of the scroll region
            self.cursor_y = self.cursor_y.saturating_sub(n).max(top);
        } else {
            // No scroll region, cursor can move to row 0
            self.cursor_y = self.cursor_y.saturating_sub(n);
        }

        self.dirty = true;
    }

    pub fn move_cursor_down(&mut self, n: usize) {
        self.pending_wrap = false;

        // Respect scroll region boundaries if one is set
        if let Some((_top, bottom)) = self.scroll_region {
            // Cursor should not move below the bottom of the scroll region
            self.cursor_y = (self.cursor_y + n).min(bottom);
        } else {
            // No scroll region, cursor is limited by screen height
            self.cursor_y = (self.cursor_y + n).min(self.height - 1);
        }

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

    /// CHT - Cursor Horizontal Forward Tabulation
    /// Move cursor forward n tab stops (default tab stops at multiples of 8)
    pub fn forward_tab(&mut self, n: usize) {
        self.pending_wrap = false;
        for _ in 0..n {
            // Move to next tab stop (multiple of 8)
            let next_tab = ((self.cursor_x / 8) + 1) * 8;
            self.cursor_x = next_tab.min(self.width - 1);
            // If we hit the right edge, stop
            if self.cursor_x == self.width - 1 {
                break;
            }
        }
        self.dirty = true;
    }

    /// CBT - Cursor Backward Tabulation
    /// Move cursor backward n tab stops (default tab stops at multiples of 8)
    pub fn back_tab(&mut self, n: usize) {
        self.pending_wrap = false;
        for _ in 0..n {
            if self.cursor_x == 0 {
                break;
            }
            // Move to previous tab stop (multiple of 8)
            // If already at a tab stop, move to the previous one
            let prev_tab = if self.cursor_x % 8 == 0 {
                self.cursor_x.saturating_sub(8)
            } else {
                (self.cursor_x / 8) * 8
            };
            self.cursor_x = prev_tab;
        }
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

        // Save current screen content to scrollback buffer before clearing
        // Only save lines up to the cursor position (or last non-empty line)
        if self.scrollback_limit > 0 {
            // Find the last line with actual content
            let mut last_content_line = self.cursor_y;
            for y in (0..self.cells.len()).rev() {
                let has_content = self.cells[y].iter().any(|cell| cell.ch != ' ' || cell.extended.is_some());
                if has_content {
                    last_content_line = y;
                    break;
                }
            }

            // Push only lines up to and including the last line with content
            for i in 0..=last_content_line.min(self.cells.len().saturating_sub(1)) {
                self.scrollback_buffer.push(self.cells[i].clone());
            }

            // Trim scrollback buffer if it exceeds the limit
            if self.scrollback_buffer.len() > self.scrollback_limit {
                let excess = self.scrollback_buffer.len() - self.scrollback_limit;
                self.scrollback_buffer.drain(0..excess);
            }
        }

        // Clear all cells
        for row in &mut self.cells {
            for cell in row {
                cell.ch = ' ';
                cell.extended = None;
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }

        // Move cursor to home position
        self.cursor_x = 0;
        self.cursor_y = 0;

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
                cell.extended = None;
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }

            // Clear all lines below
            for y in (self.cursor_y + 1)..self.height {
                for x in 0..self.width {
                    let cell = &mut self.cells[y][x];
                    cell.ch = ' ';
                    cell.extended = None;
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
                cell.extended = None;
                cell.fg_color = self.fg_color;
                cell.bg_color = self.bg_color;
            }
        }

        // Clear from start of current line to cursor
        if self.cursor_y < self.height {
            for x in 0..=self.cursor_x.min(self.width - 1) {
                let cell = &mut self.cells[self.cursor_y][x];
                cell.ch = ' ';
                cell.extended = None;
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
                cell.extended = None;
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
                cell.extended = None;
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
                cell.extended = None;
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
                cell.extended = None;
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
                    extended: None,
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                    width: 1,
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    strikethrough: self.strikethrough,
                    blink: self.blink,
                    reverse: self.reverse,
                    invisible: self.invisible,
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
                row[new_pos] = row[x].clone();
            }
        }

        // Fill inserted positions with blank characters
        let end = (cursor_x + n).min(self.width);
        for cell in row.iter_mut().take(end).skip(cursor_x) {
            *cell = Cell {
                ch: ' ',
                extended: None,
                fg_color: self.fg_color,
                bg_color: self.bg_color,
                width: 1,
                bold: self.bold,
                italic: self.italic,
                underline: self.underline,
                strikethrough: self.strikethrough,
                blink: self.blink,
                reverse: self.reverse,
                invisible: self.invisible,
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
                row[x] = row[source_pos].clone();
            } else {
                // Fill with blank at the end
                row[x] = Cell {
                    ch: ' ',
                    extended: None,
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                    width: 1,
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    strikethrough: self.strikethrough,
                    blink: self.blink,
                    reverse: self.reverse,
                    invisible: self.invisible,
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
                cell.extended = None;
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
                cell.extended = None;
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
                    extended: None,
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                    width: 1,
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    strikethrough: self.strikethrough,
                    blink: self.blink,
                    reverse: self.reverse,
                    invisible: self.invisible,
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
                    extended: None,
                    fg_color: self.fg_color,
                    bg_color: self.bg_color,
                    width: 1,
                    bold: self.bold,
                    italic: self.italic,
                    underline: self.underline,
                    strikethrough: self.strikethrough,
                    blink: self.blink,
                    reverse: self.reverse,
                    invisible: self.invisible,
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

    pub fn set_auto_wrap_mode(&mut self, enabled: bool) {
        self.auto_wrap_mode = enabled;
    }

    pub fn set_insert_mode(&mut self, enabled: bool) {
        self.insert_mode = enabled;
    }

    pub fn set_automatic_newline(&mut self, enabled: bool) {
        self.automatic_newline = enabled;
    }

    pub fn get_automatic_newline(&self) -> bool {
        self.automatic_newline
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

    pub fn get_scrollback_buffer(&self) -> &Vec<Vec<Cell>> {
        &self.scrollback_buffer
    }

    pub fn get_scroll_region(&self) -> Option<(usize, usize)> {
        self.scroll_region
    }

    /// Restore output lines to scrollback buffer (for loading from saved state)
    pub fn restore_to_scrollback(&mut self, lines: Vec<String>) {
        for line in lines {
            let mut row = Vec::with_capacity(self.width);

            // Convert string to cells
            for ch in line.chars() {
                let cell = Cell {
                    ch,
                    extended: None,
                    fg_color: DEFAULT_FG_COLOR,
                    bg_color: DEFAULT_BG_COLOR,
                    width: 1,
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    blink: false,
                    reverse: false,
                    invisible: false,
                };
                row.push(cell);
            }

            // Pad with empty cells to match width
            while row.len() < self.width {
                row.push(Cell::default());
            }

            // Truncate if too long
            row.truncate(self.width);

            // Add to scrollback buffer
            self.scrollback_buffer.push(row);
        }

        // Enforce scrollback limit
        if self.scrollback_limit > 0 {
            while self.scrollback_buffer.len() > self.scrollback_limit {
                self.scrollback_buffer.remove(0);
            }
        }

        self.dirty = true;
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
    pub fn reset_view_offset(&mut self) {
        self.scroll_offset = 0;
        self.dirty = true;
    }

    /// Repeat the last printed character n times (REP - CSI Ps b)
    pub fn repeat_last_char(&mut self, count: usize) {
        if let Some(ch) = self.last_char {
            let grapheme = ch.to_string();
            for _ in 0..count {
                self.put_grapheme(&grapheme);
            }
        }
    }

    /// Clear tab stops (TBC - CSI Ps g)
    pub fn clear_tab_stop(&mut self, mode: usize) {
        match mode {
            0 => {
                // Clear tab stop at current column
                if self.tab_stops.is_none() {
                    // Initialize with default tab stops (every 8 columns)
                    let mut stops = std::collections::HashSet::new();
                    for i in (8..self.width).step_by(8) {
                        stops.insert(i);
                    }
                    self.tab_stops = Some(stops);
                }
                if let Some(ref mut stops) = self.tab_stops {
                    stops.remove(&self.cursor_x);
                }
            }
            3 => {
                // Clear all tab stops
                self.tab_stops = Some(std::collections::HashSet::new());
            }
            _ => {
                // Unknown mode, ignore
            }
        }
    }

    /// Set a tab stop at the current column (HTS - ESC H)
    pub fn set_tab_stop(&mut self) {
        if self.tab_stops.is_none() {
            // Initialize with default tab stops (every 8 columns)
            let mut stops = std::collections::HashSet::new();
            for i in (8..self.width).step_by(8) {
                stops.insert(i);
            }
            self.tab_stops = Some(stops);
        }
        if let Some(ref mut stops) = self.tab_stops {
            stops.insert(self.cursor_x);
        }
    }

    /// Perform a soft terminal reset (DECSTR)
    pub fn soft_reset(&mut self) {
        // Reset text attributes
        self.bold = false;
        self.italic = false;
        self.underline = false;
        self.strikethrough = false;
        self.blink = false;
        self.reverse = false;
        self.invisible = false;

        // Reset colors
        self.fg_color = DEFAULT_FG_COLOR;
        self.bg_color = DEFAULT_BG_COLOR;

        // Reset modes
        self.origin_mode = false;
        self.auto_wrap_mode = true;
        self.reverse_video_mode = false;
        self.insert_mode = false;
        self.automatic_newline = false;

        // Reset scroll region
        self.scroll_region = None;

        // Reset character sets
        self.g0_charset = CharSet::Ascii;
        self.g1_charset = CharSet::Ascii;
        self.g2_charset = CharSet::Ascii;
        self.g3_charset = CharSet::Ascii;
        self.active_charset = 0;
        self.single_shift = None;

        // Move cursor to home
        self.cursor_x = 0;
        self.cursor_y = 0;

        // Clear pending wrap
        self.pending_wrap = false;

        self.dirty = true;
    }

    /// Designate a character set to one of G0-G3
    pub fn designate_charset(&mut self, g_set: usize, charset: CharSet) {
        match g_set {
            0 => self.g0_charset = charset,
            1 => self.g1_charset = charset,
            2 => self.g2_charset = charset,
            3 => self.g3_charset = charset,
            _ => {}
        }
    }

    /// Shift Out (SO) - Switch to G1 character set
    pub fn shift_out(&mut self) {
        self.active_charset = 1;
    }

    /// Shift In (SI) - Switch to G0 character set
    pub fn shift_in(&mut self) {
        self.active_charset = 0;
    }

    /// Locking Shift - invoke Gn as GL
    pub fn locking_shift(&mut self, g_set: usize) {
        if g_set <= 3 {
            self.active_charset = g_set;
        }
    }

    /// Single Shift - invoke Gn for next character only
    pub fn single_shift(&mut self, g_set: usize) {
        if g_set == 2 || g_set == 3 {
            self.single_shift = Some(g_set);
        }
    }

    /// Translate a character through the active character set
    /// This handles DEC Special Graphics charset for box drawing
    pub fn translate_charset(&mut self, ch: char) -> char {
        // Check if we have a single shift active
        let charset_index = if let Some(ss) = self.single_shift.take() { ss } else { self.active_charset };

        let charset = match charset_index {
            0 => self.g0_charset,
            1 => self.g1_charset,
            2 => self.g2_charset,
            3 => self.g3_charset,
            _ => CharSet::Ascii,
        };

        match charset {
            CharSet::Ascii => ch,
            CharSet::DecSpecialGraphics => translate_dec_special_graphics(ch),
        }
    }

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
            if scrollback_y < self.scrollback_buffer.len() && x < self.scrollback_buffer[scrollback_y].len() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_minimum_size_enforcement() {
        // Test that resize enforces minimum size of 2x2
        let mut buffer = ScreenBuffer::new_with_scrollback(10, 10, 1000, CursorStyle::default());

        // Try to resize to 0x0 - should be clamped to 2x2
        buffer.resize(0, 0);
        assert_eq!(buffer.width(), 2, "Width should be clamped to minimum of 2");
        assert_eq!(buffer.height(), 2, "Height should be clamped to minimum of 2");

        // Try to resize to 1x1 - should be clamped to 2x2
        buffer.resize(1, 1);
        assert_eq!(buffer.width(), 2, "Width should be clamped to minimum of 2");
        assert_eq!(buffer.height(), 2, "Height should be clamped to minimum of 2");
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_forward_tab() {
            let mut sb = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

            // Test moving forward 1 tab stop from column 0
            sb.cursor_x = 0;
            sb.forward_tab(1);
            assert_eq!(sb.cursor_x, 8, "Should move to column 8");

            // Test moving forward 1 tab stop from column 5
            sb.cursor_x = 5;
            sb.forward_tab(1);
            assert_eq!(sb.cursor_x, 8, "Should move to column 8 from column 5");

            // Test moving forward 2 tab stops
            sb.cursor_x = 0;
            sb.forward_tab(2);
            assert_eq!(sb.cursor_x, 16, "Should move to column 16");

            // Test moving forward from a tab stop
            sb.cursor_x = 16;
            sb.forward_tab(1);
            assert_eq!(sb.cursor_x, 24, "Should move to column 24 from column 16");

            // Test that cursor stops at right edge
            sb.cursor_x = 72;
            sb.forward_tab(10);
            assert_eq!(sb.cursor_x, 79, "Should stop at right edge (column 79)");
        }

        #[test]
        fn test_back_tab() {
            let mut sb = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

            // Test moving back 1 tab stop from column 16
            sb.cursor_x = 16;
            sb.back_tab(1);
            assert_eq!(sb.cursor_x, 8, "Should move to column 8");

            // Test moving back from middle of tab stop
            sb.cursor_x = 13;
            sb.back_tab(1);
            assert_eq!(sb.cursor_x, 8, "Should move to column 8 from column 13");

            // Test moving back 2 tab stops
            sb.cursor_x = 24;
            sb.back_tab(2);
            assert_eq!(sb.cursor_x, 8, "Should move to column 8");

            // Test moving back from tab stop boundary
            sb.cursor_x = 8;
            sb.back_tab(1);
            assert_eq!(sb.cursor_x, 0, "Should move to column 0 from column 8");

            // Test that cursor stops at left edge
            sb.cursor_x = 5;
            sb.back_tab(10);
            assert_eq!(sb.cursor_x, 0, "Should stop at left edge (column 0)");
        }

        #[test]
        fn test_forward_back_tab_combination() {
            let mut sb = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

            // Start at 0, go forward 3 tabs, then back 1 tab
            sb.cursor_x = 0;
            sb.forward_tab(3);
            assert_eq!(sb.cursor_x, 24);
            sb.back_tab(1);
            assert_eq!(sb.cursor_x, 16);

            // Go forward 1, back 2
            sb.forward_tab(1);
            assert_eq!(sb.cursor_x, 24);
            sb.back_tab(2);
            assert_eq!(sb.cursor_x, 8);
        }
    }

    #[test]
    fn test_resize_preserves_content() {
        // Test that resize preserves content when growing/shrinking
        let mut buffer = ScreenBuffer::new_with_scrollback(10, 10, 1000, CursorStyle::default());

        // Put some content in the buffer
        buffer.move_cursor_to(0, 0);
        buffer.put_grapheme("A");
        buffer.move_cursor_to(4, 4);
        buffer.put_grapheme("B");

        // Grow the buffer
        buffer.resize(10, 10);
        assert_eq!(buffer.width(), 10);
        assert_eq!(buffer.height(), 10);

        // Check that original content is preserved
        if let Some(cell) = buffer.get_cell(0, 0) {
            assert_eq!(cell.ch, 'A');
        } else {
            panic!("Cell (0,0) should exist");
        }

        if let Some(cell) = buffer.get_cell(4, 4) {
            assert_eq!(cell.ch, 'B');
        } else {
            panic!("Cell (4,4) should exist");
        }

        // Shrink the buffer
        buffer.resize(3, 3);
        assert_eq!(buffer.width(), 3);
        assert_eq!(buffer.height(), 3);

        // Original cell should still be there
        if let Some(cell) = buffer.get_cell(0, 0) {
            assert_eq!(cell.ch, 'A');
        } else {
            panic!("Cell (0,0) should exist after shrinking");
        }
    }

    #[test]
    fn test_resize_height_decrease_preserves_recent_content() {
        // Test that when terminal gets shorter, recent content stays visible
        // and older content moves to scrollback
        let mut buffer = ScreenBuffer::new_with_scrollback(80, 20, 1000, CursorStyle::default());

        // Fill the buffer with identifiable content (20 lines)
        for y in 0..20 {
            buffer.move_cursor_to(0, y);
            // Put line number as content
            let line_str = format!("LINE_{:02}", y);
            for ch in line_str.chars() {
                buffer.put_grapheme(&ch.to_string());
            }
        }

        // Verify all lines are present before resize
        for y in 0..20 {
            if let Some(cell) = buffer.get_cell(0, y) {
                assert_eq!(cell.ch, 'L', "Line {} should start with 'L'", y);
            }
        }

        // Resize to half the height (20 -> 10)
        buffer.resize(80, 10);

        // Check that scrollback has the top 10 lines
        let scrollback = buffer.get_scrollback_buffer();
        assert_eq!(scrollback.len(), 10, "Should have 10 lines in scrollback");

        // Verify scrollback contains the old top lines (LINE_00 through LINE_09)
        for (i, row) in scrollback.iter().enumerate().take(10) {
            if row.len() > 0 {
                assert_eq!(row[0].ch, 'L', "Scrollback line {} should start with 'L'", i);
            }
        }

        // Check that visible buffer has the bottom 10 lines (LINE_10 through LINE_19)
        assert_eq!(buffer.height(), 10, "New height should be 10");
        for y in 0..10 {
            if let Some(cell) = buffer.get_cell(0, y) {
                assert_eq!(cell.ch, 'L', "Visible line {} should start with 'L'", y);
                // Line 0 in new buffer should be LINE_10 from old buffer
                // The key point is that bottom lines are preserved
            }
        }

        // Verify cursor was adjusted correctly
        assert!(buffer.cursor_y < 10, "Cursor Y should be within new height");
    }

    #[test]
    fn test_resize_width_decrease_rewraps_content() {
        // Test that when width decreases, content is rewrapped to preserve text
        let mut buffer = ScreenBuffer::new_with_scrollback(80, 24, 100, CursorStyle::default());

        // Fill first line with a long string of identifiable characters
        buffer.move_cursor_to(0, 0);
        let long_line = "AAAA BBBB CCCC DDDD EEEE FFFF GGGG HHHH IIII JJJJ KKKK LLLL MMMM NNNN OOOO PPPP";
        for ch in long_line.chars() {
            buffer.put_grapheme(&ch.to_string());
        }

        // Add a second line
        buffer.move_cursor_to(0, 1);
        let line2 = "LINE2_START 1111 2222 3333 4444 5555 6666 7777 8888 9999 AAAA BBBB CCCC DDDD";
        for ch in line2.chars() {
            buffer.put_grapheme(&ch.to_string());
        }

        // Verify content is there
        let cell_a = buffer.get_cell(0, 0).unwrap();
        assert_eq!(cell_a.ch, 'A');
        let cell_l = buffer.get_cell(0, 1).unwrap();
        assert_eq!(cell_l.ch, 'L');

        // Resize to half width (80 -> 40) - should trigger rewrapping
        buffer.resize(40, 10);

        // Check that buffer was resized
        assert_eq!(buffer.width(), 40);
        assert_eq!(buffer.height(), 10);

        // After rewrap, we should have more lines with the content spread across them
        // The long line should now be wrapped into multiple lines
        // Verify that key characters are still present somewhere in the buffer
        let mut found_a = false;
        let mut found_o = false;
        let mut found_line2_start = false;

        for y in 0..buffer.height() {
            for x in 0..buffer.width() {
                if let Some(cell) = buffer.get_cell(x, y) {
                    if cell.ch == 'A' && x == 0 {
                        found_a = true;
                    }
                    if cell.ch == 'O' {
                        found_o = true;
                    }
                    if cell.ch == 'L' && x == 0 {
                        found_line2_start = true;
                    }
                }
            }
        }

        assert!(found_a, "First character 'A' should still be present");
        assert!(found_o, "Character 'O' from later in line should be present after rewrap");
        assert!(found_line2_start, "Second line should still be present after rewrap");
    }

    #[test]
    fn test_resize_width_decrease_with_scrollback_and_cursor() {
        // Test that when rewrapping creates more lines than fit, excess goes to scrollback
        // and cursor position is correctly tracked
        let mut buffer = ScreenBuffer::new_with_scrollback(80, 24, 20, CursorStyle::default());

        // Fill buffer with multiple long lines (more than will fit after rewrap)
        for i in 0..5 {
            buffer.move_cursor_to(0, i);
            // Each line is ~80 characters, will become ~4 lines at width 20
            let line = format!("LINE{}_AAAA_BBBB_CCCC_DDDD_EEEE_FFFF_GGGG_HHHH_IIII_JJJJ_KKKK_LLLL_MMMM_NNNN", i);
            for ch in line.chars() {
                buffer.put_grapheme(&ch.to_string());
            }
        }

        // Put cursor at end of last line (bottom of buffer)
        let initial_cursor_y = buffer.cursor_y;
        assert_eq!(initial_cursor_y, 4, "Cursor should be at last line before resize");

        // Resize to much smaller width (80 -> 20)
        // 5 lines of ~80 chars each = ~20 lines at width 20
        // But we only have height 5, so 15 lines should go to scrollback
        buffer.resize(20, 5);

        // Verify dimensions
        assert_eq!(buffer.width(), 20);
        assert_eq!(buffer.height(), 5);

        // Verify that scrollback has content
        let scrollback = buffer.get_scrollback_buffer();
        assert!(scrollback.len() > 0, "Scrollback should contain excess lines from rewrapping");
        eprintln!("Scrollback has {} lines", scrollback.len());

        // Verify cursor is still in bounds
        assert!(
            buffer.cursor_x < buffer.width(),
            "Cursor X should be in bounds: {} < {}",
            buffer.cursor_x,
            buffer.width()
        );
        assert!(
            buffer.cursor_y < buffer.height(),
            "Cursor Y should be in bounds: {} < {}",
            buffer.cursor_y,
            buffer.height()
        );

        // Verify that recent content (where cursor was) is still visible
        // Look for LINE4 in visible buffer
        let mut found_line4 = false;
        for y in 0..buffer.height() {
            if let Some(cell) = buffer.get_cell(0, y) {
                if cell.ch == 'L' {
                    // Check if this starts with "LINE4"
                    let mut matches = true;
                    for (i, expected_ch) in "LINE4".chars().enumerate() {
                        if let Some(c) = buffer.get_cell(i, y) {
                            if c.ch != expected_ch {
                                matches = false;
                                break;
                            }
                        } else {
                            matches = false;
                            break;
                        }
                    }
                    if matches {
                        found_line4 = true;
                        break;
                    }
                }
            }
        }
        assert!(found_line4, "Most recent content (LINE4) should still be visible after rewrap");

        // Verify that older content is in scrollback
        let mut found_line0_in_scrollback = false;
        for row in scrollback.iter() {
            if row.len() > 0 && row[0].ch == 'L' {
                // Check if this starts with "LINE0"
                let mut matches = true;
                for (i, expected_ch) in "LINE0".chars().enumerate() {
                    if i < row.len() && row[i].ch == expected_ch {
                        continue;
                    } else {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    found_line0_in_scrollback = true;
                    break;
                }
            }
        }
        assert!(found_line0_in_scrollback, "Older content (LINE0) should be in scrollback");
    }

    #[test]
    fn test_resize_with_very_small_font() {
        // Simulate what happens when font is too large for window
        // This would result in cols=0 or rows=0 without minimum enforcement
        let mut buffer = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

        // Fill with some content
        for y in 0..24 {
            for x in 0..80 {
                buffer.move_cursor_to(x, y);
                buffer.put_grapheme("X");
            }
        }

        // Try to resize to very small - should be clamped to 2x2
        buffer.resize(0, 0);
        assert_eq!(buffer.width(), 2);
        assert_eq!(buffer.height(), 2);

        // Should be able to access all cells without panic
        for y in 0..2 {
            for x in 0..2 {
                assert!(buffer.get_cell(x, y).is_some());
            }
        }
    }

    #[test]
    fn test_cursor_stays_in_bounds_after_resize() {
        let mut buffer = ScreenBuffer::new_with_scrollback(80, 24, 100, CursorStyle::default());

        // Move cursor to bottom right
        buffer.move_cursor_to(79, 23);
        assert_eq!(buffer.cursor_x, 79);
        assert_eq!(buffer.cursor_y, 23);

        // Shrink buffer - cursor should be clamped
        buffer.resize(10, 10);
        assert_eq!(buffer.cursor_x, 9, "Cursor X should be clamped to width-1");
        assert_eq!(buffer.cursor_y, 9, "Cursor Y should be clamped to height-1");

        // Resize to minimum - cursor should still be valid
        buffer.resize(2, 2);
        assert_eq!(buffer.cursor_x, 1, "Cursor X should be clamped to width-1");
        assert_eq!(buffer.cursor_y, 1, "Cursor Y should be clamped to height-1");
    }
}
