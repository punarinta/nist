use crate::settings::{KeyBinding, NavigationHotkeys};
use sdl3::keyboard::Keycode;
use std::time::{Duration, Instant};

/// Sequential hotkey state tracker
/// Tracks the first key press in a sequential hotkey combination
#[derive(Debug, Clone)]
pub struct SequentialHotkeyState {
    pub first_key: Option<(Keycode, bool, bool, bool)>, // (keycode, ctrl, shift, alt)
    pub timestamp: Instant,
}

impl SequentialHotkeyState {
    pub fn new() -> Self {
        Self {
            first_key: None,
            timestamp: Instant::now(),
        }
    }

    /// Check if the state is valid (within timeout window)
    pub fn is_valid(&self) -> bool {
        self.first_key.is_some() && self.timestamp.elapsed() < Duration::from_secs(2)
    }

    /// Record a first key press
    pub fn record_first_key(&mut self, keycode: Keycode, is_ctrl: bool, is_shift: bool, is_alt: bool) {
        self.first_key = Some((keycode, is_ctrl, is_shift, is_alt));
        self.timestamp = Instant::now();
    }

    /// Clear the state
    pub fn clear(&mut self) {
        self.first_key = None;
    }

    /// Get the recorded first key if valid
    pub fn get_first_key(&self) -> Option<(Keycode, bool, bool, bool)> {
        if self.is_valid() {
            self.first_key
        } else {
            None
        }
    }
}

/// Navigation actions that can be configured in settings
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationAction {
    SplitRight,
    SplitDown,
    ClosePane,
    NextPane,
    PreviousPane,
    NewTab,
    NextTab,
    PreviousTab,
    GoToPrompt,
}

/// Represents actions that can be triggered by hotkeys
#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyAction {
    // Navigation actions (configurable via settings)
    Navigation(NavigationAction),

    // Clipboard operations
    Copy,
    CopySelection, // Ctrl+C that only copies if there's a selection
    Paste,
    PasteQuick, // Ctrl+V paste for idle terminal (no shift)

    // Scrollback navigation
    ScrollPageUp,
    ScrollPageDown,
    ScrollLineUp,
    ScrollLineDown,
    GoToPrompt, // Scroll to the prompt (reset scroll position)
}

/// Match navigation hotkeys from settings (single-key only)
/// Returns a NavigationAction if the key combination matches a configured navigation hotkey
pub fn match_navigation_hotkey(
    keycode: Keycode,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    navigation_hotkeys: &NavigationHotkeys,
) -> Option<NavigationAction> {
    // Helper function to check if any binding matches (single-key bindings only)
    // Note: binding.matches() already filters out sequential bindings
    let matches_any = |bindings: &[KeyBinding]| -> bool { bindings.iter().any(|binding| binding.matches(keycode, is_ctrl, is_shift, is_alt)) };

    if matches_any(&navigation_hotkeys.split_right) {
        return Some(NavigationAction::SplitRight);
    }
    if matches_any(&navigation_hotkeys.split_down) {
        return Some(NavigationAction::SplitDown);
    }
    if matches_any(&navigation_hotkeys.close_pane) {
        return Some(NavigationAction::ClosePane);
    }
    if matches_any(&navigation_hotkeys.next_pane) {
        return Some(NavigationAction::NextPane);
    }
    if matches_any(&navigation_hotkeys.previous_pane) {
        return Some(NavigationAction::PreviousPane);
    }
    if matches_any(&navigation_hotkeys.new_tab) {
        return Some(NavigationAction::NewTab);
    }
    if matches_any(&navigation_hotkeys.next_tab) {
        return Some(NavigationAction::NextTab);
    }
    if matches_any(&navigation_hotkeys.previous_tab) {
        return Some(NavigationAction::PreviousTab);
    }
    if matches_any(&navigation_hotkeys.go_to_prompt) {
        return Some(NavigationAction::GoToPrompt);
    }

    None
}

/// Match sequential navigation hotkeys from settings
/// Returns a NavigationAction if the current key completes a sequential navigation hotkey
pub fn match_sequential_navigation_hotkey(
    keycode: Keycode,
    sequential_state: &SequentialHotkeyState,
    navigation_hotkeys: &NavigationHotkeys,
) -> Option<NavigationAction> {
    // Get the first key from the sequential state
    let (first_keycode, first_ctrl, first_shift, first_alt) = sequential_state.get_first_key()?;

    // Helper function to check if any sequential binding matches
    let matches_any_sequential = |bindings: &[KeyBinding]| -> bool {
        bindings
            .iter()
            .any(|binding| binding.is_sequential() && binding.matches_sequence(first_keycode, first_ctrl, first_shift, first_alt, keycode))
    };

    if matches_any_sequential(&navigation_hotkeys.split_right) {
        return Some(NavigationAction::SplitRight);
    }
    if matches_any_sequential(&navigation_hotkeys.split_down) {
        return Some(NavigationAction::SplitDown);
    }
    if matches_any_sequential(&navigation_hotkeys.close_pane) {
        return Some(NavigationAction::ClosePane);
    }
    if matches_any_sequential(&navigation_hotkeys.next_pane) {
        return Some(NavigationAction::NextPane);
    }
    if matches_any_sequential(&navigation_hotkeys.previous_pane) {
        return Some(NavigationAction::PreviousPane);
    }
    if matches_any_sequential(&navigation_hotkeys.new_tab) {
        return Some(NavigationAction::NewTab);
    }
    if matches_any_sequential(&navigation_hotkeys.next_tab) {
        return Some(NavigationAction::NextTab);
    }
    if matches_any_sequential(&navigation_hotkeys.previous_tab) {
        return Some(NavigationAction::PreviousTab);
    }
    if matches_any_sequential(&navigation_hotkeys.go_to_prompt) {
        return Some(NavigationAction::GoToPrompt);
    }

    None
}

/// Check if a key press should start a sequential navigation hotkey from settings
/// Returns true if this key combination is the first part of any configured sequential navigation hotkey
pub fn is_sequential_navigation_hotkey_start(keycode: Keycode, is_ctrl: bool, is_shift: bool, is_alt: bool, navigation_hotkeys: &NavigationHotkeys) -> bool {
    // Helper function to check if any binding starts with this key
    let starts_with = |bindings: &[KeyBinding]| -> bool {
        bindings
            .iter()
            .any(|binding| binding.is_sequential() && binding.matches_first_key(keycode, is_ctrl, is_shift, is_alt))
    };

    starts_with(&navigation_hotkeys.split_right)
        || starts_with(&navigation_hotkeys.split_down)
        || starts_with(&navigation_hotkeys.close_pane)
        || starts_with(&navigation_hotkeys.next_pane)
        || starts_with(&navigation_hotkeys.previous_pane)
        || starts_with(&navigation_hotkeys.new_tab)
        || starts_with(&navigation_hotkeys.next_tab)
        || starts_with(&navigation_hotkeys.previous_tab)
        || starts_with(&navigation_hotkeys.go_to_prompt)
}

/// Match a keycode and modifiers to a hotkey action (hardcoded hotkeys)
/// Returns None if the key combination doesn't match any hotkey
/// Only handles clipboard and scrollback operations now - navigation is handled by settings
pub fn match_hotkey(keycode: Keycode, is_ctrl: bool, is_shift: bool) -> Option<HotkeyAction> {
    if is_ctrl && is_shift {
        // Ctrl+Shift combinations (clipboard operations)
        match keycode {
            Keycode::C => Some(HotkeyAction::Copy),
            Keycode::V => Some(HotkeyAction::Paste),
            _ => None,
        }
    } else if is_ctrl && !is_shift {
        // Ctrl combinations (clipboard operations)
        match keycode {
            Keycode::C => Some(HotkeyAction::CopySelection), // Special: only copies if selection exists
            Keycode::V => Some(HotkeyAction::PasteQuick),    // Special: only pastes if terminal is idle
            _ => None,
        }
    } else if is_shift && !is_ctrl {
        // Shift combinations (scrollback navigation)
        match keycode {
            Keycode::PageUp => Some(HotkeyAction::ScrollPageUp),
            Keycode::PageDown => Some(HotkeyAction::ScrollPageDown),
            Keycode::Up => Some(HotkeyAction::ScrollLineUp),
            Keycode::Down => Some(HotkeyAction::ScrollLineDown),
            _ => None,
        }
    } else {
        None
    }
}

/// Extract modifier flags from SDL keymod
pub fn get_modifiers(keymod: sdl3::keyboard::Mod) -> (bool, bool, bool) {
    let is_ctrl = keymod.contains(sdl3::keyboard::Mod::LCTRLMOD) || keymod.contains(sdl3::keyboard::Mod::RCTRLMOD);
    let is_shift = keymod.contains(sdl3::keyboard::Mod::LSHIFTMOD) || keymod.contains(sdl3::keyboard::Mod::RSHIFTMOD);
    let is_alt = keymod.contains(sdl3::keyboard::Mod::LALTMOD) || keymod.contains(sdl3::keyboard::Mod::RALTMOD);
    (is_ctrl, is_shift, is_alt)
}

/// Match sequential hotkey combinations (e.g., Alt-G-P)
/// Returns Some(HotkeyAction) if the current key completes a sequential hotkey,
/// or returns None if it doesn't match any known sequential pattern.
///
/// Sequential hotkeys work as follows:
/// 1. User presses modifiers (e.g., Alt) + first key (e.g., G)
/// 2. User presses second key (e.g., P) - modifiers may be held or released
///
/// This function checks hardcoded sequential hotkey patterns.
pub fn match_sequential_hotkey(
    keycode: Keycode,
    _is_ctrl: bool,
    _is_shift: bool,
    _is_alt: bool,
    sequential_state: &SequentialHotkeyState,
) -> Option<HotkeyAction> {
    // Get the first key from the sequential state
    let (first_keycode, _first_ctrl, _first_shift, first_alt) = sequential_state.get_first_key()?;

    // Match hardcoded sequential patterns
    // Alt-G-P: Go to prompt (scroll to bottom, reset scroll position)
    if first_alt && first_keycode == Keycode::G && keycode == Keycode::P {
        return Some(HotkeyAction::GoToPrompt);
    }

    None
}

/// Check if a key press should start a sequential hotkey
/// Returns true if this key combination is the first part of a known sequential hotkey
pub fn is_sequential_hotkey_start(keycode: Keycode, is_ctrl: bool, is_shift: bool, is_alt: bool) -> bool {
    // Alt-G is the start of Alt-G-P (go to prompt)
    if is_alt && !is_ctrl && !is_shift && keycode == Keycode::G {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Key, KeyBinding, NavigationHotkeys};

    #[test]
    fn test_sequential_navigation_hotkeys_from_settings() {
        // Create a NavigationHotkeys config with sequential hotkeys
        let mut nav_hotkeys = NavigationHotkeys::default();

        // Add a sequential hotkey: Alt+G -> N for nextPane
        nav_hotkeys.next_pane.push(KeyBinding {
            ctrl: false,
            shift: false,
            alt: true,
            key: Key::G,
            key2: Some(Key::N),
        });

        // Add a sequential hotkey: Alt+G -> P for previousPane
        nav_hotkeys.previous_pane.push(KeyBinding {
            ctrl: false,
            shift: false,
            alt: true,
            key: Key::G,
            key2: Some(Key::P),
        });

        // Test that Alt+G is recognized as a sequential hotkey start
        assert!(is_sequential_navigation_hotkey_start(Keycode::G, false, false, true, &nav_hotkeys));

        // Test that other keys are not recognized as sequential starts
        assert!(!is_sequential_navigation_hotkey_start(Keycode::X, false, false, true, &nav_hotkeys));

        // Create sequential state and record the first key (Alt+G)
        let mut seq_state = SequentialHotkeyState::new();
        seq_state.record_first_key(Keycode::G, false, false, true);

        // Test that pressing N completes the nextPane sequential hotkey
        let result = match_sequential_navigation_hotkey(Keycode::N, &seq_state, &nav_hotkeys);
        assert_eq!(result, Some(NavigationAction::NextPane));

        // Reset and test previousPane
        seq_state.record_first_key(Keycode::G, false, false, true);
        let result = match_sequential_navigation_hotkey(Keycode::P, &seq_state, &nav_hotkeys);
        assert_eq!(result, Some(NavigationAction::PreviousPane));

        // Test that wrong second key doesn't match
        seq_state.record_first_key(Keycode::G, false, false, true);
        let result = match_sequential_navigation_hotkey(Keycode::X, &seq_state, &nav_hotkeys);
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_key_navigation_hotkeys_still_work() {
        let nav_hotkeys = NavigationHotkeys::default();

        // Default single-key hotkeys should still work
        let result = match_navigation_hotkey(Keycode::RightBracket, true, false, false, &nav_hotkeys);
        assert_eq!(result, Some(NavigationAction::NextPane));

        // Sequential hotkeys should NOT match in single-key matching
        let mut nav_hotkeys_with_seq = NavigationHotkeys::default();
        nav_hotkeys_with_seq.next_pane.push(KeyBinding {
            ctrl: false,
            shift: false,
            alt: true,
            key: Key::G,
            key2: Some(Key::N),
        });

        // Alt+G should not match as a single-key binding for nextPane
        let result = match_navigation_hotkey(Keycode::G, false, false, true, &nav_hotkeys_with_seq);
        assert_eq!(result, None);
    }

    #[test]
    fn test_go_to_prompt_sequential_hotkey() {
        // Test the default GoToPrompt configuration (Alt+G -> P)
        let nav_hotkeys = NavigationHotkeys::default();

        // Verify goToPrompt has default binding
        assert_eq!(nav_hotkeys.go_to_prompt.len(), 1);
        assert!(nav_hotkeys.go_to_prompt[0].is_sequential());
        assert_eq!(nav_hotkeys.go_to_prompt[0].alt, true);
        assert_eq!(nav_hotkeys.go_to_prompt[0].ctrl, false);
        assert_eq!(nav_hotkeys.go_to_prompt[0].shift, false);

        // Test that Alt+G is recognized as a sequential hotkey start
        assert!(is_sequential_navigation_hotkey_start(Keycode::G, false, false, true, &nav_hotkeys));

        // Create sequential state and record the first key (Alt+G)
        let mut seq_state = SequentialHotkeyState::new();
        seq_state.record_first_key(Keycode::G, false, false, true);

        // Test that pressing P completes the goToPrompt sequential hotkey
        let result = match_sequential_navigation_hotkey(Keycode::P, &seq_state, &nav_hotkeys);
        assert_eq!(result, Some(NavigationAction::GoToPrompt));

        // Test that Alt+G should not match as a single-key binding
        let result = match_navigation_hotkey(Keycode::G, false, false, true, &nav_hotkeys);
        assert_eq!(result, None);
    }
}
