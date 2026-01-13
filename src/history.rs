use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Read shell history for cross-platform support
pub fn read_shell_history(max_entries: usize) -> Vec<String> {
    let shell = env::var("SHELL").unwrap_or_else(|_| {
        // Default based on OS
        if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/bash".to_string()
        }
    });

    let shell_name = Path::new(&shell).file_name().and_then(|s| s.to_str()).unwrap_or("bash");
    eprintln!("[HISTORY] Reading history for shell: {} (from $SHELL={})", shell_name, shell);

    match shell_name {
        "bash" => read_bash_history(max_entries),
        "zsh" => read_zsh_history(max_entries),
        "fish" => read_fish_history(max_entries),
        "powershell" | "pwsh" => read_powershell_history(max_entries),
        "cmd.exe" | "cmd" => read_cmd_history(max_entries),
        _ => read_bash_history(max_entries), // Fallback
    }
}

/// Read bash history from ~/.bash_history
fn read_bash_history(max_entries: usize) -> Vec<String> {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("[HISTORY] HOME environment variable not set");
            return Vec::new();
        }
    };
    let path = PathBuf::from(home).join(".bash_history");
    eprintln!("[HISTORY] Reading bash history from: {:?}", path);
    read_history_file(&path, max_entries)
}

/// Read zsh history from ~/.zsh_history
fn read_zsh_history(max_entries: usize) -> Vec<String> {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let path = PathBuf::from(home).join(".zsh_history");
    let mut entries = Vec::new();

    if let Ok(content) = fs::read_to_string(&path) {
        for line in content.lines().rev() {
            if entries.len() >= max_entries {
                break;
            }

            // Zsh history format: : <timestamp>:<duration>;<command>
            // Extract the command part after the semicolon
            if let Some(cmd) = line.split(';').nth(1) {
                let cmd = cmd.trim();
                if !cmd.is_empty() && !cmd.starts_with('#') {
                    entries.push(cmd.to_string());
                }
            }
        }
    }

    entries
}

/// Read fish history from ~/.local/share/fish/fish_history
fn read_fish_history(max_entries: usize) -> Vec<String> {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let path = PathBuf::from(home).join(".local/share/fish/fish_history");
    let mut entries = Vec::new();

    if let Ok(content) = fs::read_to_string(&path) {
        for line in content.lines() {
            if entries.len() >= max_entries {
                break;
            }

            if line.starts_with("- cmd:") {
                if let Some(cmd) = line.strip_prefix("- cmd:") {
                    let cmd = cmd.trim().trim_matches('"');
                    if !cmd.is_empty() {
                        entries.push(cmd.to_string());
                    }
                }
            }
        }
    }

    entries
}

/// Read PowerShell history
fn read_powershell_history(max_entries: usize) -> Vec<String> {
    let home = if cfg!(windows) {
        match env::var("USERPROFILE") {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        }
    } else {
        match env::var("HOME") {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        }
    };

    let path = PathBuf::from(home).join("AppData/Roaming/Microsoft/Windows/PowerShell/PSReadline/ConsoleHost_history.txt");

    read_history_file(&path, max_entries)
}

/// Read Windows CMD history (limited support)
fn read_cmd_history(_max_entries: usize) -> Vec<String> {
    // CMD doesn't have a persistent history file by default
    // Return empty Vec - will fall back to terminal's private history
    Vec::new()
}

/// Generic history file reader (reads from end, most recent first)
fn read_history_file(path: &Path, max_entries: usize) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[HISTORY] Failed to read history file {:?}: {}", path, e);
            return Vec::new();
        }
    };

    let total_lines = content.lines().count();
    eprintln!("[HISTORY] History file has {} total lines", total_lines);

    let mut entries = Vec::new();

    // Read from end to get most recent entries
    for line in content.lines().rev() {
        let line: &str = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Clean numbered history entries (e.g., " 1747  nist -v" -> "nist -v")
        let cleaned = clean_history_line(line);
        if !cleaned.is_empty() {
            entries.push(cleaned);
            if entries.len() >= max_entries {
                break;
            }
        }
    }

    eprintln!("[HISTORY] Read {} history entries (newest first)", entries.len());
    if !entries.is_empty() {
        eprintln!("[HISTORY] Most recent entry: {:?}", entries[0]);
        if entries.len() > 1 {
            eprintln!("[HISTORY] Second entry: {:?}", entries[1]);
        }
    }

    entries
}

/// Clean a history line by removing numbered prefixes
/// Handles formats like: " 1747  nist -v", "1747 cargo run", etc.
fn clean_history_line(line: &str) -> String {
    let trimmed = line.trim();

    // Check if line starts with digits (possibly after whitespace)
    let mut chars = trimmed.chars();
    let first_char = match chars.next() {
        Some(ch) => ch,
        None => return String::new(),
    };

    // If first character is a digit, this might be a numbered entry
    if first_char.is_ascii_digit() {
        // Find where the digits end
        let mut digit_end = 1;
        for ch in chars {
            if ch.is_ascii_digit() {
                digit_end += 1;
            } else {
                break;
            }
        }

        // Extract the part after the digits
        if digit_end < trimmed.len() {
            let remaining = &trimmed[digit_end..];
            // Skip any whitespace after the number
            let command = remaining.trim_start();
            if !command.is_empty() {
                return command.to_string();
            }
        }
        // If nothing after digits, return empty
        return String::new();
    }

    // Not a numbered entry, return as-is
    trimmed.to_string()
}

/// Combine shell history and terminal history, removing duplicates
/// Keeps the most recent occurrence of each command
/// Returns commands with newest first
pub fn combine_and_deduplicate(shell_history: Vec<String>, terminal_history: Vec<String>, max_rows: usize) -> Vec<String> {
    // Combine: shell_history (oldest first), then terminal_history (newest commands)
    // This creates a timeline from oldest to newest
    let mut combined: Vec<String> = shell_history;
    combined.extend(terminal_history);

    // Deduplicate: keep newest occurrence by iterating in reverse
    // When we iterate backwards, the first time we see a command is its newest occurrence
    let mut seen = HashSet::new();
    let mut deduped: Vec<String> = Vec::new();

    // Iterate in reverse (newest to oldest) to keep newest occurrence
    for cmd in combined.into_iter().rev() {
        if seen.insert(cmd.clone()) {
            deduped.push(cmd);
        }
    }

    // Result is already newest-first (since we iterated in reverse)

    // Limit to max_rows
    deduped.truncate(max_rows);

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_history_line() {
        // Test numbered entries
        assert_eq!(clean_history_line(" 1747  nist -v"), "nist -v");
        assert_eq!(clean_history_line("1748 cargo run -- -v"), "cargo run -- -v");

        // Test regular entries (no numbers)
        assert_eq!(clean_history_line("ls -la"), "ls -la");
        assert_eq!(clean_history_line("  cd /tmp  "), "cd /tmp");

        // Test edge cases
        assert_eq!(clean_history_line("123"), ""); // Just a number
        assert_eq!(clean_history_line(""), "");
        assert_eq!(clean_history_line("  "), "");
    }

    #[test]
    fn test_read_shell_history() {
        // Just ensure it doesn't panic
        let history = read_shell_history(10);
        // history is Vec<String>, not Result
        assert!(history.len() >= 0);
    }

    #[test]
    fn test_combine_and_deduplicate() {
        let shell = vec!["cmd1".to_string(), "cmd2".to_string(), "cmd3".to_string()];

        let terminal = vec![
            "cmd3".to_string(), // duplicate
            "cmd4".to_string(),
            "cmd2".to_string(), // duplicate
        ];

        let result = combine_and_deduplicate(shell, terminal, 10);

        // Should be: cmd4, cmd3, cmd2, cmd1 (newest first, no duplicates)
        assert_eq!(result, vec!["cmd4".to_string(), "cmd3".to_string(), "cmd2".to_string(), "cmd1".to_string(),]);
    }

    #[test]
    fn test_combine_and_deduplicate_max_rows() {
        let shell = (0..20).map(|i| format!("shell_cmd_{}", i)).collect();
        let terminal = (20..30).map(|i| format!("term_cmd_{}", i)).collect();

        let result = combine_and_deduplicate(shell, terminal, 8);

        assert_eq!(result.len(), 8);
        // Should have newest 8 entries
        assert_eq!(result[0], "term_cmd_29");
        assert_eq!(result[7], "term_cmd_22");
    }

    #[test]
    fn test_combine_and_deduplicate_empty() {
        let result = combine_and_deduplicate(vec![], vec![], 10);
        assert!(result.is_empty());
    }
}
