use crate::ghostty_buffer::GhosttyBuffer;
use crate::pane_layout::PaneId;
use regex::Regex;
use std::sync::OnceLock;

/// Information about a detected URL
#[derive(Clone, Debug)]
pub struct UrlInfo {
    pub url: String,
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub pane_id: PaneId,
}

/// Get the compiled URL regex pattern
fn url_regex() -> &'static Regex {
    static URL_REGEX: OnceLock<Regex> = OnceLock::new();
    URL_REGEX.get_or_init(|| {
        Regex::new(r#"(https?://[^\s<>"{}|\\\[\]]+|www\.[^\s<>"{}|\\\[\]]+|ftp://[^\s<>"{}|\\\[\]]+)"#).expect("Failed to compile URL regex")
    })
}

/// Detect if there's a URL at the given position in the screen buffer
pub fn detect_url_at_position(gb: &GhosttyBuffer, row: usize, col: usize, pane_id: PaneId) -> Option<UrlInfo> {
    if row >= gb.height() {
        return None;
    }

    let row_text = extract_row_text(gb, row);

    let regex = url_regex();
    for cap in regex.captures_iter(&row_text) {
        if let Some(url_match) = cap.get(0) {
            let start = url_match.start();
            let end = url_match.end();

            if col >= start && col < end {
                return Some(UrlInfo {
                    url: url_match.as_str().to_string(),
                    row,
                    col_start: start,
                    col_end: end - 1,
                    pane_id,
                });
            }
        }
    }

    None
}

/// Extract text from a specific viewport row using GhosttyBuffer.
/// row=0 means the topmost visible row.
fn extract_row_text(gb: &GhosttyBuffer, row: usize) -> String {
    let width = gb.width();
    // Absolute row: scrollback_len + row = first cell of the live viewport + row offset
    let abs_row = gb.scrollback_len() + row;
    let mut text = String::with_capacity(width);

    for col in 0..width {
        let graphemes = gb.graphemes_at(col, abs_row);
        let cell_width = gb.cell_width_at(col, abs_row);
        if cell_width > 0 {
            if graphemes.is_empty() {
                text.push(' ');
            } else {
                for ch in &graphemes {
                    text.push(*ch);
                }
            }
        }
    }

    text.trim_end().to_string()
}

/// Open a URL in the default browser
pub fn open_url_in_browser(url: &str) -> Result<(), String> {
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
