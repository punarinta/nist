use sdl3::event::Event;
use std::sync::{Arc, Mutex};

use super::keyboard::KeyboardAction;
use super::mouse::{MouseAction, MouseState};
use crate::sdl_renderer::TabBar;
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
    SwitchTab(usize),
    MinimizeWindow,
    Resize,
    StartTextInput,
    StopTextInput,
    OpenSettings,
    ChangeFontSize(f32),
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
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    match event {
        Event::Quit { .. } => EventResult::quit(),

        Event::Window {
            win_event: sdl3::event::WindowEvent::Resized(_width, _height),
            ..
        } => EventResult::resize(),

        Event::MouseButtonDown { mouse_btn, x, y, .. } => handle_mouse_button_down_event(
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

        Event::MouseWheel { y, x, .. } => handle_mouse_wheel_event(
            *y,
            *x,
            tab_bar_gui,
            scale_factor,
            mouse_coords_need_scaling,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            event_pump,
        ),

        Event::KeyDown { keycode, keymod, scancode, .. } => handle_key_down_event(
            *keycode,
            *keymod,
            *scancode,
            tab_bar,
            tab_bar_gui,
            ctrl_keys,
            scale_factor,
            char_width,
            char_height,
            tab_bar_height,
            canvas_window,
            #[cfg(target_os = "linux")]
            clipboard_tx,
        ),

        Event::TextInput { ref text, .. } => handle_text_input_event(text, tab_bar, tab_bar_gui),

        _ => EventResult::none(),
    }
}

fn handle_mouse_button_down_event(
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

    let (w, h) = canvas_window.size_in_pixels();

    let result = super::mouse::handle_mouse_button_down(
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

    // Map mouse action to event action
    let event_action = match result.action {
        MouseAction::CloseWindow => EventAction::Quit,
        MouseAction::MinimizeWindow => EventAction::MinimizeWindow,
        MouseAction::NewTab => EventAction::NewTab,
        MouseAction::CloseTab(idx) => EventAction::CloseTab(idx),
        MouseAction::SwitchTab(idx) => EventAction::SwitchTab(idx),
        MouseAction::OpenSettings => EventAction::OpenSettings,
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

    let (w, h) = canvas_window.size_in_pixels();

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

    let (w, h) = canvas_window.size_in_pixels();

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

fn handle_mouse_wheel_event(
    y: f32,
    x: f32,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    scale_factor: f32,
    mouse_coords_need_scaling: bool,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    event_pump: &sdl3::EventPump,
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

    let mouse_state_sdl = event_pump.mouse_state();
    let (mouse_x, mouse_y) = if mouse_coords_need_scaling {
        (
            (mouse_state_sdl.x() as f32 * scale_factor) as i32,
            (mouse_state_sdl.y() as f32 * scale_factor) as i32,
        )
    } else {
        (mouse_state_sdl.x() as i32, mouse_state_sdl.y() as i32)
    };

    let (w, h) = canvas_window.size();

    let result = super::mouse::handle_mouse_wheel(y as i32, x as i32, mouse_x, mouse_y, tab_bar_gui, tab_bar_height, char_width, char_height, w, h);

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
    scale_factor: f32,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    canvas_window: &sdl3::video::Window,
    #[cfg(target_os = "linux")] clipboard_tx: &Sender<Clipboard>,
) -> EventResult {
    let Some(keycode) = keycode else {
        return EventResult::none();
    };

    let (is_ctrl_pressed, is_shift_pressed) = super::hotkeys::get_modifiers(keymod);

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

    // Handle keyboard shortcuts using hotkeys module
    if let Some(action) = super::hotkeys::match_hotkey(keycode, is_ctrl_pressed, is_shift_pressed) {
        let result = super::keyboard::handle_hotkey_action(
            action,
            tab_bar_gui,
            scale_factor,
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

fn handle_text_input_event(text: &str, tab_bar: &mut TabBar, tab_bar_gui: &Arc<Mutex<TabBarGui>>) -> EventResult {
    let result = super::keyboard::handle_text_input(text, tab_bar, tab_bar_gui);
    EventResult {
        action: EventAction::None,
        needs_render: result.needs_render,
        needs_resize: false,
    }
}
