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

/// Match navigation hotkeys from settings
/// Returns a NavigationAction if the key combination matches a configured navigation hotkey
pub fn match_navigation_hotkey(
    keycode: Keycode,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    navigation_hotkeys: &NavigationHotkeys,
) -> Option<NavigationAction> {
    // Helper function to check if any binding matches
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

    None
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
