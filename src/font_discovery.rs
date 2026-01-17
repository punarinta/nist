//! Cross-platform font discovery for finding emoji-supporting fonts
//!
//! This module searches the system for both monospace fonts (for terminal content)
//! and proportional fonts (for UI elements like tabs, menus, window controls).
//! It prioritizes fonts known to render emojis well across Windows, macOS, and Linux.
//!
//! The module searches platform-specific font directories:
//! - Windows: C:\Windows\Fonts and user font directories
//! - macOS: /System/Library/Fonts, /Library/Fonts, ~/Library/Fonts
//! - Linux: /usr/share/fonts, /usr/local/share/fonts, ~/.local/share/fonts
//!
//! If no suitable font is found, the caller should handle the None return value
//! and provide appropriate error messages with installation instructions.

use std::fs;
use std::path::{Path, PathBuf};

/// List of preferred proportional UI fonts with excellent emoji/Unicode support, in order of preference
const PREFERRED_UI_FONTS: &[&str] = &[
    // Windows system fonts
    "segoeui.ttf",
    "segoeuib.ttf",
    "seguiemj.ttf", // Segoe UI Emoji
    "arial.ttf",
    "arialbd.ttf",
    "verdana.ttf",
    // macOS system fonts
    "SFNS.ttf",
    "SFCompact.ttf",
    "Helvetica.ttf",
    "Helvetica Neue.ttf",
    // Noto fonts - excellent emoji and Unicode support
    "NotoSans-Regular.ttf",
    "NotoSans[wght].ttf",
    "NotoColorEmoji.ttf",
    // Ubuntu fonts - good emoji support
    "Ubuntu-Regular.ttf",
    "Ubuntu-R.ttf",
    "Ubuntu[wght].ttf",
    // DejaVu Sans - decent Unicode support
    "DejaVuSans.ttf",
    "DejaVuSans-Regular.ttf",
    // Liberation Sans
    "LiberationSans-Regular.ttf",
    // Roboto - modern, good Unicode
    "Roboto-Regular.ttf",
    // FreeSans - fallback
    "FreeSans.ttf",
];

/// List of preferred monospace fonts with emoji support, in order of preference
const PREFERRED_MONOSPACE_FONTS: &[&str] = &[
    // Windows system fonts
    "CascadiaCode.ttf",
    "CascadiaMono.ttf",
    "consola.ttf",
    "consolab.ttf",
    "consolai.ttf",
    "cour.ttf",
    "courbd.ttf",
    // macOS system fonts
    "SFNSMono.ttf",
    "Menlo.ttf",
    "Monaco.ttf",
    "Courier New.ttf",
    // Hack - clean and readable
    "Hack-Regular.ttf",
    // Noto fonts - excellent emoji support
    "NotoSansMono-Regular.ttf",
    "NotoSansMono.ttf",
    "NotoMono-Regular.ttf",
    // JetBrains Mono - modern, good Unicode support
    "JetBrainsMono-Regular.ttf",
    "JetBrainsMonoNL-Regular.ttf",
    // Fira Code - popular with developers
    "FiraCode-Regular.ttf",
    "FiraMono-Regular.ttf",
    // IBM Plex Mono - good Unicode coverage
    "IBMPlexMono-Regular.ttf",
    // Source Code Pro - Adobe's monospace font
    "SourceCodePro-Regular.ttf",
    // Inconsolata - classic programmer font
    "Inconsolata-Regular.ttf",
    // Ubuntu Mono - good Unicode support
    "UbuntuMono-Regular.ttf",
    // Liberation Mono - Red Hat
    "LiberationMono-Regular.ttf",
    // DejaVu Sans Mono - fallback default (limited emoji support)
    "DejaVuSansMono.ttf",
    "DejaVuSansMono-Bold.ttf",
];

/// List of preferred emoji fonts with color emoji support, in order of preference
const PREFERRED_EMOJI_FONTS: &[&str] = &[
    // Noto Color Emoji - excellent cross-platform emoji support
    "NotoColorEmoji.ttf",
    // Apple Color Emoji - macOS
    "AppleColorEmoji.ttc",
    "Apple Color Emoji.ttc",
    // Segoe UI Emoji - Windows
    "seguiemj.ttf",
    "Segoe UI Emoji.ttf",
    // EmojiOne - open source alternative
    "EmojiOneColor.otf",
    "emojione-color.otf",
    // Twemoji - Twitter's emoji font
    "TwitterColorEmoji.ttf",
    "Twemoji.ttf",
];

/// List of preferred CJK (Chinese, Japanese, Korean) fonts with full Unicode coverage
const PREFERRED_CJK_FONTS: &[&str] = &[
    // Noto CJK fonts - excellent Unicode coverage
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJKsc-Regular.otf",
    "NotoSansCJKtc-Regular.otf",
    "NotoSansCJKjp-Regular.otf",
    "NotoSansCJKkr-Regular.otf",
    "NotoSerifCJK-Regular.ttc",
    "NotoSerifCJKsc-Regular.otf",
    "NotoSerifCJKtc-Regular.otf",
    "NotoSerifCJKjp-Regular.otf",
    "NotoSerifCJKkr-Regular.otf",
    // Source Han Sans/Serif - Adobe's CJK fonts
    "SourceHanSansCN-Regular.otf",
    "SourceHanSansSC-Regular.otf",
    "SourceHanSansTC-Regular.otf",
    "SourceHanSansJP-Regular.otf",
    "SourceHanSansKR-Regular.otf",
    "SourceHanSerifCN-Regular.otf",
    "SourceHanSerifSC-Regular.otf",
    "SourceHanSerifTC-Regular.otf",
    "SourceHanSerifJP-Regular.otf",
    "SourceHanSerifKR-Regular.otf",
    // Microsoft YaHei - Windows Chinese font
    "msyh.ttc",
    "msyh.ttf",
    "msyhbd.ttf",
    // SimSun, SimHei - older Windows Chinese fonts
    "simsun.ttc",
    "simhei.ttf",
    // Hiragino - macOS Japanese font
    "HiraginoSans-W3.otf",
    "HiraginoSansGB-W3.otf",
    "HiraginoSansCNS-W3.otf",
    // PingFang - modern macOS Chinese font
    "PingFang.ttc",
    "PingFangSC-Regular.otf",
    "PingFangTC-Regular.otf",
    // WenQuanYi - popular Linux CJK fonts
    "wqy-microhei.ttc",
    "wqy-zenhei.ttc",
    // Droid Sans Fallback - Android fallback with CJK
    "DroidSansFallback.ttf",
    "DroidSansFallbackFull.ttf",
    // AR PL UMing/UKai - open source CJK fonts
    "uming.ttc",
    "ukai.ttc",
];

/// Common font directories on Windows, Linux, and macOS systems
const FONT_DIRECTORIES: &[&str] = &[
    // Windows paths
    "C:\\Windows\\Fonts",
    "%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts",
    "%USERPROFILE%\\AppData\\Local\\Microsoft\\Windows\\Fonts",
    // Linux paths
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "~/.local/share/fonts",
    "~/.fonts",
    // macOS paths
    "/System/Library/Fonts",
    "/Library/Fonts",
    "~/Library/Fonts",
];

/// Discovers the best available UI font with full emoji/Unicode support
///
/// Searches through system font directories for proportional fonts known to have
/// excellent emoji and Unicode rendering. This is ideal for UI elements like tabs,
/// menus, and window controls where emojis need to display properly.
///
/// # Returns
///
/// The full path to the best available UI font file, or None if no suitable font is found
pub fn find_best_ui_font() -> Option<String> {
    // Expand home directory in paths
    let mut search_paths = Vec::new();
    for dir in FONT_DIRECTORIES {
        if let Some(expanded) = expand_home_dir(dir) {
            search_paths.push(expanded);
        }
    }

    // Search for each preferred UI font in each directory
    for font_name in PREFERRED_UI_FONTS {
        for base_path in &search_paths {
            if let Some(font_path) = search_font_recursive(base_path, font_name) {
                eprintln!("[FONT] Found emoji-supporting UI font: {}", font_path.display());
                return Some(font_path.to_string_lossy().to_string());
            }
        }
    }

    eprintln!("[FONT] WARNING: No emoji-supporting UI fonts found in system directories");
    None
}

/// Discovers the best available monospace font with good emoji support
///
/// Searches through system font directories for preferred fonts known to have
/// good emoji rendering. Returns the first match found, or None if no suitable
/// font is available.
///
/// # Returns
///
/// The full path to the best available font file, or None if no suitable font is found
pub fn find_best_monospace_font() -> Option<String> {
    // Expand home directory in paths
    let mut search_paths = Vec::new();
    for dir in FONT_DIRECTORIES {
        if let Some(expanded) = expand_home_dir(dir) {
            search_paths.push(expanded);
        }
    }

    // Search for each preferred font in each directory
    for font_name in PREFERRED_MONOSPACE_FONTS {
        for base_path in &search_paths {
            if let Some(font_path) = search_font_recursive(base_path, font_name) {
                eprintln!("[FONT] Found emoji-supporting monospace font: {}", font_path.display());
                return Some(font_path.to_string_lossy().to_string());
            }
        }
    }

    eprintln!("[FONT] WARNING: No emoji-supporting monospace fonts found in system directories");
    None
}

/// Searches for a specific font file by name across all font directories.
///
/// # Arguments
///
/// * `font_name` - The exact filename to search for (e.g., "FreeMono.ttf")
///
/// # Returns
///
/// The full path to the font file if found, or None
pub fn find_specific_font(font_name: &str) -> Option<String> {
    // Expand home directory in paths
    let mut search_paths = Vec::new();
    for dir in FONT_DIRECTORIES {
        if let Some(expanded) = expand_home_dir(dir) {
            search_paths.push(expanded);
        }
    }

    // Search for the specific font in each directory
    for base_path in &search_paths {
        if let Some(font_path) = search_font_recursive(base_path, font_name) {
            return Some(font_path.to_string_lossy().to_string());
        }
    }

    None
}

/// Searches for the best available emoji font with color emoji support.
///
/// Searches through system font directories for emoji fonts that support
/// color rendering. Returns the first match found, or None if no suitable
/// font is available.
///
/// # Returns
///
/// The full path to the best available emoji font file, or None if no suitable font is found
pub fn find_emoji_font() -> Option<String> {
    // Expand home directory in paths
    let mut search_paths = Vec::new();
    for dir in FONT_DIRECTORIES {
        if let Some(expanded) = expand_home_dir(dir) {
            search_paths.push(expanded);
        }
    }

    // Search for each preferred emoji font in each directory
    for font_name in PREFERRED_EMOJI_FONTS {
        for base_path in &search_paths {
            if let Some(font_path) = search_font_recursive(base_path, font_name) {
                eprintln!("[FONT] Found color emoji font: {}", font_path.display());
                return Some(font_path.to_string_lossy().to_string());
            }
        }
    }

    eprintln!("[FONT] WARNING: No color emoji fonts found in system directories");
    None
}

/// Searches for the best available CJK (Chinese, Japanese, Korean) font.
///
/// Searches through system font directories for fonts that support
/// CJK characters. Returns the first match found, or None if no suitable
/// font is available.
///
/// # Returns
///
/// The full path to the best available CJK font file, or None if no suitable font is found
pub fn find_cjk_font() -> Option<String> {
    // Expand home directory in paths
    let mut search_paths = Vec::new();
    for dir in FONT_DIRECTORIES {
        if let Some(expanded) = expand_home_dir(dir) {
            search_paths.push(expanded);
        }
    }

    // Search for each preferred CJK font in each directory
    for font_name in PREFERRED_CJK_FONTS {
        for base_path in &search_paths {
            if let Some(font_path) = search_font_recursive(base_path, font_name) {
                eprintln!("[FONT] Found CJK font: {}", font_path.display());
                return Some(font_path.to_string_lossy().to_string());
            }
        }
    }

    eprintln!("[FONT] WARNING: No CJK fonts found in system directories");
    None
}

/// Recursively searches for a font file in a directory tree
///
/// # Arguments
///
/// * `base_path` - The directory to start searching from
/// * `font_name` - The filename to search for
///
/// # Returns
///
/// The full path to the font if found, or None
fn search_font_recursive(base_path: &Path, font_name: &str) -> Option<PathBuf> {
    // Check if the directory exists
    if !base_path.exists() || !base_path.is_dir() {
        return None;
    }

    // Try to read directory contents
    let entries = match fs::read_dir(base_path) {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    // Search through all entries
    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_file() {
            // Check if this is the font we're looking for
            if let Some(filename) = path.file_name() {
                if filename == font_name {
                    return Some(path);
                }
            }
        } else if path.is_dir() {
            // Recursively search subdirectories
            if let Some(found) = search_font_recursive(&path, font_name) {
                return Some(found);
            }
        }
    }

    None
}

/// Expands ~ to the user's home directory and Windows environment variables
///
/// # Arguments
///
/// * `path` - A path string that may contain ~ or Windows %VAR% variables
///
/// # Returns
///
/// The expanded path, or None if environment variables cannot be determined
fn expand_home_dir(path: &str) -> Option<PathBuf> {
    // Handle Windows environment variables like %USERPROFILE%, %LOCALAPPDATA%
    if path.contains('%') {
        let mut expanded = String::new();
        let mut chars = path.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '%' {
                // Find the closing %
                let mut var_name = String::new();
                let mut found_closing = false;

                while let Some(&next_ch) = chars.peek() {
                    if next_ch == '%' {
                        chars.next(); // consume the closing %
                        found_closing = true;
                        break;
                    }
                    var_name.push(chars.next().unwrap());
                }

                if found_closing && !var_name.is_empty() {
                    // Try to expand the environment variable
                    if let Ok(var_value) = std::env::var(&var_name) {
                        expanded.push_str(&var_value);
                    } else {
                        // If variable doesn't exist, keep the original pattern
                        expanded.push('%');
                        expanded.push_str(&var_name);
                        expanded.push('%');
                    }
                } else {
                    // No closing % found, keep the original
                    expanded.push('%');
                    expanded.push_str(&var_name);
                }
            } else {
                expanded.push(ch);
            }
        }
        return Some(PathBuf::from(expanded));
    }

    // Handle Unix-style ~ expansion
    if path.starts_with("~/") || path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(home);
            if path == "~" {
                return Some(home_path);
            } else {
                return Some(home_path.join(&path[2..]));
            }
        }
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_home_dir() {
        // Set HOME for testing
        std::env::set_var("HOME", "/home/testuser");

        assert_eq!(expand_home_dir("~/.fonts").unwrap(), PathBuf::from("/home/testuser/.fonts"));

        assert_eq!(expand_home_dir("~").unwrap(), PathBuf::from("/home/testuser"));

        assert_eq!(expand_home_dir("/usr/share/fonts").unwrap(), PathBuf::from("/usr/share/fonts"));
    }

    #[test]
    fn test_find_best_font_returns_path_or_none() {
        // This test will return Some with a .ttf path, or None if no fonts are found
        let font_path = find_best_monospace_font();
        if let Some(path) = font_path {
            assert!(!path.is_empty());
            assert!(path.ends_with(".ttf"));
        }
    }

    #[test]
    fn test_find_best_ui_font_returns_path_or_none() {
        // This test will return Some with a .ttf path, or None if no fonts are found
        let font_path = find_best_ui_font();
        if let Some(path) = font_path {
            assert!(!path.is_empty());
            assert!(path.ends_with(".ttf"));
        }
    }
}
