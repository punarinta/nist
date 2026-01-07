use sdl3::keyboard::{Keycode, Scancode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::hotkeys::HotkeyAction;
use crate::pane_layout::SplitDirection;
use crate::sdl_renderer::TabBar;
use crate::tab_gui::TabBarGui;

#[cfg(target_os = "linux")]
use arboard::Clipboard;
#[cfg(not(target_os = "linux"))]
use arboard::Clipboard;

#[cfg(target_os = "linux")]
use std::sync::mpsc::Sender;

/// Actions that keyboard handler can request from the main loop
#[derive(Debug, Clone)]
pub enum KeyboardAction {
    NewTab,
    SplitPane(SplitDirection),
    RequestQuitConfirmation,
    Quit,
    None,
}

/// Result of handling a keyboard event
pub struct KeyboardResult {
    pub action: KeyboardAction,
    pub needs_render: bool,
    pub needs_resize: bool,
}

impl KeyboardResult {
    pub fn none() -> Self {
        Self {
            action: KeyboardAction::None,
            needs_render: false,
            needs_resize: false,
        }
    }

    pub fn with_action(action: KeyboardAction) -> Self {
        Self {
            action,
            needs_render: true,
            needs_resize: false,
        }
    }

    pub fn render() -> Self {
        Self {
            action: KeyboardAction::None,
            needs_render: true,
            needs_resize: false,
        }
    }

    pub fn with_resize(action: KeyboardAction) -> Self {
        Self {
            action,
            needs_render: true,
            needs_resize: true,
        }
    }
}

/// Initialize the Ctrl+key mapping table
pub fn create_ctrl_key_map() -> HashMap<Scancode, u8> {
    [
        (Scancode::A, 1),
        (Scancode::B, 2),
        (Scancode::C, 3),
        (Scancode::D, 4),
        (Scancode::E, 5),
        (Scancode::F, 6),
        (Scancode::G, 7),
        (Scancode::H, 8),
        (Scancode::I, 9),
        (Scancode::J, 10),
        (Scancode::K, 11),
        (Scancode::L, 12),
        (Scancode::M, 13),
        (Scancode::N, 14),
        (Scancode::O, 15),
        (Scancode::P, 16),
        (Scancode::Q, 17),
        (Scancode::R, 18),
        (Scancode::S, 19),
        (Scancode::T, 20),
        (Scancode::U, 21),
        (Scancode::V, 22),
        (Scancode::W, 23),
        (Scancode::X, 24),
        (Scancode::Y, 25),
        (Scancode::Z, 26),
    ]
    .iter()
    .cloned()
    .collect()
}

/// Handle keyboard events for tab editing mode
/// Note: Caller should handle text_input().stop() when editing is finished (check tab_bar.editing_tab)
pub fn handle_tab_editing_key(keycode: Keycode, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> KeyboardResult {
    match keycode {
        Keycode::Return => {
            // Save and finish editing
            if let Some(idx) = tab_bar.editing_tab {
                let mut gui = tab_bar_gui.lock().unwrap();
                gui.tab_states[idx].temp_name = tab_bar.edit_text.clone();
                gui.tab_states[idx].finish_editing(true);
            }
            tab_bar.finish_editing(true);
            KeyboardResult::render()
        }
        Keycode::Escape => {
            // Cancel editing
            if let Some(idx) = tab_bar.editing_tab {
                let mut gui = tab_bar_gui.lock().unwrap();
                gui.tab_states[idx].finish_editing(false);
            }
            tab_bar.finish_editing(false);
            KeyboardResult::render()
        }
        Keycode::Backspace => {
            // Remove last character
            tab_bar.edit_text.pop();
            KeyboardResult::render()
        }
        _ => {
            // Ignore other keys during editing
            KeyboardResult::none()
        }
    }
}

/// Handle hotkey actions
#[allow(clippy::too_many_arguments)]
pub fn handle_hotkey_action(
    action: HotkeyAction,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    scale_factor: f32,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> KeyboardResult {
    match action {
        HotkeyAction::NewTab => KeyboardResult::with_action(KeyboardAction::NewTab),

        HotkeyAction::SplitHorizontal => KeyboardResult::with_action(KeyboardAction::SplitPane(SplitDirection::Horizontal)),

        HotkeyAction::SplitVertical => KeyboardResult::with_action(KeyboardAction::SplitPane(SplitDirection::Vertical)),

        HotkeyAction::NextTab => {
            tab_bar_gui.lock().unwrap().cycle_to_next_tab();
            KeyboardResult::render()
        }

        HotkeyAction::ClosePane => {
            let mut gui = tab_bar_gui.lock().unwrap();

            // Check if this is the last pane in the last tab
            let is_last_pane_in_last_tab = gui.tab_states.len() == 1 && gui.get_active_pane_layout().map(|pl| pl.root.count_leaf_panes()).unwrap_or(0) == 1;

            if is_last_pane_in_last_tab {
                // Request confirmation before closing
                return KeyboardResult::with_action(KeyboardAction::RequestQuitConfirmation);
            }

            if let Some(pane_layout) = gui.get_active_pane_layout() {
                let active_pane = pane_layout.active_pane();
                if pane_layout.close_pane(active_pane) {
                    // Last pane in tab closed
                    drop(gui);
                    let active_tab = tab_bar_gui.lock().unwrap().active_tab;
                    if tab_bar_gui.lock().unwrap().remove_tab(active_tab) {
                        return KeyboardResult::with_action(KeyboardAction::Quit);
                    }
                } else {
                    // Pane closed, need to resize remaining terminals
                    drop(gui);
                    return KeyboardResult::with_resize(KeyboardAction::None);
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::PreviousPane => {
            // Ctrl+[ - Navigate to previous pane, or previous tab if at first pane
            let mut gui = tab_bar_gui.lock().unwrap();
            if let Some(pane_layout) = gui.get_active_pane_layout() {
                if pane_layout.is_first_pane() {
                    // At first pane, go to previous tab
                    drop(gui);
                    tab_bar_gui.lock().unwrap().cycle_to_previous_tab();
                } else {
                    // Not at first pane, go to previous pane
                    pane_layout.cycle_to_previous_pane();
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::NextPane => {
            // Ctrl+] - Navigate to next pane, or next tab if at last pane
            let mut gui = tab_bar_gui.lock().unwrap();
            if let Some(pane_layout) = gui.get_active_pane_layout() {
                if pane_layout.is_last_pane() {
                    // At last pane, go to next tab
                    drop(gui);
                    tab_bar_gui.lock().unwrap().cycle_to_next_tab();
                } else {
                    // Not at last pane, go to next pane
                    pane_layout.cycle_to_next_pane();
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::Copy => {
            // Ctrl+Shift+C: Copy selection to clipboard
            handle_copy(
                tab_bar_gui,
                #[cfg(target_os = "linux")]
                clipboard_tx,
            );
            KeyboardResult::render()
        }

        HotkeyAction::Paste => {
            // Ctrl+Shift+V: Paste from clipboard
            handle_paste(tab_bar_gui);
            KeyboardResult::render()
        }

        HotkeyAction::CopySelection => {
            // Ctrl+C: Copy selection to clipboard (only if we have a selection)
            // If there's no selection, we'll return None to let Ctrl+C pass through
            let copied = handle_copy_selection(
                tab_bar_gui,
                scale_factor,
                char_width,
                char_height,
                tab_bar_height,
                canvas_window,
                #[cfg(target_os = "linux")]
                clipboard_tx,
            );
            if copied {
                KeyboardResult::render()
            } else {
                // No selection, so don't consume the event - let Ctrl+C pass through to terminal
                KeyboardResult::none()
            }
        }

        HotkeyAction::PasteQuick => {
            // Ctrl+V: Paste from clipboard (only when terminal is idle)
            // Check if mouse tracking is disabled - this indicates terminal is idle
            let should_paste = {
                if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
                    if let Ok(t) = terminal.lock() {
                        let mouse_tracking = *t.mouse_tracking_mode.lock().unwrap();
                        mouse_tracking == crate::terminal::MouseTrackingMode::Disabled
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if should_paste {
                handle_paste(tab_bar_gui);
                KeyboardResult::render()
            } else {
                // Mouse tracking is enabled (app is running), let Ctrl+V pass through
                KeyboardResult::none()
            }
        }

        HotkeyAction::ScrollPageUp => {
            if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
                if let Ok(t) = terminal.lock() {
                    let height = t.height as usize;
                    t.screen_buffer.lock().unwrap().scroll_view_up(height);
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::ScrollPageDown => {
            if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
                if let Ok(t) = terminal.lock() {
                    let height = t.height as usize;
                    t.screen_buffer.lock().unwrap().scroll_view_down(height);
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::ScrollLineUp => {
            if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
                if let Ok(t) = terminal.lock() {
                    t.screen_buffer.lock().unwrap().scroll_view_up(1);
                }
            }
            KeyboardResult::render()
        }

        HotkeyAction::ScrollLineDown => {
            if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
                if let Ok(t) = terminal.lock() {
                    t.screen_buffer.lock().unwrap().scroll_view_down(1);
                }
            }
            KeyboardResult::render()
        }
    }
}

/// Handle Ctrl+Shift+C: Copy selection to clipboard
fn handle_copy(tab_bar_gui: &Arc<Mutex<TabBarGui>>, #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>) {
    if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
        let t = terminal.lock().unwrap();
        if let Some(text) = t.get_selected_text() {
            if !text.is_empty() {
                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if let Err(e) = clipboard.set_text(text.clone()) {
                            eprintln!("[CLIPBOARD] Failed to copy: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("[CLIPBOARD] Failed to create clipboard: {}", e);
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    use arboard::{LinuxClipboardKind, SetExtLinux};
                    let text_copy = text.clone();
                    let tx = clipboard_tx.clone();

                    // Create clipboard in background thread to avoid blocking
                    std::thread::spawn(move || {
                        match Clipboard::new() {
                            Ok(mut clipboard) => {
                                if let Err(e) = clipboard.set().clipboard(LinuxClipboardKind::Primary).text(text_copy) {
                                    eprintln!("[PRIMARY] Failed to copy to primary selection: {}", e);
                                } else {
                                    // Send clipboard object back to main thread
                                    let _ = tx.send(clipboard);
                                }
                            }
                            Err(e) => {
                                eprintln!("[PRIMARY] Failed to create clipboard: {}", e);
                            }
                        }
                    });
                }
            }
        }
    }
}

/// Handle Ctrl+Shift+V: Paste from clipboard
fn handle_paste(tab_bar_gui: &Arc<Mutex<TabBarGui>>) {
    if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
        match Clipboard::new() {
            Ok(mut clipboard) => match clipboard.get_text() {
                Ok(text) => {
                    terminal.lock().unwrap().send_paste(&text);
                }
                Err(e) => {
                    eprintln!("[CLIPBOARD] Failed to get text: {}", e);
                }
            },
            Err(e) => {
                eprintln!("[CLIPBOARD] Failed to create clipboard: {}", e);
            }
        }
    }
}

/// Handle Ctrl+C: Copy selection with animation
#[allow(clippy::too_many_arguments)]
fn handle_copy_selection(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    _scale_factor: f32,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> bool {
    use sdl3::rect::Rect;

    let mut gui = tab_bar_gui.lock().unwrap();
    if let Some(terminal) = gui.get_active_terminal() {
        let t = terminal.lock().unwrap();
        if let Some(text) = t.get_selected_text() {
            if !text.is_empty() {
                // Calculate selection rectangle for animation before clearing
                let selection_rect = if let Some(sel) = *t.selection.lock().unwrap() {
                    // Get active pane rect
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        let (window_w, window_h) = canvas_window.size();
                        let pane_area_y = tab_bar_height as i32;
                        let pane_area_height = window_h - tab_bar_height;
                        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_w, pane_area_height);

                        // Find the active pane rect
                        pane_rects
                            .iter()
                            .find(|(_, _, term, is_active)| *is_active && Arc::ptr_eq(term, &terminal))
                            .map(|(_, rect, _, _)| {
                                // Calculate selection bounds in screen coordinates
                                let (start_col, start_row, end_col, end_row) = sel.normalized();

                                let x = rect.x() + (start_col as f32 * char_width) as i32;
                                let y = rect.y() + (start_row as f32 * char_height) as i32;
                                let width = ((end_col - start_col + 1) as f32 * char_width) as u32;
                                let height = ((end_row - start_row + 1) as f32 * char_height) as u32;

                                Rect::new(x, y, width, height)
                            })
                    } else {
                        None
                    }
                } else {
                    None
                };

                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if let Err(e) = clipboard.set_text(text.clone()) {
                            eprintln!("[CLIPBOARD] Failed to copy: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("[CLIPBOARD] Failed to create clipboard: {}", e);
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    use arboard::{LinuxClipboardKind, SetExtLinux};
                    let text_copy = text.clone();
                    let tx = clipboard_tx.clone();

                    // Create clipboard in background thread to avoid blocking
                    std::thread::spawn(move || {
                        match Clipboard::new() {
                            Ok(mut clipboard) => {
                                if let Err(e) = clipboard.set().clipboard(LinuxClipboardKind::Primary).text(text_copy) {
                                    eprintln!("[PRIMARY] Failed to copy to primary selection: {}", e);
                                } else {
                                    // Send clipboard object back to main thread
                                    let _ = tx.send(clipboard);
                                }
                            }
                            Err(e) => {
                                eprintln!("[PRIMARY] Failed to create clipboard: {}", e);
                            }
                        }
                    });
                }

                // Clear selection
                *t.selection.lock().unwrap() = None;

                // Start copy animation
                if let (Some(rect), Some(pane_layout)) = (selection_rect, gui.get_active_pane_layout()) {
                    pane_layout.copy_animation = Some(crate::ui::animations::CopyAnimation::new(rect));
                }
                return true;
            }
        }
    }
    false
}

/// Handle normal key presses (arrow keys, function keys, etc.)
pub fn handle_normal_key(keycode: Keycode, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> KeyboardResult {
    if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
        let mut t = terminal.lock().unwrap();
        let backspace_key = t.shell_config.keys.backspace.clone();

        // Check if application cursor keys mode is enabled
        let app_cursor_mode = *t.application_cursor_keys.lock().unwrap();

        match keycode {
            Keycode::Return => t.send_key(b"\r"),
            Keycode::Backspace => t.send_key(&backspace_key),
            Keycode::Tab => t.send_key(b"\t"),
            Keycode::Escape => t.send_key(b"\x1b"),
            Keycode::Up => {
                eprintln!("[KEYBOARD] Sending Up arrow (app_cursor_mode: {})", app_cursor_mode);
                if app_cursor_mode {
                    t.send_key(b"\x1bOA")
                } else {
                    t.send_key(b"\x1b[A")
                }
            }
            Keycode::Down => {
                eprintln!("[KEYBOARD] Sending Down arrow (app_cursor_mode: {})", app_cursor_mode);
                if app_cursor_mode {
                    t.send_key(b"\x1bOB")
                } else {
                    t.send_key(b"\x1b[B")
                }
            }
            Keycode::Right => {
                eprintln!("[KEYBOARD] Sending Right arrow (app_cursor_mode: {})", app_cursor_mode);
                if app_cursor_mode {
                    t.send_key(b"\x1bOC")
                } else {
                    t.send_key(b"\x1b[C")
                }
            }
            Keycode::Left => {
                eprintln!("[KEYBOARD] Sending Left arrow (app_cursor_mode: {})", app_cursor_mode);
                if app_cursor_mode {
                    t.send_key(b"\x1bOD")
                } else {
                    t.send_key(b"\x1b[D")
                }
            }
            Keycode::Home => t.send_key(b"\x1b[H"),
            Keycode::End => t.send_key(b"\x1b[F"),
            Keycode::PageUp => t.send_key(b"\x1b[5~"),
            Keycode::PageDown => t.send_key(b"\x1b[6~"),
            Keycode::Insert => t.send_key(b"\x1b[2~"),
            Keycode::Delete => t.send_key(b"\x1b[3~"),
            Keycode::F1 => t.send_key(b"\x1bOP"),
            Keycode::F2 => t.send_key(b"\x1bOQ"),
            Keycode::F3 => t.send_key(b"\x1bOR"),
            Keycode::F4 => t.send_key(b"\x1bOS"),
            Keycode::F5 => t.send_key(b"\x1b[15~"),
            Keycode::F6 => t.send_key(b"\x1b[17~"),
            Keycode::F7 => t.send_key(b"\x1b[18~"),
            Keycode::F8 => t.send_key(b"\x1b[19~"),
            Keycode::F9 => t.send_key(b"\x1b[20~"),
            Keycode::F10 => t.send_key(b"\x1b[21~"),
            Keycode::F11 => t.send_key(b"\x1b[23~"),
            Keycode::F12 => t.send_key(b"\x1b[24~"),
            _ => {}
        }
    }
    // Request render after sending key to terminal so visual feedback is immediate
    KeyboardResult::render()
}

/// Handle Ctrl+key combinations for control characters
pub fn handle_ctrl_key(scancode: Scancode, ctrl_keys: &HashMap<Scancode, u8>, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> KeyboardResult {
    if let Some(&ctrl_byte) = ctrl_keys.get(&scancode) {
        if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
            terminal.lock().unwrap().send_key(&[ctrl_byte]);
        }
        return KeyboardResult::render();
    }
    KeyboardResult::none()
}

/// Handle text input events
pub fn handle_text_input(text: &str, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> KeyboardResult {
    if tab_bar.editing_tab.is_some() {
        tab_bar.edit_text.push_str(text);
        KeyboardResult::render()
    } else if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
        terminal.lock().unwrap().send_text(text);
        // Request render after sending text to terminal so visual feedback is immediate
        KeyboardResult::render()
    } else {
        KeyboardResult::none()
    }
}
