use std::fs;
use std::path::PathBuf;

// Static shell initialization scripts
const BASH_INIT_SCRIPT: &str = include_str!("../../static/scripts/bash_init.sh");
const ZSH_INIT_SCRIPT: &str = include_str!("../../static/scripts/zsh_init.sh");

// History persistence limits
pub(crate) const MAX_COMMAND_HISTORY: usize = 5; // Maximum number of commands to keep in history
pub(crate) const MAX_OUTPUT_HISTORY: usize = 100; // Maximum number of output lines to keep in history

/// Create a temporary shell init file that configures exit code reporting
pub(crate) fn create_shell_init_file(shell_name: &str) -> Option<PathBuf> {
    match shell_name {
        "bash" => {
            // Create temporary .bashrc with PROMPT_COMMAND
            let temp_dir = std::env::temp_dir();
            let init_file = temp_dir.join(format!("nist_bashrc_{}", std::process::id()));

            if fs::write(&init_file, BASH_INIT_SCRIPT).is_ok() {
                Some(init_file)
            } else {
                eprintln!("[TERMINAL] Failed to create bash init file");
                None
            }
        }
        "zsh" => {
            // Create temporary .zshrc with precmd hook
            let temp_dir = std::env::temp_dir();
            let zsh_dir = temp_dir.join(format!("nist_zsh_{}", std::process::id()));
            let _ = fs::create_dir_all(&zsh_dir);
            let init_file = zsh_dir.join(".zshrc");

            if fs::write(&init_file, ZSH_INIT_SCRIPT).is_ok() {
                Some(init_file)
            } else {
                eprintln!("[TERMINAL] Failed to create zsh init file");
                None
            }
        }
        _ => {
            // No init file for other shells
            None
        }
    }
}
