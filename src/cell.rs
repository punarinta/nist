
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
        // Dingbats (❌ U+274C, ✔ U+2714, ✖ U+2716, ❗ U+2757, etc.)
        0x2700..=0x27BF |
        // Enclosed Alphanumeric Supplement (includes circled numbers and regional indicators for flags)
        0x1F100..=0x1F1FF |
        // Enclosed Ideographic Supplement
        0x1F200..=0x1F2FF |
        // Variation Selectors (emoji presentation)
        0xFE00..=0xFE0F |
        // Mahjong Tiles, Domino Tiles
        0x1F000..=0x1F02F |
        // Playing Cards
        0x1F0A0..=0x1F0FF |
        // Emoji characters in Miscellaneous Technical range (⌚⌛⏩⏪⏫⏬⏭⏮⏯⏰⏱⏲⏳⏸⏹⏺)
        0x231A..=0x231B |  // Watch, Hourglass
        0x23E9..=0x23F3 |  // Fast-forward/rewind/up/down, hourglass with flowing sand (⏳)
        0x23F8..=0x23FA    // Pause, Stop, Record
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
        0xD7B0..=0xD7FF |
        // CJK Symbols and Punctuation (。、「」『』【】〔〕…—～ etc.)
        0x3000..=0x303F |
        // CJK Compatibility Forms
        0xFE30..=0xFE4F |
        // Halfwidth and Fullwidth Forms (，！？：；""'' etc.)
        0xFF00..=0xFFEF
    )
}

/// Check if a string contains CJK characters
#[inline]
pub fn is_cjk_grapheme(s: &str) -> bool {
    // Check if any character in the grapheme cluster is CJK
    s.chars().any(is_cjk_char)
}
