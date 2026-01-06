// Internal terminal library - hardcoded knowledge about different shells and their peculiarities
// This is NOT user configuration - it's our internal knowledge base about how different shells work

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub command: String,
    pub _env_term: String,
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
                _env_term: "xterm-256color".to_string(),
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
                _env_term: "xterm-256color".to_string(),
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
                _env_term: "xterm-256color".to_string(),
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
                _env_term: "xterm-256color".to_string(),
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
                _env_term: "xterm-256color".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_configs_exist() {
        let library = TerminalLibrary::new();

        // Verify all shell configurations are present
        assert!(library.shells.contains_key("sh"));
        assert!(library.shells.contains_key("bash"));
        assert!(library.shells.contains_key("cmd"));
        assert!(library.shells.contains_key("powershell"));
        assert!(library.shells.contains_key("zsh"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_windows_default_shell() {
        let library = TerminalLibrary::new();

        // On Windows, default should be cmd
        assert_eq!(library.default_shell, "cmd");

        let default = library.get_default_shell();
        assert_eq!(default.command, "cmd.exe");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_macos_default_shell() {
        let library = TerminalLibrary::new();

        // On macOS, default should be zsh
        assert_eq!(library.default_shell, "zsh");

        let default = library.get_default_shell();
        assert_eq!(default.command, "zsh");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_linux_default_shell() {
        let library = TerminalLibrary::new();

        // On Linux, default should be bash
        assert_eq!(library.default_shell, "bash");

        let default = library.get_default_shell();
        assert_eq!(default.command, "bash");
    }

    #[test]
    fn test_cmd_configuration() {
        let library = TerminalLibrary::new();
        let cmd_config = library.shells.get("cmd").unwrap();

        assert_eq!(cmd_config.command, "cmd.exe");
        assert_eq!(cmd_config.keys.backspace, vec![8]);
        assert_eq!(cmd_config.keys._return_key, vec![13]);
    }

    #[test]
    fn test_powershell_configuration() {
        let library = TerminalLibrary::new();
        let ps_config = library.shells.get("powershell").unwrap();

        assert_eq!(ps_config.command, "powershell.exe");
        assert_eq!(ps_config.keys.backspace, vec![8]);
        assert_eq!(ps_config.keys._return_key, vec![13]);
    }

    #[test]
    fn test_bash_configuration() {
        let library = TerminalLibrary::new();
        let bash_config = library.shells.get("bash").unwrap();

        assert_eq!(bash_config.command, "bash");
        assert_eq!(bash_config.keys.backspace, vec![8, 32, 8]);
        assert_eq!(bash_config.keys._return_key, vec![10]);
    }

    #[test]
    fn test_sh_configuration() {
        let library = TerminalLibrary::new();
        let sh_config = library.shells.get("sh").unwrap();

        assert_eq!(sh_config.command, "sh");
        assert_eq!(sh_config.keys.backspace, vec![127]);
        assert_eq!(sh_config.keys._return_key, vec![10]);
    }

    #[test]
    fn test_zsh_configuration() {
        let library = TerminalLibrary::new();
        let zsh_config = library.shells.get("zsh").unwrap();

        assert_eq!(zsh_config.command, "zsh");
        assert_eq!(zsh_config.keys.backspace, vec![127]);
        assert_eq!(zsh_config.keys._return_key, vec![10]);
    }
}
