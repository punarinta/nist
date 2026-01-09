// Internal terminal library - hardcoded knowledge about different shells and their peculiarities
// This is NOT user configuration - it's our internal knowledge base about how different shells work

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub command: String,
    pub args: Vec<String>,
    pub keys: KeyMappings,
}

#[derive(Debug, Clone)]
pub struct KeyMappings {
    pub backspace: Vec<u8>,
    pub _delete: Vec<u8>,
    pub _return_key: Vec<u8>,
}

pub struct TerminalLibrary {
    shells: HashMap<String, ShellConfig>,
    default_shell: String,
}

impl TerminalLibrary {
    pub fn new() -> Self {
        let mut shells = HashMap::new();

        // sh - POSIX shell
        // Uses DEL (127) for backspace
        shells.insert(
            "sh".to_string(),
            ShellConfig {
                command: "sh".to_string(),
                args: vec![],
                keys: KeyMappings {
                    backspace: vec![127],           // DEL character
                    _delete: vec![27, 91, 51, 126], // ESC [ 3 ~
                    _return_key: vec![10],          // LF
                },
            },
        );

        // bash - Bourne Again Shell
        // Uses BS SPACE BS sequence for backspace (visual erase)
        shells.insert(
            "bash".to_string(),
            ShellConfig {
                command: "bash".to_string(),
                args: vec!["--noprofile".to_string()],
                keys: KeyMappings {
                    backspace: vec![8, 32, 8],      // BS SPACE BS sequence
                    _delete: vec![27, 91, 51, 126], // ESC [ 3 ~
                    _return_key: vec![10],          // LF
                },
            },
        );

        // cmd.exe - Windows Command Prompt
        // Uses simple BS for backspace
        shells.insert(
            "cmd".to_string(),
            ShellConfig {
                command: "cmd.exe".to_string(),
                args: vec!["/D".to_string()],
                keys: KeyMappings {
                    backspace: vec![8],             // BS character
                    _delete: vec![27, 91, 51, 126], // ESC [ 3 ~
                    _return_key: vec![13],          // CR (Windows uses CR for enter)
                },
            },
        );

        // PowerShell - Windows PowerShell
        // Uses simple BS for backspace
        shells.insert(
            "powershell".to_string(),
            ShellConfig {
                command: "powershell.exe".to_string(),
                args: vec!["-NoLogo".to_string(), "-NoProfile".to_string()],
                keys: KeyMappings {
                    backspace: vec![8],             // BS character
                    _delete: vec![27, 91, 51, 126], // ESC [ 3 ~
                    _return_key: vec![13],          // CR
                },
            },
        );

        // zsh - Z Shell (default on macOS)
        // Uses DEL (127) for backspace like sh
        shells.insert(
            "zsh".to_string(),
            ShellConfig {
                command: "zsh".to_string(),
                args: vec!["--no-globalrcs".to_string()],
                keys: KeyMappings {
                    backspace: vec![127],           // DEL character
                    _delete: vec![27, 91, 51, 126], // ESC [ 3 ~
                    _return_key: vec![10],          // LF
                },
            },
        );

        // Determine platform-specific default shell
        #[cfg(target_os = "windows")]
        let default_shell = "cmd".to_string();

        #[cfg(target_os = "macos")]
        let default_shell = "zsh".to_string();

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let default_shell = "bash".to_string();

        TerminalLibrary { shells, default_shell }
    }

    pub fn get_default_shell(&self) -> &ShellConfig {
        self.shells.get(&self.default_shell).expect("Default shell must exist in library")
    }
}

impl Default for TerminalLibrary {
    fn default() -> Self {
        Self::new()
    }
}
