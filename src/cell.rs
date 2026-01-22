use crate::ansi::{DEFAULT_BG_COLOR, DEFAULT_FG_COLOR};
use sdl3::pixels::Color;

/// A terminal cell containing a character and its formatting attributes
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

impl Cell {
    /// Creates a default cell with a specific character
    pub fn with_char(ch: char) -> Self {
        Cell { ch, ..Default::default() }
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
