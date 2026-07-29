use sdl3::event::Event;
use std::sync::{Arc, Mutex};

use super::keyboard::KeyboardAction;
use super::mouse::{MouseAction, MouseState};
use crate::sdl_renderer::TabBar;
use crate::settings::Settings;
use crate::tab_gui::TabBarGui;

#[cfg(target_os = "linux")]
use arboard::Clipboard;
#[cfg(target_os = "linux")]
use std::sync::mpsc::Sender;

/// Actions that can be requested from event handling
#[derive(Debug, Clone)]
pub enum EventAction {
    RequestQuitConfirmation,
    Quit,
    NewTab,
    SplitPane(crate::pane_layout::SplitDirection),
    CloseTab(usize),
    CloseTabWithConfirm(usize),
    SwitchTab(usize),
    TabRenamed,
    TabReordered,
    PaneClosed,
    MinimizeWindow,
    MaximizeRestoreWindow,
    Resize,
    StartTextInput,
    StopTextInput,
    OpenSettings,
    ChangeFontSize(f32),
    TerminalHistorySearch,
    AiCommandGeneration,
    VoiceInput,
    StopVoiceInput,
    None,
}

/// Result of handling an event
pub struct EventResult {
    pub action: EventAction,
    pub needs_render: bool,
    pub needs_resize: bool,
}

impl EventResult {
    pub fn none() -> Self {
        Self {
            action: EventAction::None,
            needs_render: false,
            needs_resize: false,
        }
    }

    pub fn quit() -> Self {
        Self {
            action: EventAction::Quit,
            needs_render: false,
            needs_resize: false,
        }
    }

    pub fn resize() -> Self {
        Self {
            action: EventAction::Resize,
            needs_render: true,
            needs_resize: false,
        }
    }
}

/// Handle a single SDL2 event
pub fn handle_event(
    event: &Event,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    ctrl_keys: &std::collections::HashMap<sdl3::keyboard::Scancode, u8>,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    event_pump: &sdl3::EventPump,
    settings: &Settings,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    match event {
        Event::Quit { .. } => EventResult::quit(),

        // A plain resize, a change in the drawable's physical pixel size, or the
        // window moving to a differently-scaled display all require the render
        // state (and possibly the scale factor) to be recomputed. The main loop's
        // resize handler re-detects scaling and rebuilds fonts/chrome if it changed.
        Event::Window {
            win_event:
                sdl3::event::WindowEvent::Resized(..)
                | sdl3::event::WindowEvent::PixelSizeChanged(..)
                | sdl3::event::WindowEvent::DisplayChanged(..),
            ..
        } => EventResult::resize(),

        // Runtime display scale change (e.g. `wlr-randr --scale` while running).
        Event::Display {
            display_event: sdl3::event::DisplayEvent::ContentScaleChanged,
            ..
        } => EventResult::resize(),

        Event::MouseButtonDown { mouse_btn, x, y, clicks, .. } => handle_mouse_button_down_event(
            *mouse_btn,
            *x as i32,
            *y as i32,
            *clicks,
            tab_bar,
            tab_bar_gui,
            mouse_state,
            scale_factor,
            mouse_coords_need_scaling,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            event_pump,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        ),

        Event::MouseButtonUp { mouse_btn, x, y, .. } => handle_mouse_button_up_event(
            *mouse_btn,
            *x as i32,
            *y as i32,
            tab_bar,
            tab_bar_gui,
            mouse_state,
            scale_factor,
            mouse_coords_need_scaling,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        ),

        Event::MouseMotion { x, y, .. } => handle_mouse_motion_event(
            *x as i32,
            *y as i32,
            tab_bar,
            tab_bar_gui,
            mouse_state,
            scale_factor,
            mouse_coords_need_scaling,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
        ),

        Event::MouseWheel { y, x, mouse_x, mouse_y, .. } => handle_mouse_wheel_event(
            *y,
            *x,
            *mouse_x,
            *mouse_y,
            tab_bar_gui,
            mouse_state,
            scale_factor,
            mouse_coords_need_scaling,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            event_pump,
            settings,
        ),

        Event::KeyDown { keycode, keymod, scancode, .. } => handle_key_down_event(
            *keycode,
            *keymod,
            *scancode,
            tab_bar,
            tab_bar_gui,
            ctrl_keys,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            settings,
            mouse_state,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        ),

        Event::KeyUp { keycode, .. } => handle_key_up_event(*keycode, mouse_state, settings),
        Event::TextInput { ref text, .. } => handle_text_input_event(text, tab_bar, tab_bar_gui),

        _ => EventResult::none(),
    }
}

/// Window size in the coordinate space mouse hit-testing works in.
///
/// Mouse coordinates are scaled to physical pixels (see `mouse_coords_need_scaling`)
/// and `tab_bar_height` is physical too, so pane rects must be built from the
/// physical drawable size. Using the logical size here shrinks every pane rect on a
/// scaled display and silently drops events for a pointer outside the shrunken rect.
fn mouse_space_window_size(canvas_window: &sdl3::video::Window) -> (u32, u32) {
    canvas_window.size_in_pixels()
}

fn handle_mouse_button_down_event(
    mouse_btn: sdl3::mouse::MouseButton,
    x: i32,
    y: i32,
    clicks: u8,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    event_pump: &sdl3::EventPump,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    let (mouse_x, mouse_y) = if mouse_coords_need_scaling {
        ((x as f32 * scale_factor) as i32, (y as f32 * scale_factor) as i32)
    } else {
        (x, y)
    };

    let (w, h) = mouse_space_window_size(canvas_window);

    let result = super::mouse::handle_mouse_button_down(
        mouse_btn,
        mouse_x,
        mouse_y,
        clicks,
        tab_bar,
        tab_bar_gui,
        tab_bar_height,
        char_width,
        char_height,
        w,
        h,
        scale_factor,
        mouse_state,
        event_pump,
        #[cfg(target_os = "linux")]
        clipboard_tx,
    );

    // Map mouse action to event action
    let event_action = match result.action {
        MouseAction::CloseWindow => EventAction::Quit,
        MouseAction::MinimizeWindow => EventAction::MinimizeWindow,
        MouseAction::MaximizeRestoreWindow => EventAction::MaximizeRestoreWindow,
        MouseAction::NewTab => EventAction::NewTab,
        MouseAction::CloseTab(idx) => EventAction::CloseTab(idx),
        MouseAction::CloseTabWithConfirm(idx) => EventAction::CloseTabWithConfirm(idx),
        MouseAction::SwitchTab(idx) => EventAction::SwitchTab(idx),
        MouseAction::OpenSettings => EventAction::OpenSettings,
        MouseAction::TabReordered => EventAction::TabReordered,
        MouseAction::None => EventAction::None,
    };

    // Check if we need to start text input for tab editing
    let needs_text_input = result.needs_render && tab_bar.editing_tab.is_some();
    if needs_text_input {
        EventResult {
            action: EventAction::StartTextInput,
            needs_render: result.needs_render,
            needs_resize: false,
        }
    } else if matches!(event_action, EventAction::None) {
        EventResult {
            action: EventAction::None,
            needs_render: result.needs_render,
            needs_resize: false,
        }
    } else {
        EventResult {
            action: event_action,
            needs_render: result.needs_render,
            needs_resize: false,
        }
    }
}

fn handle_mouse_button_up_event(
    mouse_btn: sdl3::mouse::MouseButton,
    x: i32,
    y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    let (mouse_x, mouse_y) = if mouse_coords_need_scaling {
        ((x as f32 * scale_factor) as i32, (y as f32 * scale_factor) as i32)
    } else {
        (x, y)
    };

    let (w, h) = mouse_space_window_size(canvas_window);

    let result = super::mouse::handle_mouse_button_up(
        mouse_btn,
        mouse_x,
        mouse_y,
        tab_bar,
        tab_bar_gui,
        tab_bar_height,
        char_width,
        char_height,
        w,
        h,
        mouse_state,
        #[cfg(target_os = "linux")]
        clipboard_tx,
    );

    // Check if we need to resize after divider drag
    let needs_resize = result.needs_render && !mouse_state.dragging_divider;

    EventResult {
        action: EventAction::None,
        needs_render: result.needs_render,
        needs_resize,
    }
}

fn handle_mouse_motion_event(
    x: i32,
    y: i32,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
) -> EventResult {
    let (mouse_x, mouse_y) = if mouse_coords_need_scaling {
        ((x as f32 * scale_factor) as i32, (y as f32 * scale_factor) as i32)
    } else {
        (x, y)
    };

    let (w, h) = mouse_space_window_size(canvas_window);

    let result = super::mouse::handle_mouse_motion(
        mouse_x,
        mouse_y,
        tab_bar,
        tab_bar_gui,
        tab_bar_height,
        char_width,
        char_height,
        w,
        h,
        mouse_state,
    );

    EventResult {
        action: EventAction::None,
        needs_render: result.needs_render,
        needs_resize: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_mouse_wheel_event(
    y: f32,
    x: f32,
    event_mouse_x: f32,
    event_mouse_y: f32,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    mouse_state: &mut MouseState,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    event_pump: &sdl3::EventPump,
    settings: &Settings,
) -> EventResult {
    // Check if Ctrl is pressed for font size change
    let keyboard_state = event_pump.keyboard_state();
    let is_ctrl_pressed =
        keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::LCtrl) || keyboard_state.is_scancode_pressed(sdl3::keyboard::Scancode::RCtrl);

    // If Ctrl is pressed, handle font size change
    if is_ctrl_pressed && y != 0.0 {
        // y > 0 is scroll up (increase font), y < 0 is scroll down (decrease font)
        let delta = if y > 0.0 { 1.0 } else { -1.0 };
        return EventResult {
            action: EventAction::ChangeFontSize(delta),
            needs_render: true,
            needs_resize: true,
        };
    }

    // Use the position carried by the wheel event itself rather than the live pointer
    // state: the latter can be stale (a touchpad two-finger scroll produces no motion
    // events), and a stale position lands outside the pane, so the report is never sent.
    let (mouse_x, mouse_y) = if mouse_coords_need_scaling {
        ((event_mouse_x * scale_factor) as i32, (event_mouse_y * scale_factor) as i32)
    } else {
        (event_mouse_x as i32, event_mouse_y as i32)
    };

    let (w, h) = mouse_space_window_size(canvas_window);

    let result = super::mouse::handle_mouse_wheel(
        y,
        x,
        mouse_x,
        mouse_y,
        tab_bar_gui,
        mouse_state,
        tab_bar_height,
        char_width,
        char_height,
        w,
        h,
        settings.terminal.scroll_multiplier,
    );

    EventResult {
        action: EventAction::None,
        needs_render: result.needs_render,
        needs_resize: false,
    }
}

fn handle_key_down_event(
    keycode: Option<sdl3::keyboard::Keycode>,
    keymod: sdl3::keyboard::Mod,
    scancode: Option<sdl3::keyboard::Scancode>,
    tab_bar: &mut TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    ctrl_keys: &std::collections::HashMap<sdl3::keyboard::Scancode, u8>,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    settings: &Settings,
    mouse_state: &mut MouseState,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    let Some(keycode) = keycode else {
        return EventResult::none();
    };

    let (is_ctrl_pressed, is_shift_pressed, is_alt_pressed) = super::hotkeys::get_modifiers(keymod);

    // Track Ctrl key state for URL hover detection
    use sdl3::keyboard::Keycode;
    if matches!(keycode, Keycode::LCtrl | Keycode::RCtrl) {
        mouse_state.ctrl_pressed = true;
    }

    // Handle tab editing mode
    if tab_bar.editing_tab.is_some() {
        let result = super::keyboard::handle_tab_editing_key(keycode, tab_bar, tab_bar_gui);

        // Check if editing was finished (Return or Escape)
        use sdl3::keyboard::Keycode;
        if matches!(keycode, Keycode::Return | Keycode::Escape) {
            return EventResult {
                action: EventAction::StopTextInput,
                needs_render: result.needs_render,
                needs_resize: false,
            };
        }

        return EventResult {
            action: EventAction::None,
            needs_render: result.needs_render,
            needs_resize: false,
        };
    }

    // Check for sequential navigation hotkey completion from settings
    if let Some(nav_action) = super::hotkeys::match_sequential_navigation_hotkey(keycode, &tab_bar.sequential_hotkey_state, &settings.hotkeys.navigation) {
        // Clear the sequential state since we found a match
        tab_bar.sequential_hotkey_state.clear();

        let result = super::keyboard::handle_hotkey_action(
            super::hotkeys::HotkeyAction::Navigation(nav_action),
            tab_bar_gui,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        );

        return EventResult {
            action: EventAction::None,
            needs_render: result.needs_render,
            needs_resize: result.needs_resize,
        };
    }

    // Check for sequential hotkey completion (second key in a sequence like Alt-G-P)
    if let Some(action) = super::hotkeys::match_sequential_hotkey(keycode, &tab_bar.sequential_hotkey_state)
    {
        // Clear the sequential state since we found a match
        tab_bar.sequential_hotkey_state.clear();

        let result = super::keyboard::handle_hotkey_action(
            action,
            tab_bar_gui,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        );

        return EventResult {
            action: EventAction::None,
            needs_render: result.needs_render,
            needs_resize: result.needs_resize,
        };
    }

    // Check if this key starts a sequential hotkey (settings or hardcoded)
    if super::hotkeys::is_sequential_navigation_hotkey_start(keycode, is_ctrl_pressed, is_shift_pressed, is_alt_pressed, &settings.hotkeys.navigation)
        || super::hotkeys::is_sequential_hotkey_start(keycode, is_ctrl_pressed, is_shift_pressed, is_alt_pressed)
    {
        // Record this as the first key in a potential sequence
        tab_bar
            .sequential_hotkey_state
            .record_first_key(keycode, is_ctrl_pressed, is_shift_pressed, is_alt_pressed);
        // Don't pass this key to the terminal
        return EventResult::none();
    }

    // If we have a sequential state but this key doesn't complete it, clear the state
    if tab_bar.sequential_hotkey_state.is_valid() {
        tab_bar.sequential_hotkey_state.clear();
        // Fall through to handle this key normally
    }

    // For TerminalHistorySearch, check if terminals are grouped OR if a foreground process is running
    // BEFORE matching hotkey. If either is true, we pass Ctrl+R through to the terminal/process
    let is_ctrl_r = keycode == sdl3::keyboard::Keycode::R && is_ctrl_pressed && !is_shift_pressed && !is_alt_pressed;
    let mut should_skip_hotkey_for_ctrl_r = false;

    if is_ctrl_r {
        if let Ok(gui) = tab_bar_gui.lock() {
            let is_not_grouped = gui
                .tab_states
                .get(gui.active_tab)
                .map(|t| t.pane_layout.selected_panes.is_empty())
                .unwrap_or(true);

            // Check if a foreground process (ssh, vim, etc.) is running
            let has_foreground_process = if let Some(terminal) = gui.tab_states.get(gui.active_tab).and_then(|t| t.pane_layout.get_active_terminal()) {
                if let Ok(t) = terminal.lock() {
                    t.is_foreground_process_running()
                } else {
                    false
                }
            } else {
                false
            };

            if !is_not_grouped || has_foreground_process {
                // Terminals are grouped OR a foreground process is running
                // Skip hotkey matching to let key pass to terminal/process
                should_skip_hotkey_for_ctrl_r = true;
            }
        }
    }

    // First check navigation hotkeys from settings (skip Ctrl+R if terminals are grouped)
    if !should_skip_hotkey_for_ctrl_r {
        if let Some(nav_action) =
            super::hotkeys::match_navigation_hotkey(keycode, is_ctrl_pressed, is_shift_pressed, is_alt_pressed, &settings.hotkeys.navigation)
        {
            use super::hotkeys::NavigationAction;

            // Map navigation action to keyboard action
            let keyboard_action = match nav_action {
                NavigationAction::SplitRight => super::keyboard::KeyboardAction::SplitPane(crate::pane_layout::SplitDirection::Vertical),
                NavigationAction::SplitDown => super::keyboard::KeyboardAction::SplitPane(crate::pane_layout::SplitDirection::Horizontal),
                NavigationAction::ClosePane => super::keyboard::KeyboardAction::None, // Will be handled below
                NavigationAction::NextPane | NavigationAction::PreviousPane => super::keyboard::KeyboardAction::None, // Will be handled below
                NavigationAction::NewTab => super::keyboard::KeyboardAction::NewTab,
                NavigationAction::NextTab | NavigationAction::PreviousTab => super::keyboard::KeyboardAction::None, // Will be handled below
                NavigationAction::GoToPrompt => super::keyboard::KeyboardAction::None,                              // Will be handled below
                NavigationAction::TerminalHistorySearch => super::keyboard::KeyboardAction::RequestTerminalHistorySearch,
                NavigationAction::AiCommandGeneration => super::keyboard::KeyboardAction::RequestAiCommandGeneration,
                NavigationAction::VoiceInput => super::keyboard::KeyboardAction::RequestVoiceInput,
            };

            // Handle the action
            let result = super::keyboard::handle_hotkey_action(
                super::hotkeys::HotkeyAction::Navigation(nav_action.clone()),
                tab_bar_gui,
                char_width,
                char_height,
                tab_bar_height,
                canvas_window,
                #[cfg(target_os = "linux")]
                clipboard_tx,
            );

            // Map to event action
            let event_action = match keyboard_action {
                KeyboardAction::NewTab => EventAction::NewTab,
                KeyboardAction::SplitPane(direction) => EventAction::SplitPane(direction),
                KeyboardAction::RequestQuitConfirmation => EventAction::RequestQuitConfirmation,
                KeyboardAction::Quit => EventAction::Quit,
                KeyboardAction::RequestTerminalHistorySearch => EventAction::TerminalHistorySearch,
                KeyboardAction::RequestAiCommandGeneration => EventAction::AiCommandGeneration,
                KeyboardAction::RequestVoiceInput => EventAction::VoiceInput,
                KeyboardAction::TabRenamed => EventAction::TabRenamed,
                KeyboardAction::PaneClosed => EventAction::PaneClosed,
                KeyboardAction::None => EventAction::None,
            };

            return EventResult {
                action: event_action,
                needs_render: result.needs_render,
                needs_resize: result.needs_resize,
            };
        }
    } else {
        eprintln!(
            "[EVENTS] Skipping navigation hotkey check (should_skip_hotkey_for_ctrl_r={})",
            should_skip_hotkey_for_ctrl_r
        );
    }

    // Handle keyboard shortcuts using hotkeys module (hardcoded fallback)
    // Skip if we're passing Ctrl+R through to terminal (grouped terminals)
    if !should_skip_hotkey_for_ctrl_r {
        if let Some(action) = super::hotkeys::match_hotkey(keycode, is_ctrl_pressed, is_shift_pressed) {
            let result = super::keyboard::handle_hotkey_action(
                action,
                tab_bar_gui,
                char_width,
                char_height,
                tab_bar_height,
                canvas_window,
                #[cfg(target_os = "linux")]
                clipboard_tx,
            );

            // Only consume the event if the action was actually handled
            // (i.e., needs_render is true or action is not None)
            // This allows Ctrl+C to pass through to the terminal when there's no selection
            if result.needs_render || !matches!(result.action, KeyboardAction::None) {
                // Map keyboard action to event action
                let event_action = match result.action {
                    KeyboardAction::NewTab => EventAction::NewTab,
                    KeyboardAction::SplitPane(direction) => EventAction::SplitPane(direction),
                    KeyboardAction::RequestQuitConfirmation => EventAction::RequestQuitConfirmation,
                    KeyboardAction::Quit => EventAction::Quit,
                    KeyboardAction::RequestTerminalHistorySearch => EventAction::TerminalHistorySearch,
                    KeyboardAction::RequestAiCommandGeneration => EventAction::AiCommandGeneration,
                    KeyboardAction::RequestVoiceInput => EventAction::VoiceInput,
                    KeyboardAction::TabRenamed => EventAction::TabRenamed,
                    KeyboardAction::PaneClosed => EventAction::PaneClosed,
                    KeyboardAction::None => EventAction::None,
                };

                return EventResult {
                    action: event_action,
                    needs_render: result.needs_render,
                    needs_resize: result.needs_resize,
                };
            }
            // If the hotkey was not consumed (e.g., Ctrl+C with no selection),
            // fall through to send the control character to the terminal
        }
    }

    // Other Ctrl+key combinations
    if is_ctrl_pressed && !is_shift_pressed {
        if let Some(scancode_val) = scancode {
            super::keyboard::handle_ctrl_key(scancode_val, ctrl_keys, tab_bar_gui);
            return EventResult::none();
        }
    }

    // Send normal keys to terminal
    super::keyboard::handle_normal_key(keycode, tab_bar_gui);
    EventResult::none()
}

fn handle_key_up_event(keycode: Option<sdl3::keyboard::Keycode>, mouse_state: &mut MouseState, settings: &Settings) -> EventResult {
    use sdl3::keyboard::Keycode;

    let Some(key) = keycode else {
        return EventResult::none();
    };

    // Detect Ctrl key release and clear URL hover state
    if matches!(key, Keycode::LCtrl | Keycode::RCtrl) {
        mouse_state.ctrl_pressed = false;
        mouse_state.hovered_url = None;
        return EventResult {
            action: EventAction::None,
            needs_render: true,
            needs_resize: false,
        };
    }

    // Check if a "hold" navigation hotkey was released
    if let Some(nav_action) = super::hotkeys::match_hold_release_navigation_hotkey(key, &settings.hotkeys.navigation) {
        use super::hotkeys::NavigationAction;
        let event_action = match nav_action {
            NavigationAction::VoiceInput => EventAction::StopVoiceInput,
            _ => EventAction::None,
        };
        return EventResult {
            action: event_action,
            needs_render: false,
            needs_resize: false,
        };
    }

    EventResult::none()
}

fn handle_text_input_event(text: &str, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> EventResult {
    let result = super::keyboard::handle_text_input(text, tab_bar, tab_bar_gui);
    EventResult {
        action: EventAction::None,
        needs_render: result.needs_render,
        needs_resize: false,
    }
}
