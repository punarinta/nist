use crate::url_detection::{detect_url_at_position, UrlInfo};
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
    MaximizeRestoreWindow,
    CloseTab(usize),
    CloseTabWithConfirm(usize),
    SwitchTab(usize),
    OpenSettings,
    TabReordered,
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
    pub ctrl_pressed: bool,
    pub hovered_url: Option<UrlInfo>,
    /// True when Shift is held during a mouse-down, bypassing app mouse tracking for native selection
    pub shift_bypassing_mouse: bool,
    /// How many viewport lines we scrolled up during an active drag to compensate for scrollback
    /// growth (new PTY output). Reset to 0 on mouse-up by scrolling back down the same amount.
    pub selection_drag_scroll_compensation: usize,
    /// Leftover fractional vertical wheel delta (touchpads/hi-res wheels emit sub-1.0 events)
    pub wheel_accum_y: f32,
    /// Leftover fractional horizontal wheel delta
    pub wheel_accum_x: f32,
    /// When the previous wheel event arrived, used to drop stale leftovers between gestures.
    pub last_wheel_at: Option<std::time::Instant>,
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
            ctrl_pressed: false,
            hovered_url: None,
            shift_bypassing_mouse: false,
            selection_drag_scroll_compensation: 0,
            wheel_accum_y: 0.0,
            wheel_accum_x: 0.0,
            last_wheel_at: None,
        }
    }
}

/// A gap this long between wheel events ends the gesture: leftover fractions are
/// dropped so the next flick starts from zero instead of inheriting old progress.
pub const WHEEL_GESTURE_GAP: std::time::Duration = std::time::Duration::from_millis(250);

/// Accumulate a (possibly fractional) wheel delta and return the whole number of steps
/// to apply now. The fractional remainder stays in `accum` for the next event, so a burst
/// of small touchpad deltas (e.g. 0.3 each) adds up instead of truncating to zero.
///
/// Deltas are summed signed: a touchpad reports plenty of tiny opposite-sign jitter
/// mid-gesture, and discarding the leftover on every sign flip is what made a slow
/// two-finger scroll never reach a whole step. Leftovers are dropped by elapsed time
/// (see `WHEEL_GESTURE_GAP`), not by direction.
pub fn accumulate_wheel_delta(accum: &mut f32, delta: f32) -> i32 {
    if delta == 0.0 {
        return 0;
    }
    *accum += delta;
    let steps = accum.trunc();
    *accum -= steps;
    steps as i32
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
    // Blocking lock on purpose: this is the main thread and every other holder of
    // this mutex keeps it briefly. Skipping on a busy lock silently dropped mouse
    // reports (wheel scrolling in mouse-tracking apps just did nothing).
    let mut gui = match tab_bar_gui.lock() {
        Ok(g) => g,
        Err(_) => return, // Poisoned: nothing useful to do with this event
    };
    if let Some(pane_layout) = gui.get_active_pane_layout() {
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;
        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

        // Find which pane contains the mouse
        for (_pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
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

        for (_pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
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

/// Handle selection update: move the selection's loose end to the mouse position.
///
/// The pointer may sit outside the pane — a drag that runs past the top or bottom
/// edge is exactly how you select more than one screenful. In that case the viewport
/// is scrolled by however many rows the pointer overshoots (proportional autoscroll,
/// so the further out you drag the faster it goes) and the loose end is clamped to
/// the edge row. The selection's anchor is stored in absolute rows, so it keeps
/// pointing at the line it was started on however far the view travels.
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

        // The pane under the pointer, or — when the drag has left every pane — the
        // active one, which is the pane holding the selection being dragged.
        let target = pane_rects
            .iter()
            .find(|(_, rect, _, _, _)| rect.contains_point((mouse_x, mouse_y)))
            .or_else(|| pane_rects.iter().find(|(_, _, _, is_active, _)| *is_active));

        if let Some((_pane_id, rect, terminal, _is_active, _is_selected)) = target {
            let (relative_x, relative_y) = crate::ui::render::adjust_mouse_coords_for_padding(mouse_x, mouse_y, rect.x(), rect.y());
            let col_f = relative_x as f32 / char_width;
            let row_f = relative_y as f32 / char_height;

            if let Ok(mut t) = terminal.lock() {
                let (term_w, term_h) = (t.width.max(1), t.height.max(1));

                // Rows the pointer overshoots the pane by, in either direction.
                let overshoot_up = if row_f < 0.0 { (-row_f).ceil() as usize } else { 0 };
                let overshoot_down = if row_f >= term_h as f32 {
                    (row_f - term_h as f32 + 1.0).ceil() as usize
                } else {
                    0
                };
                if overshoot_up > 0 || overshoot_down > 0 {
                    if let Ok(mut gb) = t.ghostty_buffer.try_lock() {
                        if overshoot_up > 0 {
                            gb.scroll_view_up(overshoot_up);
                        } else {
                            gb.scroll_view_down(overshoot_down);
                        }
                    }
                }

                // Scroll first, then resolve the row: the loose end must land on the
                // line now shown at the edge, not the one that was there before.
                let col = col_f.floor().clamp(0.0, (term_w - 1) as f32) as usize;
                let row = row_f.floor().clamp(0.0, (term_h - 1) as f32) as usize;
                t.update_selection(col, row);
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
    clicks: u8,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    scale_factor: f32,
    mouse_state: &mut MouseState,
    event_pump: &sdl3::EventPump,
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
                        // Find which pane was clicked and open context menu
                        let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);
                        for (pane_id, rect, _, _, _) in pane_rects {
                            if rect.contains_point((mouse_x, mouse_y)) {
                                pane_layout.open_context_menu(pane_id, mouse_x, mouse_y, window_width as i32, window_height as i32, scale_factor);
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
            clicks,
            tab_bar,
            tab_bar_gui,
            tab_bar_height,
            char_width,
            char_height,
            window_width,
            window_height,
            mouse_state,
            event_pump,
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
    clicks: u8,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    mouse_state: &mut MouseState,
    event_pump: &sdl3::EventPump,
) -> MouseResult {
    // Check if clicking on tab bar
    if mouse_y < tab_bar_height as i32 {
        return handle_tab_bar_click(mouse_x, mouse_y, clicks, tab_bar, tab_bar_gui, mouse_state);
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

    // Check if Ctrl/Shift are pressed
    let keyboard_state = event_pump.keyboard_state();
    let is_ctrl_pressed =
        keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::LCtrl) || keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::RCtrl);
    let is_shift_pressed =
        keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::LShift) || keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::RShift);

    // Ctrl+Click on URL - open it in browser
    if is_ctrl_pressed && mouse_state.hovered_url.is_some() {
        if let Some(ref url_info) = mouse_state.hovered_url {
            match crate::url_detection::open_url_in_browser(&url_info.url) {
                Ok(_) => {
                    eprintln!("[URL] Opened URL: {}", url_info.url);
                }
                Err(e) => {
                    eprintln!("[URL] Failed to open URL: {}", e);
                }
            }
            // Clear the hovered URL after opening
            mouse_state.hovered_url = None;
            return MouseResult::render();
        }
    }

    let mut terminal_has_mouse_tracking = false;
    let mut shift_bypass = false;

    if let Ok(mut gui) = tab_bar_gui.try_lock() {
        // Check if other tabs have selections (before mutable borrow)
        let has_other_tab_selections = if is_ctrl_pressed { gui.has_selections_on_other_tab() } else { false };

        if let Some(pane_layout) = gui.get_active_pane_layout() {
            // Try to start dragging a divider
            if pane_layout.start_drag_divider(mouse_x, mouse_y, 0, pane_area_y, window_width, pane_area_height) {
                mouse_state.dragging_divider = true;
                mouse_state.last_mouse_pos = (mouse_x, mouse_y);
                return MouseResult::with_divider_drag();
            }

            // Handle pane click
            if let Some(clicked_pane_id) = pane_layout.handle_click(mouse_x, mouse_y, 0, pane_area_y, window_width, pane_area_height) {
                if is_ctrl_pressed {
                    // Ctrl+click: toggle pane selection for group input
                    // Only allow if no other tab has selections
                    if !has_other_tab_selections {
                        pane_layout.toggle_pane_selection(clicked_pane_id);
                        eprintln!(
                            "[GROUP INPUT] Toggled pane {:?} selection. Selected panes: {:?}",
                            clicked_pane_id, pane_layout.selected_panes
                        );
                    } else {
                        eprintln!("[GROUP INPUT] Cannot select on this tab - another tab has selections");
                    }
                }
                // Note: handle_click already sets the active pane
            }

            // Check if the running program wants mouse events
            if let Some(terminal) = gui.get_active_terminal() {
                if let Ok(t) = terminal.try_lock() {
                    terminal_has_mouse_tracking = t.is_mouse_tracking_enabled();
                }
            }
            // Shift+click bypasses app mouse tracking so the user can always make native selections
            shift_bypass = is_shift_pressed && terminal_has_mouse_tracking;

            // Handle double-click word selection and triple-click line selection.
            // Skip when the program has mouse tracking enabled - pass the clicks through instead.
            // Exception: Shift held bypasses mouse tracking for native selection.
            if (!terminal_has_mouse_tracking || shift_bypass) && clicks >= 2 && mouse_y >= tab_bar_height as i32 {
                if let Some(terminal) = gui.get_active_terminal() {
                    if let Ok(mut t) = terminal.try_lock() {
                        // Convert mouse coordinates to terminal cell coordinates
                        let pane_padding = crate::ui::render::get_pane_padding();

                        // Get active pane rect
                        if let Some(pane_layout) = gui.get_active_pane_layout() {
                            let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

                            // Find the active pane rect
                            if let Some((_, rect, _, _, _)) = pane_rects.iter().find(|(pid, _, _, _, _)| *pid == pane_layout.active_pane) {
                                let col = ((mouse_x - rect.x() - pane_padding as i32) as f32 / char_width) as usize;
                                let row = ((mouse_y - rect.y() - pane_padding as i32) as f32 / char_height) as usize;

                                if clicks >= 3 {
                                    // Select the whole line at this position
                                    t.select_line_at(row);
                                } else {
                                    // Select the word at this position
                                    t.select_word_at(col, row);
                                }

                                // Don't prepare for regular selection
                                drop(t);
                                drop(gui);

                                return MouseResult::render();
                            }
                        }
                    }
                }
            }
        }
    }

    // Always prepare for a potential drag selection.
    // Motion events are never forwarded to the app, so drag-selecting never
    // conflicts with app mouse tracking.  The initial click is still forwarded
    // (see below) so app interactions like clicking buttons still work.
    // Shift+tracking sets shift_bypassing_mouse which also suppresses the
    // button-down forwarding for a fully transparent selection experience.
    mouse_state.mouse_down_for_selection = true;
    mouse_state.selection_start_pos = (mouse_x, mouse_y);
    mouse_state.selection_started = false;
    mouse_state.shift_bypassing_mouse = shift_bypass;

    // Send left mouse button press event to terminal (button 0 = left).
    // Skip when Shift is bypassing mouse tracking - the click is for selection, not the app.
    if !shift_bypass {
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
    }

    MouseResult::render()
}

/// Handle tab bar clicks
fn handle_tab_bar_click(mouse_x: i32, mouse_y: i32, clicks: u8, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>, mouse_state: &mut MouseState) -> MouseResult {
    // Update hover state
    tab_bar.update_hover(mouse_x, mouse_y);

    // Check scroll buttons
    if tab_bar.left_scroll_button_rect.contains_point(mouse_x, mouse_y) {
        tab_bar.scroll_left();
        return MouseResult::render();
    }
    if tab_bar.right_scroll_button_rect.contains_point(mouse_x, mouse_y) {
        tab_bar.scroll_right();
        return MouseResult::render();
    }

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
    } else if tab_bar.maximize_button_rect.contains_point(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::MaximizeRestoreWindow);
    } else if tab_bar.add_button_rect.contains_point(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::NewTab);
    } else if let Some(close_idx) = tab_bar.get_clicked_close_button(mouse_x, mouse_y) {
        return MouseResult::with_action(MouseAction::CloseTabWithConfirm(close_idx));
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
        if tab_idx == current_active && tab_bar.editing_tab.is_none() && clicks >= 2 {
            // Double-clicking on already active tab - start editing
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
    let was_shift_bypassing = mouse_state.shift_bypassing_mouse;
    let was_selection_started = mouse_state.selection_started;
    if mouse_btn == MouseButton::Left && mouse_state.mouse_down_for_selection {
        mouse_state.mouse_down_for_selection = false;
        mouse_state.shift_bypassing_mouse = false;

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
        // Undo any viewport scroll we applied during the drag to keep selected rows visible.
        // This returns the viewport to its pre-drag position (typically the live bottom).
        let comp = std::mem::replace(&mut mouse_state.selection_drag_scroll_compensation, 0);
        if comp > 0 {
            if let Ok(gui) = tab_bar_gui.try_lock() {
                if let Some(term) = gui.get_active_terminal() {
                    if let Ok(t) = term.try_lock() {
                        if let Ok(mut gb) = t.ghostty_buffer.try_lock() {
                            gb.scroll_view_down(comp);
                        }
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
            result.action = MouseAction::TabReordered;
        }
        result.needs_render = true;
    }

    // If left button released but no drag happened, clear ready state
    if mouse_btn == MouseButton::Left && mouse_state.ready_to_drag_tab {
        mouse_state.ready_to_drag_tab = false;
    }

    // Send mouse release events to terminal.
    // Skip the left button release when a drag selection occurred (whether via Shift-bypass
    // or a plain drag).  The app already got the button-down; sending a release at a
    // different position would confuse it about the click location.
    let skip_left_release = mouse_btn == MouseButton::Left
        && (was_shift_bypassing || was_selection_started);
    if mouse_y >= tab_bar_height as i32 && !skip_left_release {
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
        pane_layout.handle_context_menu_click(mouse_x, mouse_y);
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

    // Update context menu hover if open
    if let Ok(mut gui) = tab_bar_gui.try_lock() {
        if let Some(pane_layout) = gui.get_active_pane_layout() {
            if pane_layout.context_menu.is_some() {
                pane_layout.update_context_menu_hover(mouse_x, mouse_y);
                needs_render = true;
            }
        }
    }

    if mouse_y < tab_bar_height as i32 {
        needs_render = true;
    }

    // Detect URL hover when Ctrl is held and mouse is in terminal area
    if mouse_state.ctrl_pressed && mouse_y >= tab_bar_height as i32 {
        // Calculate terminal coordinates
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;

        // Try to get the active terminal and detect URL
        if let Ok(mut gui) = tab_bar_gui.try_lock() {
            if let Some(pane_layout) = gui.get_active_pane_layout() {
                let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

                // Find which pane the mouse is over
                for (pane_id, rect, terminal, _, _) in pane_rects {
                    if mouse_x >= rect.x() && mouse_x < rect.x() + rect.width() as i32 && mouse_y >= rect.y() && mouse_y < rect.y() + rect.height() as i32 {
                        // Mouse is over this pane
                        if let Ok(t) = terminal.try_lock() {
                            let pane_padding = crate::ui::render::get_pane_padding();

                            // Convert to terminal cell coordinates
                            let col = ((mouse_x - rect.x() - pane_padding as i32) as f32 / char_width).floor() as usize;
                            let row = ((mouse_y - rect.y() - pane_padding as i32) as f32 / char_height).floor() as usize;

                            // Detect URL at this position
                            let new_url = if let Ok(gb) = t.ghostty_buffer.lock() {
                                detect_url_at_position(&gb, row, col, pane_id)
                            } else {
                                None
                            };

                            // Check if URL hover changed
                            let url_changed = match (&mouse_state.hovered_url, &new_url) {
                                (None, None) => false,
                                (Some(_), None) | (None, Some(_)) => true,
                                (Some(old), Some(new)) => {
                                    old.url != new.url || old.row != new.row || old.col_start != new.col_start || old.pane_id != new.pane_id
                                }
                            };

                            if url_changed {
                                mouse_state.hovered_url = new_url;
                                needs_render = true;
                            }
                        }
                        break;
                    }
                }
            }
        }
    } else if !mouse_state.ctrl_pressed && mouse_state.hovered_url.is_some() {
        // Ctrl was released, clear hovered URL
        mouse_state.hovered_url = None;
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

    // Start/update selection if mouse is dragging with left button down.
    // Once the drag is under way we keep following the pointer even after it leaves
    // the terminal area (up into the tab bar, off the bottom of the window): that is
    // what lets a selection grow past what the screen shows, via the autoscroll in
    // handle_selection_update.
    if mouse_state.mouse_down_for_selection
        && !mouse_state.dragging_tab
        && (mouse_state.selection_started || mouse_y >= tab_bar_height as i32)
    {
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
        if mouse_state.drag_motion_counter.is_multiple_of(3) {
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
///
/// `scroll_multiplier` is how many lines one wheel click scrolls (3 is the terminal
/// convention). It is applied before accumulation, so a touchpad's fractional deltas
/// are scaled up too — without it a gentle two-finger scroll needs a full wheel click
/// worth of travel to move a single line, which reads as "scrolling is stuck".
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_wheel(
    wheel_y_raw: f32,
    wheel_x_raw: f32,
    mouse_x: i32,
    mouse_y: i32,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    window_width: u32,
    window_height: u32,
    scroll_multiplier: f32,
) -> MouseResult {
    if mouse_y < tab_bar_height as i32 {
        return MouseResult::none();
    }

    // A pause between flicks ends the gesture: don't let a leftover fraction from a
    // gesture minutes ago count toward this one (or fight its direction).
    let now = std::time::Instant::now();
    if mouse_state
        .last_wheel_at
        .is_none_or(|prev| now.duration_since(prev) > WHEEL_GESTURE_GAP)
    {
        mouse_state.wheel_accum_y = 0.0;
        mouse_state.wheel_accum_x = 0.0;
    }
    mouse_state.last_wheel_at = Some(now);

    let multiplier = if scroll_multiplier > 0.0 { scroll_multiplier } else { 1.0 };
    let wheel_y = accumulate_wheel_delta(&mut mouse_state.wheel_accum_y, wheel_y_raw * multiplier);
    let wheel_x = accumulate_wheel_delta(&mut mouse_state.wheel_accum_x, wheel_x_raw * multiplier);

    let mut needs_render = false;

    // y > 0 is scroll up (backward in time), y < 0 is scroll down (forward in time)
    if wheel_y != 0 {
        // One lock for the whole operation: locking per report let a busy frame drop
        // wheel reports on the floor (send_mouse_to_terminal skips on a busy lock).
        let terminal = tab_bar_gui.lock().unwrap().get_active_terminal();
        let (tracking_enabled, alt_screen) = terminal
            .as_ref()
            .map(|t| {
                let t = t.lock().unwrap();
                let alt = t.ghostty_buffer.lock().map(|gb| gb.is_alt_screen()).unwrap_or(false);
                (t.is_mouse_tracking_enabled(), alt)
            })
            .unwrap_or((false, false));

        let lines = wheel_y.unsigned_abs() as usize;

        if tracking_enabled {
            // Forward scroll to app as button 64 (up) / 65 (down)
            let button = if wheel_y > 0 { 64u8 } else { 65u8 };
            for _ in 0..lines {
                send_mouse_to_terminal(
                    tab_bar_gui,
                    mouse_x,
                    mouse_y,
                    button,
                    true,
                    char_width,
                    char_height,
                    tab_bar_height,
                    window_width,
                    window_height,
                );
            }
        } else if alt_screen {
            // Alternate screen has no scrollback to move. Translate the wheel into
            // cursor keys (xterm's "alternate scroll") so full-screen apps that don't
            // track the mouse — less, man, git log — still scroll.
            if let Some(terminal) = terminal {
                if let Ok(mut t) = terminal.lock() {
                    let key: &[u8] = if wheel_y > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..lines {
                        t.send_key(key);
                    }
                }
            }
        } else if let Some(terminal) = terminal {
            {
                let t = terminal.lock().unwrap();
                if wheel_y > 0 {
                    t.ghostty_buffer.lock().unwrap().scroll_view_up(lines);
                } else {
                    t.ghostty_buffer.lock().unwrap().scroll_view_down(lines);
                }
            }
            needs_render = true;

            if mouse_state.mouse_down_for_selection && mouse_state.selection_started {
                // Scrolling mid-drag extends the selection over the lines the scroll
                // just brought under the pointer — that is how you select more than
                // one screenful without dragging to the edge. The anchor is stored in
                // absolute rows, so it stays on the line the drag started from.
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
                // The viewport is now where the user put it. Drop the compensation
                // owed for PTY output during this drag so mouse-up does not yank the
                // view back down out from under them.
                mouse_state.selection_drag_scroll_compensation = 0;
            }
        }
    }

    // Handle horizontal scrolling if needed (less common). One report per column,
    // same as the vertical axis, so a wide gesture doesn't collapse into one step.
    if wheel_x != 0 {
        let button = if wheel_x > 0 { 66u8 } else { 67u8 };
        for _ in 0..wheel_x.unsigned_abs() {
            send_mouse_to_terminal(
                tab_bar_gui,
                mouse_x,
                mouse_y,
                button,
                true,
                char_width,
                char_height,
                tab_bar_height,
                window_width,
                window_height,
            );
        }
    }

    MouseResult {
        action: MouseAction::None,
        needs_render,
    }
}
