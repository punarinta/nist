use sdl3::mouse::MouseButton;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use arboard::Clipboard;
#[cfg(not(target_os = "linux"))]
use arboard::Clipboard;

#[cfg(target_os = "linux")]
use std::sync::mpsc::Sender;

use crate::sdl_renderer::TabBar;
use crate::tab_gui::TabBarGui;

/// Actions that mouse handler can request from the main loop
#[derive(Debug, Clone)]
pub enum MouseAction {
    NewTab,
    CloseWindow,
    MinimizeWindow,
    CloseTab(usize),
    SwitchTab(usize),
    OpenSettings,
    None,
}

/// Result of handling a mouse event
pub struct MouseResult {
    pub action: MouseAction,
    pub needs_render: bool,
}

impl MouseResult {
    pub fn none() -> Self {
        Self {
            action: MouseAction::None,
            needs_render: false,
        }
    }

    pub fn with_action(action: MouseAction) -> Self {
        Self { action, needs_render: true }
    }

    pub fn render() -> Self {
        Self {
            action: MouseAction::None,
            needs_render: true,
        }
    }

    pub fn with_divider_drag() -> Self {
        Self {
            action: MouseAction::None,
            needs_render: true,
        }
    }
}

/// Mouse state tracker
pub struct MouseState {
    pub dragging_divider: bool,
    pub last_mouse_pos: (i32, i32),
    pub drag_motion_counter: u32,
    pub mouse_down_for_selection: bool,
    pub selection_start_pos: (i32, i32),
    pub selection_started: bool,
    pub dragging_tab: bool,
    pub tab_drag_start_pos: (i32, i32),
    pub ready_to_drag_tab: bool,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            dragging_divider: false,
            last_mouse_pos: (0, 0),
            drag_motion_counter: 0,
            mouse_down_for_selection: false,
            selection_start_pos: (0, 0),
            selection_started: false,
            dragging_tab: false,
            tab_drag_start_pos: (0, 0),
            ready_to_drag_tab: false,
        }
    }
}

/// Send mouse event to terminal
pub fn send_mouse_to_terminal(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_x: i32,
    mouse_y: i32,
    button: u8,
    pressed: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    window_width: u32,
    window_height: u32,
) {
    let mut gui = match tab_bar_gui.try_lock() {
        Ok(g) => g,
        Err(_) => return, // Skip this mouse event if lock is busy
    };
    if let Some(pane_layout) = gui.get_active_pane_layout() {
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;
        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

        // Find which pane contains the mouse
        for (_pane_id, rect, terminal, _is_active) in pane_rects {
            if rect.contains_point((mouse_x, mouse_y)) {
                // Convert screen coordinates to terminal coordinates (1-based)
                let (relative_x, relative_y) = crate::ui::render::adjust_mouse_coords_for_padding(mouse_x, mouse_y, rect.x(), rect.y());
                let col = ((relative_x as f32 / char_width).floor() as u32 + 1).max(1);
                let row = ((relative_y as f32 / char_height).floor() as u32 + 1).max(1);

                if let Ok(mut t) = terminal.lock() {
                    t.send_mouse_event(button, col, row, pressed);
                }
                break;
            }
        }
    }
}

/// Handle selection start
pub fn handle_selection_start(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_x: i32,
    mouse_y: i32,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    window_width: u32,
    window_height: u32,
) {
    let mut gui = match tab_bar_gui.try_lock() {
        Ok(g) => g,
        Err(_) => return, // Skip selection start if lock is busy
    };
    if let Some(pane_layout) = gui.get_active_pane_layout() {
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;
        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

        for (_pane_id, rect, terminal, _is_active) in pane_rects {
            if rect.contains_point((mouse_x, mouse_y)) {
                let (relative_x, relative_y) = crate::ui::render::adjust_mouse_coords_for_padding(mouse_x, mouse_y, rect.x(), rect.y());
                let col = ((relative_x as f32 / char_width).floor() as usize).max(0);
                let row = ((relative_y as f32 / char_height).floor() as usize).max(0);

                if let Ok(mut t) = terminal.lock() {
                    t.start_selection(col, row);
                }
                break;
            }
        }
    }
}

/// Handle selection update
pub fn handle_selection_update(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_x: i32,
    mouse_y: i32,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    window_width: u32,
    window_height: u32,
) {
    let mut gui = match tab_bar_gui.try_lock() {
        Ok(g) => g,
        Err(_) => return, // Skip selection update if lock is busy
    };
    if let Some(pane_layout) = gui.get_active_pane_layout() {
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;
        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

        for (_pane_id, rect, terminal, _is_active) in pane_rects {
            if rect.contains_point((mouse_x, mouse_y)) {
                let (relative_x, relative_y) = crate::ui::render::adjust_mouse_coords_for_padding(mouse_x, mouse_y, rect.x(), rect.y());
                let col = ((relative_x as f32 / char_width).floor() as usize).max(0);
                let row = ((relative_y as f32 / char_height).floor() as usize).max(0);

                if let Ok(mut t) = terminal.lock() {
                    t.update_selection(col, row);
                }
                break;
            }
        }
    }
}

/// Handle mouse button down event
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_button_down(
    mouse_btn: MouseButton,
    mouse_x: i32,
    mouse_y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    mouse_state: &mut MouseState,
    #[allow(unused_variables)]
    #[cfg(target_os = "linux")]
    clipboard_tx: &Sender<Clipboard>,
) -> MouseResult {
    match mouse_btn {
        MouseButton::Right => {
            // Right-click for context menu
            if mouse_y >= tab_bar_height as i32 {
                // Send right mouse button press to terminal (button 2 = right)
                send_mouse_to_terminal(
                    tab_bar_gui,
                    mouse_x,
                    mouse_y,
                    2,
                    true,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );

                let pane_area_y = tab_bar_height as i32;
                let pane_area_height = window_height - tab_bar_height;

                if let Ok(mut gui) = tab_bar_gui.try_lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        // Find which pane was clicked
                        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);
                        for (pane_id, rect, _, _) in pane_rects {
                            if rect.contains_point((mouse_x, mouse_y)) {
                                pane_layout.context_menu_open = Some((pane_id, mouse_x, mouse_y));
                                eprintln!("[MAIN] Context menu opened for pane {:?} at ({}, {})", pane_id, mouse_x, mouse_y);
                                break;
                            }
                        }
                    }
                }
            }
            MouseResult::render()
        }
        MouseButton::Middle => {
            // Check if middle click is on a tab in the tab bar
            if mouse_y < tab_bar_height as i32 {
                if let Some(tab_idx) = tab_bar.get_clicked_tab(mouse_x, mouse_y) {
                    return MouseResult::with_action(MouseAction::CloseTab(tab_idx));
                }
                // If in tab bar but not on a tab, just render
                return MouseResult::render();
            }

            // Send middle mouse button press to terminal (button 1 = middle)
            if mouse_y >= tab_bar_height as i32 {
                send_mouse_to_terminal(
                    tab_bar_gui,
                    mouse_x,
                    mouse_y,
                    1,
                    true,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );
            }

            // Middle click paste
            if let Ok(gui) = tab_bar_gui.try_lock() {
                if let Some(terminal) = gui.get_active_terminal() {
                    if let Ok(mut t) = terminal.try_lock() {
                        #[cfg(target_os = "linux")]
                        {
                            use arboard::{GetExtLinux, LinuxClipboardKind};
                            match Clipboard::new() {
                                Ok(mut clipboard) => match clipboard.get().clipboard(LinuxClipboardKind::Primary).text() {
                                    Ok(text) => {
                                        t.send_paste(&text);
                                    }
                                    Err(e) => {
                                        eprintln!("[PRIMARY] Failed to get PRIMARY clipboard text: {}", e);
                                    }
                                },
                                Err(e) => {
                                    eprintln!("[PRIMARY] Failed to create clipboard: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            MouseResult::render()
        }
        MouseButton::Left => handle_left_button_down(
            mouse_x,
            mouse_y,
            tab_bar,
            tab_bar_gui,
            tab_bar_height,
            char_width,
            char_height,
            window_width,
            window_height,
            mouse_state,
        ),
        _ => MouseResult::none(),
    }
}

/// Handle left mouse button down event
/// Note: Caller should handle text_input().stop() when editing is cancelled
#[allow(clippy::too_many_arguments)]
fn handle_left_button_down(
    mouse_x: i32,
    mouse_y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    mouse_state: &mut MouseState,
) -> MouseResult {
    // Check if clicking on tab bar
    if mouse_y < tab_bar_height as i32 {
        return handle_tab_bar_click(mouse_x, mouse_y, tab_bar, tab_bar_gui, mouse_state);
    }

    // Click outside tab bar - cancel any editing
    // Note: Caller should call text_input().stop() after checking tab_bar.editing_tab changed
    if let Some(editing_idx) = tab_bar.editing_tab {
        if let Ok(mut gui) = tab_bar_gui.try_lock() {
            gui.tab_states[editing_idx].finish_editing(false);
        }
        tab_bar.finish_editing(false);
    }

    // Click in terminal area - check for pane activation or divider drag
    let pane_area_y = tab_bar_height as i32;
    let pane_area_height = window_height - tab_bar_height;

    if let Ok(mut gui) = tab_bar_gui.try_lock() {
        if let Some(pane_layout) = gui.get_active_pane_layout() {
            // Try to start dragging a divider
            if pane_layout.start_drag_divider(mouse_x, mouse_y, 0, pane_area_y, window_width, pane_area_height) {
                mouse_state.dragging_divider = true;
                mouse_state.last_mouse_pos = (mouse_x, mouse_y);
                return MouseResult::with_divider_drag();
            } else {
                // Activate pane
                pane_layout.handle_click(mouse_x, mouse_y, 0, pane_area_y, window_width, pane_area_height);
            }
        }
    }

    // Prepare for potential selection (don't start yet)
    mouse_state.mouse_down_for_selection = true;
    mouse_state.selection_start_pos = (mouse_x, mouse_y);
    mouse_state.selection_started = false;

    // Send left mouse button press event to terminal (button 0 = left)
    send_mouse_to_terminal(
        tab_bar_gui,
        mouse_x,
        mouse_y,
        0,
        true,
        char_width,
        char_height,
        tab_bar_height,
        window_width,
        window_height,
    );

    MouseResult::render()
}

/// Handle tab bar clicks
fn handle_tab_bar_click(mouse_x: i32, mouse_y: i32, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>, mouse_state: &mut MouseState) -> MouseResult {
    // Update hover state
    tab_bar.update_hover(mouse_x, mouse_y);

    // Check CPU indicator
    if tab_bar.cpu_indicator_rect.contains_point(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::OpenSettings);
    }

    // Check window control buttons
    if tab_bar.close_button_rect.contains_point(mouse_x, mouse_y) {
        eprintln!("[MAIN] Close window button clicked");
        return MouseResult::with_action(MouseAction::CloseWindow);
    } else if tab_bar.minimize_button_rect.contains_point(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::MinimizeWindow);
    } else if tab_bar.add_button_rect.contains_point(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::NewTab);
    } else if let Some(close_idx) = tab_bar.get_clicked_close_button(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::CloseTab(close_idx));
    } else if let Some(tab_idx) = tab_bar.get_clicked_tab(mouse_x, mouse_y) {
        // If currently editing a different tab, cancel the edit
        if let Some(editing_idx) = tab_bar.editing_tab {
            if editing_idx != tab_idx {
                if let Ok(mut gui) = tab_bar_gui.try_lock() {
                    gui.tab_states[editing_idx].finish_editing(false);
                }
                tab_bar.finish_editing(false);
                // Note: Caller should call text_input().stop()
            }
        }

        let current_active = match tab_bar_gui.try_lock() {
            Ok(gui) => gui.active_tab,
            Err(_) => return MouseResult::none(), // Skip if can't get lock
        };
        if tab_idx == current_active && tab_bar.editing_tab.is_none() {
            // Clicking on already active tab - start editing
            tab_bar.start_editing(tab_idx);
            if let Ok(mut gui) = tab_bar_gui.try_lock() {
                gui.tab_states[tab_idx].start_editing();
            }
            // Note: Caller should call text_input().start()
        } else if tab_bar.editing_tab.is_none() {
            // Prepare for potential tab drag (will be confirmed on mouse move)
            mouse_state.ready_to_drag_tab = true;
            mouse_state.tab_drag_start_pos = (mouse_x, mouse_y);
            return MouseResult::with_action(MouseAction::SwitchTab(tab_idx));
        }
    }

    MouseResult::render()
}

/// Handle mouse button up event
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_button_up(
    mouse_btn: MouseButton,
    mouse_x: i32,
    mouse_y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    mouse_state: &mut MouseState,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> MouseResult {
    let mut result = MouseResult::none();

    // Handle end of mouse selection
    if mouse_btn == MouseButton::Left && mouse_state.mouse_down_for_selection {
        mouse_state.mouse_down_for_selection = false;

        // Only check for selected text if selection was actually started
        if mouse_state.selection_started {
            handle_selection_complete(
                tab_bar_gui,
                #[cfg(target_os = "linux")]
                clipboard_tx,
            );
        } else {
            // Click without drag - clear any existing selection
            if let Ok(gui) = tab_bar_gui.try_lock() {
                if let Some(terminal) = gui.get_active_terminal() {
                    if let Ok(mut t) = terminal.try_lock() {
                        t.clear_selection();
                    }
                }
            }
        }
        mouse_state.selection_started = false;
        result.needs_render = true;
    }

    if mouse_state.dragging_divider {
        mouse_state.dragging_divider = false;
        let release_start = std::time::Instant::now();
        // Non-blocking lock - critical for responsiveness during drag release
        if let Ok(mut gui) = tab_bar_gui.try_lock() {
            if let Some(pane_layout) = gui.get_active_pane_layout() {
                pane_layout.stop_drag_divider();
            }
        } else {
            eprintln!("[PERF] Warning: Failed to acquire lock during drag stop");
        }
        let stop_time = release_start.elapsed();
        eprintln!("[PERF] Drag release: stop={}µs", stop_time.as_micros());
        result.needs_render = true;
    }

    // Handle end of tab dragging
    if mouse_btn == MouseButton::Left && mouse_state.dragging_tab {
        mouse_state.dragging_tab = false;
        mouse_state.ready_to_drag_tab = false;
        mouse_state.tab_drag_start_pos = (0, 0);
        if let Some((from_idx, to_idx)) = tab_bar.stop_dragging_tab() {
            // Reorder the tabs
            if let Ok(mut gui) = tab_bar_gui.try_lock() {
                gui.reorder_tab(from_idx, to_idx);
            }
        }
        result.needs_render = true;
    }

    // If left button released but no drag happened, clear ready state
    if mouse_btn == MouseButton::Left && mouse_state.ready_to_drag_tab {
        mouse_state.ready_to_drag_tab = false;
    }

    // Send mouse release events to terminal
    if mouse_y >= tab_bar_height as i32 {
        let button = match mouse_btn {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => 0, // Default to left for other buttons
        };
        send_mouse_to_terminal(
            tab_bar_gui,
            mouse_x,
            mouse_y,
            button,
            false,
            char_width,
            char_height,
            tab_bar_height,
            window_width,
            window_height,
        );
    }

    // Handle context menu clicks
    if mouse_btn == MouseButton::Left {
        if let Some(action) = handle_context_menu_click(mouse_x, mouse_y, tab_bar_gui) {
            result.action = action;
            result.needs_render = true;
        }
    }

    result
}

/// Handle context menu clicks
fn handle_context_menu_click(mouse_x: i32, mouse_y: i32, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> Option<MouseAction> {
    let mut gui = tab_bar_gui.lock().unwrap();
    if let Some(pane_layout) = gui.get_active_pane_layout() {
        if let Some((menu_pane_id, menu_x, menu_y)) = pane_layout.context_menu_open {
            // Check if clicking on context menu
            let menu_rect = sdl3::rect::Rect::new(menu_x, menu_y, 400, 175);
            if menu_rect.contains_point((mouse_x, mouse_y)) {
                // Handle menu item clicks
                let relative_y = mouse_y - menu_y - 5;
                let item_index = (relative_y / 55) as usize;

                // Check pane count to determine if "Turn into a tab" is disabled
                let pane_count = pane_layout.root.count_leaf_panes();

                if item_index < 3 {
                    match item_index {
                        0 => pane_layout.pending_context_action = Some((menu_pane_id, "split_vertical".to_string())),
                        1 => pane_layout.pending_context_action = Some((menu_pane_id, "split_horizontal".to_string())),
                        2 => {
                            // Only allow "Turn into a tab" if there's more than 1 pane
                            if pane_count > 1 {
                                pane_layout.pending_context_action = Some((menu_pane_id, "to_tab".to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            // Close menu on any click
            pane_layout.context_menu_open = None;
        }
    }
    None
}

/// Handle selection complete (copy to clipboard)
fn handle_selection_complete(tab_bar_gui: &Arc<Mutex<TabBarGui>>, #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>) {
    if let Ok(gui) = tab_bar_gui.try_lock() {
        if let Some(terminal) = gui.get_active_terminal() {
            if let Ok(t) = terminal.try_lock() {
                if let Some(text) = t.get_selected_text() {
                    if !text.is_empty() {
                        // Copy selected text to PRIMARY clipboard (Linux middle-click clipboard)
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
                        #[cfg(not(target_os = "linux"))]
                        {
                            match Clipboard::new() {
                                Ok(mut clipboard) => {
                                    if let Err(e) = clipboard.set_text(text) {
                                        eprintln!("[CLIPBOARD] Failed to copy: {}", e);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[CLIPBOARD] Failed to create clipboard: {}", e);
                                }
                            }
                        }
                    } else {
                        drop(t);
                        if let Ok(gui) = tab_bar_gui.try_lock() {
                            if let Some(terminal) = gui.get_active_terminal() {
                                if let Ok(mut t) = terminal.try_lock() {
                                    t.clear_selection();
                                }
                            }
                        }
                    }
                } else {
                    drop(t);
                    if let Ok(gui) = tab_bar_gui.try_lock() {
                        if let Some(terminal) = gui.get_active_terminal() {
                            if let Ok(mut t) = terminal.try_lock() {
                                t.clear_selection();
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Handle mouse motion event
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_motion(
    mouse_x: i32,
    mouse_y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    mouse_state: &mut MouseState,
) -> MouseResult {
    let mut needs_render = false;

    // Always update tab bar hover to handle unhover correctly
    tab_bar.update_hover(mouse_x, mouse_y);

    if mouse_y < tab_bar_height as i32 {
        needs_render = true;
    }

    // Handle tab dragging in tab bar
    if mouse_y < tab_bar_height as i32 && !mouse_state.dragging_tab && mouse_state.ready_to_drag_tab {
        // Check if we should start dragging a tab
        let distance_moved = ((mouse_x - mouse_state.tab_drag_start_pos.0).pow(2) + (mouse_y - mouse_state.tab_drag_start_pos.1).pow(2)) as f32;
        // Threshold: about 5 pixels (5^2 = 25) to distinguish from click
        if distance_moved > 25.0 {
            if let Some(tab_idx) = tab_bar.get_clicked_tab(mouse_state.tab_drag_start_pos.0, mouse_state.tab_drag_start_pos.1) {
                // Don't start dragging if editing a tab
                if tab_bar.editing_tab.is_none() {
                    tab_bar.start_dragging_tab(tab_idx, mouse_state.tab_drag_start_pos.0);
                    mouse_state.dragging_tab = true;
                    mouse_state.ready_to_drag_tab = false;
                    needs_render = true;
                }
            }
        }
    } else if mouse_state.dragging_tab {
        // Update tab drag position
        tab_bar.update_drag(mouse_x);
        needs_render = true;
    }

    // Start/update selection if mouse is dragging with left button down
    if mouse_state.mouse_down_for_selection && mouse_y >= tab_bar_height as i32 && !mouse_state.dragging_tab {
        let distance_moved = ((mouse_x - mouse_state.selection_start_pos.0).pow(2) + (mouse_y - mouse_state.selection_start_pos.1).pow(2)) as f32;
        // Threshold: about 5 pixels (5^2 = 25)
        if distance_moved > 25.0 {
            if !mouse_state.selection_started {
                // First time exceeding threshold - start selection at original position
                handle_selection_start(
                    tab_bar_gui,
                    mouse_state.selection_start_pos.0,
                    mouse_state.selection_start_pos.1,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );
                mouse_state.selection_started = true;
            }

            // Update selection to current position
            handle_selection_update(
                tab_bar_gui,
                mouse_x,
                mouse_y,
                char_width,
                char_height,
                tab_bar_height,
                window_width,
                window_height,
            );
            needs_render = true;
        }
    }

    if mouse_state.dragging_divider {
        let drag_start = std::time::Instant::now();
        let delta_x = mouse_x - mouse_state.last_mouse_pos.0;
        let delta_y = mouse_y - mouse_state.last_mouse_pos.1;

        // Throttle: process every 3rd motion event to reduce lock contention
        mouse_state.drag_motion_counter = mouse_state.drag_motion_counter.wrapping_add(1);
        if mouse_state.drag_motion_counter % 3 == 0 {
            let pane_area_y = tab_bar_height as i32;
            let pane_area_height = window_height - tab_bar_height;

            // Non-blocking lock - skip update if lock is busy
            let lock_start = std::time::Instant::now();
            if let Ok(mut gui) = tab_bar_gui.try_lock() {
                let lock_acquired = lock_start.elapsed();
                if let Some(pane_layout) = gui.get_active_pane_layout() {
                    pane_layout.update_drag_divider(delta_x, delta_y, 0, pane_area_y, window_width, pane_area_height);
                    // Only update last_mouse_pos after successfully applying the delta
                    mouse_state.last_mouse_pos = (mouse_x, mouse_y);
                }
                let update_done = lock_start.elapsed();
                if update_done.as_micros() > 1000 {
                    eprintln!("[PERF] Drag update: lock={}µs, total={}µs", lock_acquired.as_micros(), update_done.as_micros());
                }
            } else {
                eprintln!("[PERF] Skipped drag update - lock busy (tried in {}µs)", lock_start.elapsed().as_micros());
            }
        }
        let total_drag_time = drag_start.elapsed();
        if total_drag_time.as_micros() > 2000 {
            eprintln!("[PERF] Total drag motion handling: {}µs", total_drag_time.as_micros());
        }
        needs_render = true;
    }

    MouseResult {
        action: MouseAction::None,
        needs_render,
    }
}

/// Handle mouse wheel event
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_wheel(
    wheel_y: i32,
    wheel_x: i32,
    mouse_x: i32,
    mouse_y: i32,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
) -> MouseResult {
    if mouse_y < tab_bar_height as i32 {
        return MouseResult::none();
    }

    let mut needs_render = false;

    // Mouse wheel scrolls through scrollback buffer
    // y > 0 is scroll up (backward in time), y < 0 is scroll down (forward in time)
    if wheel_y != 0 {
        if let Some(terminal) = tab_bar_gui.lock().unwrap().get_active_terminal() {
            let t = terminal.lock().unwrap();
            let lines_to_scroll = wheel_y.abs().max(1) as usize;

            if wheel_y > 0 {
                // Scroll up (backward) through scrollback
                t.screen_buffer.lock().unwrap().scroll_view_up(lines_to_scroll);
            } else {
                // Scroll down (forward) toward live view
                t.screen_buffer.lock().unwrap().scroll_view_down(lines_to_scroll);
            }
            needs_render = true;
        }
    }

    // Handle horizontal scrolling if needed (less common)
    if wheel_x != 0 {
        match wheel_x.cmp(&0) {
            std::cmp::Ordering::Greater => {
                // Scroll right - button 66
                send_mouse_to_terminal(
                    tab_bar_gui,
                    mouse_x,
                    mouse_y,
                    66,
                    true,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );
            }
            std::cmp::Ordering::Less => {
                // Scroll left - button 67
                send_mouse_to_terminal(
                    tab_bar_gui,
                    mouse_x,
                    mouse_y,
                    67,
                    true,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );
            }
            std::cmp::Ordering::Equal => {}
        }
    }

    MouseResult {
        action: MouseAction::None,
        needs_render,
    }
}
