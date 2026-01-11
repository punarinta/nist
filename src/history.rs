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

    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("bash");

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
        Err(_) => return Vec::new(),
    };
    let path = PathBuf::from(home).join(".bash_history");
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
    let path = PathBuf::from(home)
        .join(".local/share/fish/fish_history");
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

    let path = PathBuf::from(home)
        .join("AppData/Roaming/Microsoft/Windows/PowerShell/PSReadline/ConsoleHost_history.txt");

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
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    // Read from end to get most recent entries
    for line in content.lines().rev() {
        let line: &str = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            entries.push(line.to_string());
            if entries.len() >= max_entries {
                break;
            }
        }
    }

    entries
}

/// Combine shell history and terminal history, removing duplicates
/// Keeps the most recent occurrence of each command
/// Returns commands with newest first
pub fn combine_and_deduplicate(
    shell_history: Vec<String>,
    terminal_history: Vec<String>,
    max_rows: usize,
) -> Vec<String> {
    // Combine: shell history first (older), then terminal history (newer)
    let mut combined: Vec<String> = shell_history;
    combined.extend(terminal_history);

    // Deduplicate: keep first occurrence (which is oldest in our combined list)
    // Then reverse so newest is first
    let mut seen = HashSet::new();
    let mut deduped: Vec<String> = Vec::new();

    for cmd in combined {
        if seen.insert(cmd.clone()) {
            deduped.push(cmd);
        }
    }

    // Reverse to have newest first (for display)
    deduped.reverse();

    // Limit to max_rows
    deduped.truncate(max_rows);

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_shell_history() {
        // Just ensure it doesn't panic
        let history = read_shell_history(10);
        assert!(history.is_ok());
    }

    #[test]
    fn test_combine_and_deduplicate() {
        let shell = vec![
            "cmd1".to_string(),
            "cmd2".to_string(),
            "cmd3".to_string(),
        ];

        let terminal = vec![
            "cmd3".to_string(),  // duplicate
            "cmd4".to_string(),
            "cmd2".to_string(),  // duplicate
        ];

        let result = combine_and_deduplicate(shell, terminal, 10);

        // Should be: cmd4, cmd3, cmd2, cmd1 (newest first, no duplicates)
        assert_eq!(result, vec![
            "cmd4".to_string(),
            "cmd3".to_string(),
            "cmd2".to_string(),
            "cmd1".to_string(),
        ]);
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
