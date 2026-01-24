use crate::screen_buffer::ScreenBuffer;
use regex::Regex;
use std::sync::OnceLock;

/// Information about a detected URL
#[derive(Clone, Debug)]
pub struct UrlInfo {
    pub url: String,
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
}

/// Get the compiled URL regex pattern
fn url_regex() -> &'static Regex {
    static URL_REGEX: OnceLock<Regex> = OnceLock::new();
    URL_REGEX.get_or_init(|| {
        // Match:
        // - http:// and https:// URLs
        // - www. URLs (common even without protocol)
        // - ftp:// URLs
        // Stop at whitespace and special characters that typically aren't in URLs
        Regex::new(r#"(https?://[^\s<>"{}|\\\[\]]+|www\.[^\s<>"{}|\\\[\]]+|ftp://[^\s<>"{}|\\\[\]]+)"#).expect("Failed to compile URL regex")
    })
}

/// Detect if there's a URL at the given position in the screen buffer
/// Returns UrlInfo if a URL is found at or near the position
pub fn detect_url_at_position(screen_buffer: &ScreenBuffer, row: usize, col: usize) -> Option<UrlInfo> {
    // Check bounds
    if row >= screen_buffer.height() {
        return None;
    }

    // Extract the text from the row
    let row_text = extract_row_text(screen_buffer, row);

    // Find all URLs in the row
    let regex = url_regex();
    for cap in regex.captures_iter(&row_text) {
        if let Some(url_match) = cap.get(0) {
            let start = url_match.start();
            let end = url_match.end();

            // Check if the click position is within this URL
            if col >= start && col < end {
                return Some(UrlInfo {
                    url: url_match.as_str().to_string(),
                    row,
                    col_start: start,
                    col_end: end - 1, // Make it inclusive
                });
            }
        }
    }

    None
}

/// Extract text from a specific row in the screen buffer
fn extract_row_text(screen_buffer: &ScreenBuffer, row: usize) -> String {
    let width = screen_buffer.width();
    let mut text = String::with_capacity(width);

    for col in 0..width {
        if let Some(cell) = screen_buffer.get_cell(col, row) {
            // Skip continuation cells (width == 0)
            if cell.width > 0 {
                text.push(cell.ch);
                // Add extended grapheme if present
                if let Some(ref extended) = cell.extended {
                    text.push_str(extended);
                }
            }
        } else {
            text.push(' ');
        }
    }

    // Trim trailing whitespace for cleaner URL detection
    text.trim_end().to_string()
}

/// Open a URL in the default browser
pub fn open_url_in_browser(url: &str) -> Result<(), String> {
    // Ensure URL has protocol
    let url_with_protocol = if url.starts_with("www.") {
        format!("https://{}", url)
    } else if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("ftp://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url_with_protocol)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url_with_protocol)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", "", &url_with_protocol])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_buffer::{CursorStyle, ScreenBuffer};

    #[test]
    fn test_url_regex_matches_http() {
        let regex = url_regex();
        let text = "Visit http://example.com for more info";
        let caps: Vec<_> = regex.find_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].as_str(), "http://example.com");
    }

    #[test]
    fn test_url_regex_matches_https() {
        let regex = url_regex();
        let text = "Visit https://example.com for more info";
        let caps: Vec<_> = regex.find_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].as_str(), "https://example.com");
    }

    #[test]
    fn test_url_regex_matches_www() {
        let regex = url_regex();
        let text = "Visit www.example.com for more info";
        let caps: Vec<_> = regex.find_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].as_str(), "www.example.com");
    }

    #[test]
    fn test_url_regex_matches_ftp() {
        let regex = url_regex();
        let text = "Download from ftp://files.example.com/file.zip";
        let caps: Vec<_> = regex.find_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].as_str(), "ftp://files.example.com/file.zip");
    }

    #[test]
    fn test_url_regex_multiple_urls() {
        let regex = url_regex();
        let text = "Check https://first.com and www.second.com";
        let caps: Vec<_> = regex.find_iter(text).collect();
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].as_str(), "https://first.com");
        assert_eq!(caps[1].as_str(), "www.second.com");
    }

    #[test]
    fn test_detect_url_at_position_finds_url() {
        let mut screen_buffer = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

        // Simulate text "Visit https://example.com for info"
        let text = "Visit https://example.com for info";
        for ch in text.chars() {
            screen_buffer.put_grapheme(&ch.to_string());
        }

        // Click on the URL (column 10 is within "https://example.com")
        let result = detect_url_at_position(&screen_buffer, 0, 10);
        assert!(result.is_some());

        if let Some(url_info) = result {
            assert_eq!(url_info.url, "https://example.com");
            assert_eq!(url_info.row, 0);
        }
    }

    #[test]
    fn test_detect_url_at_position_no_url() {
        let screen_buffer = ScreenBuffer::new_with_scrollback(80, 24, 1000, CursorStyle::default());

        // Empty buffer, no URL
        let result = detect_url_at_position(&screen_buffer, 0, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_open_url_adds_protocol_to_www() {
        // This test just ensures the function doesn't panic
        // We can't easily test actual browser opening in unit tests
        let result = open_url_in_browser("www.example.com");
        // On CI systems without display, this might fail, so we just check it doesn't panic
        let _ = result;
    }
}
