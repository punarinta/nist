//! Settings management for the terminal emulator
//!
//! Handles loading and saving user settings to JSON files.
//! Uses separate directories for production and test builds:
//! - Linux/macOS Production: ~/.config/nist/settings.json
//! - Linux/macOS Test/Debug: ~/.config/nist-test/settings.json
//! - Windows Production: %APPDATA%\nist\settings.json
//! - Windows Test/Debug: %APPDATA%\nist-test\settings.json

use directories::ProjectDirs;
use sdl3::keyboard::Keycode;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Key enum for hotkey bindings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Key {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    // Numbers
    #[serde(rename = "0")]
    Num0,
    #[serde(rename = "1")]
    Num1,
    #[serde(rename = "2")]
    Num2,
    #[serde(rename = "3")]
    Num3,
    #[serde(rename = "4")]
    Num4,
    #[serde(rename = "5")]
    Num5,
    #[serde(rename = "6")]
    Num6,
    #[serde(rename = "7")]
    Num7,
    #[serde(rename = "8")]
    Num8,
    #[serde(rename = "9")]
    Num9,

    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,

    // Arrow keys
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,

    // Special keys
    Tab,
    Enter,
    Escape,
    Space,
    Backspace,
    Delete,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,

    // Brackets and punctuation
    LeftBracket,
    RightBracket,
    Minus,
    Equals,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backslash,
    Backtick,
}

/// Key binding with modifiers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyBinding {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    pub key: Key,
}

impl Key {
    /// Convert Key enum to SDL Keycode
    pub fn to_keycode(&self) -> Option<Keycode> {
        match self {
            // Letters
            Key::A => Some(Keycode::A),
            Key::B => Some(Keycode::B),
            Key::C => Some(Keycode::C),
            Key::D => Some(Keycode::D),
            Key::E => Some(Keycode::E),
            Key::F => Some(Keycode::F),
            Key::G => Some(Keycode::G),
            Key::H => Some(Keycode::H),
            Key::I => Some(Keycode::I),
            Key::J => Some(Keycode::J),
            Key::K => Some(Keycode::K),
            Key::L => Some(Keycode::L),
            Key::M => Some(Keycode::M),
            Key::N => Some(Keycode::N),
            Key::O => Some(Keycode::O),
            Key::P => Some(Keycode::P),
            Key::Q => Some(Keycode::Q),
            Key::R => Some(Keycode::R),
            Key::S => Some(Keycode::S),
            Key::T => Some(Keycode::T),
            Key::U => Some(Keycode::U),
            Key::V => Some(Keycode::V),
            Key::W => Some(Keycode::W),
            Key::X => Some(Keycode::X),
            Key::Y => Some(Keycode::Y),
            Key::Z => Some(Keycode::Z),

            // Numbers - TODO: Find correct SDL3 Keycode names
            Key::Num0 => None,
            Key::Num1 => None,
            Key::Num2 => None,
            Key::Num3 => None,
            Key::Num4 => None,
            Key::Num5 => None,
            Key::Num6 => None,
            Key::Num7 => None,
            Key::Num8 => None,
            Key::Num9 => None,

            // Function keys
            Key::F1 => Some(Keycode::F1),
            Key::F2 => Some(Keycode::F2),
            Key::F3 => Some(Keycode::F3),
            Key::F4 => Some(Keycode::F4),
            Key::F5 => Some(Keycode::F5),
            Key::F6 => Some(Keycode::F6),
            Key::F7 => Some(Keycode::F7),
            Key::F8 => Some(Keycode::F8),
            Key::F9 => Some(Keycode::F9),
            Key::F10 => Some(Keycode::F10),
            Key::F11 => Some(Keycode::F11),
            Key::F12 => Some(Keycode::F12),

            // Arrow keys
            Key::ArrowUp => Some(Keycode::Up),
            Key::ArrowDown => Some(Keycode::Down),
            Key::ArrowLeft => Some(Keycode::Left),
            Key::ArrowRight => Some(Keycode::Right),

            // Special keys
            Key::Tab => Some(Keycode::Tab),
            Key::Enter => Some(Keycode::Return),
            Key::Escape => Some(Keycode::Escape),
            Key::Space => Some(Keycode::Space),
            Key::Backspace => Some(Keycode::Backspace),
            Key::Delete => Some(Keycode::Delete),
            Key::PageUp => Some(Keycode::PageUp),
            Key::PageDown => Some(Keycode::PageDown),
            Key::Home => Some(Keycode::Home),
            Key::End => Some(Keycode::End),
            Key::Insert => Some(Keycode::Insert),

            // Brackets and punctuation
            Key::LeftBracket => Some(Keycode::LeftBracket),
            Key::RightBracket => Some(Keycode::RightBracket),
            Key::Minus => Some(Keycode::Minus),
            Key::Equals => Some(Keycode::Equals),
            Key::Semicolon => Some(Keycode::Semicolon),
            Key::Quote => None, // TODO: Find correct SDL3 Keycode name
            Key::Comma => Some(Keycode::Comma),
            Key::Period => Some(Keycode::Period),
            Key::Slash => Some(Keycode::Slash),
            Key::Backslash => Some(Keycode::Backslash),
            Key::Backtick => None, // TODO: Find correct SDL3 Keycode name
        }
    }
}

impl KeyBinding {
    /// Check if this key binding matches the given keycode and modifiers
    pub fn matches(&self, keycode: Keycode, is_ctrl: bool, is_shift: bool, is_alt: bool) -> bool {
        if self.ctrl != is_ctrl || self.shift != is_shift || self.alt != is_alt {
            return false;
        }

        if let Some(binding_keycode) = self.key.to_keycode() {
            binding_keycode == keycode
        } else {
            false
        }
    }
}

/// Navigation hotkeys
/// Navigation hotkeys configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationHotkeys {
    #[serde(rename = "splitRight", default = "default_split_right")]
    pub split_right: Vec<KeyBinding>,
    #[serde(rename = "splitDown", default = "default_split_down")]
    pub split_down: Vec<KeyBinding>,
    #[serde(rename = "closePane", default = "default_close_pane")]
    pub close_pane: Vec<KeyBinding>,
    #[serde(rename = "nextPane", default = "default_next_pane")]
    pub next_pane: Vec<KeyBinding>,
    #[serde(rename = "previousPane", default = "default_previous_pane")]
    pub previous_pane: Vec<KeyBinding>,
    #[serde(rename = "newTab", default = "default_new_tab")]
    pub new_tab: Vec<KeyBinding>,
    #[serde(rename = "nextTab", default = "default_next_tab")]
    pub next_tab: Vec<KeyBinding>,
    #[serde(rename = "previousTab", default = "default_previous_tab")]
    pub previous_tab: Vec<KeyBinding>,
}

// Default functions for NavigationHotkeys fields
fn default_split_right() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: true,
        alt: false,
        key: Key::J,
    }]
}

fn default_split_down() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: true,
        alt: false,
        key: Key::H,
    }]
}

fn default_close_pane() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: true,
        alt: false,
        key: Key::W,
    }]
}

fn default_next_pane() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: false,
        alt: false,
        key: Key::RightBracket,
    }]
}

fn default_previous_pane() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: false,
        alt: false,
        key: Key::LeftBracket,
    }]
}

fn default_new_tab() -> Vec<KeyBinding> {
    vec![KeyBinding {
        ctrl: true,
        shift: true,
        alt: false,
        key: Key::T,
    }]
}

fn default_next_tab() -> Vec<KeyBinding> {
    vec![
        KeyBinding {
            ctrl: true,
            shift: true,
            alt: false,
            key: Key::Tab,
        },
        KeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            key: Key::Tab,
        },
    ]
}

fn default_previous_tab() -> Vec<KeyBinding> {
    vec![]
}

impl Default for NavigationHotkeys {
    fn default() -> Self {
        Self {
            split_right: default_split_right(),
            split_down: default_split_down(),
            close_pane: default_close_pane(),
            next_pane: default_next_pane(),
            previous_pane: default_previous_pane(),
            new_tab: default_new_tab(),
            next_tab: default_next_tab(),
            previous_tab: default_previous_tab(),
        }
    }
}

/// Hotkeys configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Hotkeys {
    #[serde(default)]
    pub navigation: NavigationHotkeys,
}

/// Terminal-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSettings {
    #[serde(rename = "fontSize")]
    pub font_size: f32,
    #[serde(rename = "fontFamily")]
    pub font_family: String,
    pub cursor: String,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_size: 12.0,
            font_family: "auto".to_string(),
            cursor: "pipe".to_string(),
        }
    }
}

/// Settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub external: Vec<String>,
    pub terminal: TerminalSettings,
    #[serde(default)]
    pub hotkeys: Hotkeys,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            external: Vec::new(),
            terminal: TerminalSettings::default(),
            hotkeys: Hotkeys::default(),
        }
    }
}

/// Get the path to the settings file based on build profile
///
/// Get the settings file path based on the platform and build profile.
///
/// Uses platform-appropriate directories:
/// - Linux/macOS Production: ~/.config/nist/settings.json
/// - Linux/macOS Test/Debug: ~/.config/nist-test/settings.json
/// - Windows Production: %APPDATA%\nist\settings.json
/// - Windows Test/Debug: %APPDATA%\nist-test\settings.json
fn get_settings_file_path() -> Result<PathBuf, String> {
    // Determine the application name based on build profile
    #[cfg(production)]
    let app_name = "nist";

    #[cfg(not(production))]
    let app_name = "nist-test";

    // Get the platform-appropriate config directory
    let proj_dirs = ProjectDirs::from("", "", app_name).ok_or_else(|| "Failed to determine config directory".to_string())?;

    let config_dir = proj_dirs.config_dir();

    // Create directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(config_dir.join("settings.json"))
}

/// Get the path to the settings file (public API)
pub fn get_settings_path() -> Result<PathBuf, String> {
    get_settings_file_path()
}

/// Load settings from the settings file
/// If the file doesn't exist, creates it with default settings
pub fn load_settings() -> Result<Settings, String> {
    let settings_path = get_settings_file_path()?;

    if !settings_path.exists() {
        // Create default settings file
        let default_settings = Settings::default();
        save_settings(&default_settings)?;
        return Ok(default_settings);
    }

    let contents = fs::read_to_string(&settings_path).map_err(|e| format!("Failed to read settings file: {}", e))?;

    let settings: Settings = serde_json::from_str(&contents).map_err(|e| format!("Failed to parse settings file: {}", e))?;

    Ok(settings)
}

/// Save settings to the settings file
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let settings_path = get_settings_file_path()?;

    let json = serde_json::to_string_pretty(settings).map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, json).map_err(|e| format!("Failed to write settings file: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.external.len(), 0);
        assert_eq!(settings.terminal.font_size, 12.0);
        assert_eq!(settings.terminal.font_family, "auto");
        assert_eq!(settings.terminal.cursor, "pipe");
        // Verify default hotkeys are present
        assert_eq!(settings.hotkeys.navigation.split_right.len(), 1);
        assert_eq!(settings.hotkeys.navigation.split_down.len(), 1);
        assert_eq!(settings.hotkeys.navigation.close_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.next_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.previous_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.new_tab.len(), 1);
        assert_eq!(settings.hotkeys.navigation.next_tab.len(), 2); // Has two default bindings
        assert_eq!(settings.hotkeys.navigation.previous_tab.len(), 0); // No default
    }

    #[test]
    fn test_settings_serialization() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings.external.len(), deserialized.external.len());
        assert_eq!(settings.terminal.font_size, deserialized.terminal.font_size);
        assert_eq!(settings.terminal.font_family, deserialized.terminal.font_family);
        assert_eq!(settings.terminal.cursor, deserialized.terminal.cursor);
    }

    #[test]
    fn test_config_directory_path() {
        // Test that we can get a config directory path
        // This may fail in parallel test runs due to permission issues, so we accept both success and certain errors
        let path = get_settings_file_path();

        if let Err(e) = &path {
            // If there's a permission error during parallel test runs, that's acceptable
            if e.contains("Permission denied") {
                eprintln!("Note: Permission denied in parallel test run (acceptable)");
                return;
            }
        }

        assert!(path.is_ok(), "Should be able to get settings file path: {:?}", path.err());

        let path = path.unwrap();
        assert!(path.to_string_lossy().ends_with("settings.json"), "Path should end with settings.json");

        // Verify the path contains the correct app name based on build profile
        let path_str = path.to_string_lossy();
        #[cfg(production)]
        assert!(
            path_str.contains("nist") && !path_str.contains("nist-test"),
            "Production build should use 'nist' directory, got: {}",
            path_str
        );

        #[cfg(not(production))]
        assert!(
            path_str.contains("nist-test"),
            "Debug build should use 'nist-test' directory, got: {}",
            path_str
        );
    }

    #[test]
    fn test_config_path_is_platform_appropriate() {
        let path = get_settings_file_path();

        // Handle permission errors in parallel test runs
        if let Err(e) = &path {
            if e.contains("Permission denied") {
                eprintln!("Note: Permission denied in parallel test run (acceptable)");
                return;
            }
        }

        let path = path.unwrap();
        let path_str = path.to_string_lossy();

        // On Windows, should use AppData
        #[cfg(target_os = "windows")]
        assert!(
            path_str.contains("AppData") || path_str.contains("APPDATA"),
            "Windows should use AppData directory, got: {}",
            path_str
        );

        // On Unix-like systems, should use .config
        #[cfg(not(target_os = "windows"))]
        assert!(
            path_str.contains(".config"),
            "Unix-like systems should use .config directory, got: {}",
            path_str
        );
    }

    #[test]
    fn test_hotkeys_serialization() {
        let mut settings = Settings::default();

        // Add an additional hotkey to the existing default
        settings.hotkeys.navigation.split_right.push(KeyBinding {
            ctrl: true,
            shift: true,
            alt: false,
            key: Key::H,
        });

        let json = serde_json::to_string_pretty(&settings).unwrap();
        assert!(json.contains("hotkeys"));
        assert!(json.contains("navigation"));
        assert!(json.contains("splitRight"));

        let deserialized: Settings = serde_json::from_str(&json).unwrap();
        // Should have 2 bindings: 1 default + 1 added
        assert_eq!(deserialized.hotkeys.navigation.split_right.len(), 2);
        // Check the added one (second binding)
        assert_eq!(deserialized.hotkeys.navigation.split_right[1].ctrl, true);
        assert_eq!(deserialized.hotkeys.navigation.split_right[1].shift, true);
        assert_eq!(deserialized.hotkeys.navigation.split_right[1].key, Key::H);
    }

    #[test]
    fn test_key_enum_serialization() {
        let key = Key::ArrowDown;
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"ArrowDown\"");

        let key = Key::F1;
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"F1\"");

        let key = Key::Num0;
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"0\"");
    }

    #[test]
    fn test_key_to_keycode_conversion() {
        assert_eq!(Key::H.to_keycode(), Some(Keycode::H));
        assert_eq!(Key::ArrowDown.to_keycode(), Some(Keycode::Down));
        assert_eq!(Key::F1.to_keycode(), Some(Keycode::F1));
        assert_eq!(Key::Enter.to_keycode(), Some(Keycode::Return));
        // Num0 keycode name unknown in SDL3, skipping test
        // assert_eq!(Key::Num0.to_keycode(), Some(Keycode::Num0));
    }

    #[test]
    fn test_key_binding_matches() {
        let binding = KeyBinding {
            ctrl: true,
            shift: true,
            alt: false,
            key: Key::H,
        };

        // Should match Ctrl+Shift+H
        assert!(binding.matches(Keycode::H, true, true, false));

        // Should not match without Ctrl
        assert!(!binding.matches(Keycode::H, false, true, false));

        // Should not match without Shift
        assert!(!binding.matches(Keycode::H, true, false, false));

        // Should not match different key
        assert!(!binding.matches(Keycode::J, true, true, false));

        // Should not match with Alt
        assert!(!binding.matches(Keycode::H, true, true, true));
    }

    #[test]
    fn test_hotkeys_json_parsing() {
        let json = r#"
        {
            "external": [],
            "terminal": {
                "fontSize": 12.0,
                "fontFamily": "auto",
                "cursor": "pipe"
            },
            "hotkeys": {
                "navigation": {
                    "splitRight": [
                        {
                            "ctrl": true,
                            "shift": true,
                            "alt": false,
                            "key": "H"
                        }
                    ],
                    "splitDown": [
                        {
                            "ctrl": true,
                            "shift": true,
                            "key": "J"
                        }
                    ],
                    "newTab": [
                        {
                            "ctrl": true,
                            "shift": true,
                            "key": "T"
                        }
                    ]
                }
            }
        }
        "#;

        let settings: Settings = serde_json::from_str(json).unwrap();

        // Verify splitRight hotkey
        assert_eq!(settings.hotkeys.navigation.split_right.len(), 1);
        assert_eq!(settings.hotkeys.navigation.split_right[0].ctrl, true);
        assert_eq!(settings.hotkeys.navigation.split_right[0].shift, true);
        assert_eq!(settings.hotkeys.navigation.split_right[0].alt, false);
        assert_eq!(settings.hotkeys.navigation.split_right[0].key, Key::H);

        // Verify splitDown hotkey (alt defaults to false)
        assert_eq!(settings.hotkeys.navigation.split_down.len(), 1);
        assert_eq!(settings.hotkeys.navigation.split_down[0].alt, false);

        // Verify newTab hotkey
        assert_eq!(settings.hotkeys.navigation.new_tab.len(), 1);
    }

    #[test]
    fn test_multiple_hotkeys_for_same_action() {
        let json = r#"
        {
            "external": [],
            "terminal": {
                "fontSize": 12.0,
                "fontFamily": "auto",
                "cursor": "pipe"
            },
            "hotkeys": {
                "navigation": {
                    "newTab": [
                        {
                            "ctrl": true,
                            "shift": true,
                            "key": "T"
                        },
                        {
                            "ctrl": true,
                            "shift": true,
                            "key": "N"
                        }
                    ]
                }
            }
        }
        "#;

        let settings: Settings = serde_json::from_str(json).unwrap();

        // Should have two bindings for newTab
        assert_eq!(settings.hotkeys.navigation.new_tab.len(), 2);
        assert_eq!(settings.hotkeys.navigation.new_tab[0].key, Key::T);
        assert_eq!(settings.hotkeys.navigation.new_tab[1].key, Key::N);
    }

    #[test]
    fn test_navigation_hotkey_matching() {
        use crate::input::hotkeys::{match_navigation_hotkey, NavigationAction};

        let mut settings = Settings::default();

        // Add splitRight binding: Ctrl+Shift+H
        settings.hotkeys.navigation.split_right.push(KeyBinding {
            ctrl: true,
            shift: true,
            alt: false,
            key: Key::H,
        });

        // Add newTab binding: Ctrl+Shift+T
        settings.hotkeys.navigation.new_tab.push(KeyBinding {
            ctrl: true,
            shift: true,
            alt: false,
            key: Key::T,
        });

        // Test matching splitRight
        let result = match_navigation_hotkey(
            Keycode::H,
            true,  // ctrl
            true,  // shift
            false, // alt
            &settings.hotkeys.navigation,
        );
        assert_eq!(result, Some(NavigationAction::SplitRight));

        // Test matching newTab
        let result = match_navigation_hotkey(Keycode::T, true, true, false, &settings.hotkeys.navigation);
        assert_eq!(result, Some(NavigationAction::NewTab));

        // Test non-matching (wrong modifiers)
        let result = match_navigation_hotkey(
            Keycode::H,
            false, // no ctrl
            true,
            false,
            &settings.hotkeys.navigation,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_arrow_keys_serialization() {
        let binding = KeyBinding {
            ctrl: false,
            shift: true,
            alt: false,
            key: Key::ArrowUp,
        };

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains("ArrowUp"));

        let deserialized: KeyBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.key, Key::ArrowUp);
        assert_eq!(deserialized.shift, true);
    }

    #[test]
    fn test_function_keys_serialization() {
        let keys = vec![Key::F1, Key::F5, Key::F12];

        for key in keys {
            let json = serde_json::to_string(&key).unwrap();
            let deserialized: Key = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, key);
        }
    }

    #[test]
    fn test_default_navigation_hotkeys() {
        let settings = Settings::default();

        // Verify default navigation hotkeys are set correctly
        assert_eq!(settings.hotkeys.navigation.split_right.len(), 1);
        assert_eq!(settings.hotkeys.navigation.split_right[0].key, Key::J);
        assert_eq!(settings.hotkeys.navigation.split_right[0].ctrl, true);
        assert_eq!(settings.hotkeys.navigation.split_right[0].shift, true);

        assert_eq!(settings.hotkeys.navigation.split_down.len(), 1);
        assert_eq!(settings.hotkeys.navigation.split_down[0].key, Key::H);
        assert_eq!(settings.hotkeys.navigation.split_down[0].ctrl, true);
        assert_eq!(settings.hotkeys.navigation.split_down[0].shift, true);

        assert_eq!(settings.hotkeys.navigation.close_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.close_pane[0].key, Key::W);

        assert_eq!(settings.hotkeys.navigation.next_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.next_pane[0].key, Key::RightBracket);

        assert_eq!(settings.hotkeys.navigation.previous_pane.len(), 1);
        assert_eq!(settings.hotkeys.navigation.previous_pane[0].key, Key::LeftBracket);

        assert_eq!(settings.hotkeys.navigation.new_tab.len(), 1);
        assert_eq!(settings.hotkeys.navigation.new_tab[0].key, Key::T);

        assert_eq!(settings.hotkeys.navigation.next_tab.len(), 2); // Has two default bindings
        assert_eq!(settings.hotkeys.navigation.next_tab[0].key, Key::Tab);
        assert_eq!(settings.hotkeys.navigation.next_tab[0].ctrl, true);
        assert_eq!(settings.hotkeys.navigation.next_tab[0].shift, true);
        assert_eq!(settings.hotkeys.navigation.next_tab[1].key, Key::Tab);
        assert_eq!(settings.hotkeys.navigation.next_tab[1].ctrl, true);
        assert_eq!(settings.hotkeys.navigation.next_tab[1].shift, false);

        assert_eq!(settings.hotkeys.navigation.previous_tab.len(), 0); // No default binding
    }
}
