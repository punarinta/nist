use sdl2::keyboard::Keycode;

/// Represents actions that can be triggered by hotkeys
#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyAction {
    // Tab management
    NewTab,
    NextTab,

    // Pane management
    ClosePane,
    SplitHorizontal,
    SplitVertical,
    PreviousPane,
    NextPane,

    // Clipboard operations
    Copy,
    CopySelection, // Ctrl+C that only copies if there's a selection
    Paste,

    // Scrollback navigation
    ScrollPageUp,
    ScrollPageDown,
    ScrollLineUp,
    ScrollLineDown,
}

/// Match a keycode and modifiers to a hotkey action
/// Returns None if the key combination doesn't match any hotkey
pub fn match_hotkey(keycode: Keycode, is_ctrl: bool, is_shift: bool) -> Option<HotkeyAction> {
    if is_ctrl && is_shift {
        // Ctrl+Shift combinations
        match keycode {
            Keycode::T => Some(HotkeyAction::NewTab),
            Keycode::H => Some(HotkeyAction::SplitHorizontal),
            Keycode::J => Some(HotkeyAction::SplitVertical),
            Keycode::W => Some(HotkeyAction::ClosePane),
            Keycode::Tab => Some(HotkeyAction::NextTab),
            Keycode::C => Some(HotkeyAction::Copy),
            Keycode::V => Some(HotkeyAction::Paste),
            _ => None,
        }
    } else if is_ctrl && !is_shift {
        // Ctrl combinations (without Shift)
        match keycode {
            Keycode::Tab => Some(HotkeyAction::NextTab),
            Keycode::LeftBracket => Some(HotkeyAction::PreviousPane),
            Keycode::RightBracket => Some(HotkeyAction::NextPane),
            Keycode::C => Some(HotkeyAction::CopySelection), // Special: only copies if selection exists
            _ => None,
        }
    } else if is_shift && !is_ctrl {
        // Shift combinations (without Ctrl)
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
pub fn get_modifiers(keymod: sdl2::keyboard::Mod) -> (bool, bool) {
    let is_ctrl = keymod.contains(sdl2::keyboard::Mod::LCTRLMOD) || keymod.contains(sdl2::keyboard::Mod::RCTRLMOD);
    let is_shift = keymod.contains(sdl2::keyboard::Mod::LSHIFTMOD) || keymod.contains(sdl2::keyboard::Mod::RSHIFTMOD);
    (is_ctrl, is_shift)
}
