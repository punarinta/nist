//! Custom confirmation dialog with DPI scaling support
//!
//! SDL3's native message box doesn't respect DPI scaling, so we implement
//! our own modal dialog using SDL3 rendering primitives.

use crate::ai::agent::generate_command;
use crate::history;
use crate::settings::Settings;
use crate::terminal::Terminal;
use crate::ui::filtered_list::{FilteredList, ListRow};
use crate::ui::text_input::TextInput;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, Canvas};
use sdl3::ttf::Font;
use sdl3::video::Window;
use sdl3::EventPump;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

const DIALOG_BG: Color = Color::RGB(50, 50, 50);
const DIALOG_BORDER: Color = Color::RGB(100, 100, 100);
const BUTTON_BG: Color = Color::RGB(70, 70, 70);
const BUTTON_HOVER: Color = Color::RGB(90, 90, 90);
const BUTTON_YES: Color = Color::RGB(60, 120, 180);
const BUTTON_YES_HOVER: Color = Color::RGB(80, 140, 200);
const TEXT_COLOR: Color = Color::RGB(255, 255, 255);

/// Wrap text to fit within a maximum width
fn wrap_text(text: &str, font: &Font, max_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        let test_line = if current_line.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current_line, word)
        };

        // Check if test_line fits
        if let Ok(surface) = font.render(&test_line).blended(Color::RGB(255, 255, 255)) {
            if surface.width() <= max_width {
                current_line = test_line;
            } else {
                // Line is too long, push current line and start new one
                if !current_line.is_empty() {
                    lines.push(current_line);
                }
                current_line = word.to_string();
            }
        }
    }

    // Push the last line
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

/// Shows a custom confirmation dialog with Yes/No buttons
/// Returns true if user clicked Yes, false if No or closed the dialog
pub fn show_confirmation_dialog(canvas: &mut Canvas<Window>, event_pump: &mut EventPump, font: &Font, scale_factor: f32, title: &str, message: &str) -> bool {
    let texture_creator = &canvas.texture_creator();

    // Capture current screen content as a texture background
    let (window_width, window_height) = canvas.window().size_in_pixels();
    let background_texture = canvas
        .read_pixels(None)
        .ok()
        .and_then(|surface| texture_creator.create_texture_from_surface(&surface).ok());

    // Calculate text dimensions first to determine required dialog size
    let title_surface = font.render(title).blended(TEXT_COLOR).ok();
    let message_surface = font.render(message).blended(TEXT_COLOR).ok();

    let title_width = title_surface.as_ref().map(|s| s.width()).unwrap_or(0);
    let title_height = title_surface.as_ref().map(|s| s.height()).unwrap_or(0);
    let message_width = message_surface.as_ref().map(|s| s.width()).unwrap_or(0);
    let message_height = message_surface.as_ref().map(|s| s.height()).unwrap_or(0);

    // Dialog dimensions - dynamic based on text content with minimum sizes
    let button_width = (100.0 * scale_factor) as u32;
    let button_height = (35.0 * scale_factor) as u32;
    let padding = (20.0 * scale_factor) as i32;
    let button_spacing = (16.0 * scale_factor) as i32;
    let text_spacing = (12.0 * scale_factor) as i32;

    // Calculate minimum width needed for buttons
    let min_button_area_width = button_width as i32 * 2 + button_spacing + padding * 2;

    // Calculate required width based on text content
    let required_text_width = title_width.max(message_width) as i32 + padding * 2;
    let dialog_width = required_text_width.max(min_button_area_width).max((500.0 * scale_factor) as i32) as u32;

    // Calculate required height based on text content
    let content_height = padding + title_height as i32 + text_spacing + message_height as i32 + text_spacing + button_height as i32 + padding;
    let dialog_height = content_height.max((160.0 * scale_factor) as i32) as u32;

    let dialog_x = (window_width as i32 - dialog_width as i32) / 2;
    let dialog_y = (window_height as i32 - dialog_height as i32) / 2;

    let dialog_rect = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Button positions
    let button_y = dialog_y + dialog_height as i32 - button_height as i32 - padding;
    let total_button_width = button_width as i32 * 2 + button_spacing;
    let buttons_start_x = dialog_x + (dialog_width as i32 - total_button_width) / 2;

    let no_button_rect = Rect::new(buttons_start_x, button_y, button_width, button_height);
    let yes_button_rect = Rect::new(buttons_start_x + button_width as i32 + button_spacing, button_y, button_width, button_height);

    // Detect if mouse coordinates need scaling (same logic as main event loop)
    // Only scale when window size != drawable size (handles platform differences)
    let mouse_coords_need_scaling = scale_factor > 1.0 && {
        let (w_width, _) = canvas.window().size();
        let (d_width, _) = canvas.window().size_in_pixels();
        w_width != d_width
    };

    let mut mouse_pos = (0i32, 0i32);
    let mut result = None;

    // Modal event loop
    while result.is_none() {
        // Process events
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    result = Some(false);
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    result = Some(false);
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    result = Some(true);
                }
                Event::MouseMotion { x, y, .. } => {
                    // Scale mouse coordinates from logical to physical pixels for hit testing
                    // Only scale when window size != drawable size (handles platform differences)
                    mouse_pos = if mouse_coords_need_scaling {
                        ((x * scale_factor) as i32, (y * scale_factor) as i32)
                    } else {
                        (x as i32, y as i32)
                    };
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    // Scale mouse coordinates from logical to physical pixels for hit testing
                    // Only scale when window size != drawable size (handles platform differences)
                    let point = if mouse_coords_need_scaling {
                        ((x * scale_factor) as i32, (y * scale_factor) as i32)
                    } else {
                        (x as i32, y as i32)
                    };
                    if yes_button_rect.contains_point(point) {
                        result = Some(true);
                    } else if no_button_rect.contains_point(point) {
                        result = Some(false);
                    }
                }
                _ => {}
            }
        }

        // Render dialog (clear with fully transparent background)
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 0));
        canvas.clear();

        // Draw captured background (if available) to preserve content behind dialog
        if let Some(ref bg_texture) = background_texture {
            let _ = canvas.copy(bg_texture, None, None);
        }

        // Draw semi-transparent overlay for slight shading (25% opacity)
        // Enable blend mode for transparency to work
        canvas.set_blend_mode(BlendMode::Blend);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 128));
        let overlay_rect = Rect::new(0, 0, window_width, window_height);
        let _ = canvas.fill_rect(overlay_rect);
        canvas.set_blend_mode(BlendMode::None); // Reset blend mode

        // Draw dialog background
        canvas.set_draw_color(DIALOG_BG);
        let _ = canvas.fill_rect(dialog_rect);

        // Draw dialog border
        canvas.set_draw_color(DIALOG_BORDER);
        let _ = canvas.draw_rect(dialog_rect);

        // Draw title
        if let Some(ref title_surf) = title_surface {
            if let Ok(title_texture) = texture_creator.create_texture_from_surface(title_surf) {
                let title_x = dialog_x + (dialog_width as i32 - title_width as i32) / 2;
                let title_y = dialog_y + padding;
                let title_rect = Rect::new(title_x, title_y, title_width, title_height);
                let _ = canvas.copy(&title_texture, None, title_rect);
            }
        }

        // Draw message
        if let Some(ref msg_surf) = message_surface {
            if let Ok(msg_texture) = texture_creator.create_texture_from_surface(msg_surf) {
                let msg_x = dialog_x + (dialog_width as i32 - message_width as i32) / 2;
                let msg_y = dialog_y + padding + title_height as i32 + text_spacing;
                let msg_rect = Rect::new(msg_x, msg_y, message_width, message_height);
                let _ = canvas.copy(&msg_texture, None, msg_rect);
            }
        }

        // Draw No button
        let no_hovered = no_button_rect.contains_point(mouse_pos);
        canvas.set_draw_color(if no_hovered { BUTTON_HOVER } else { BUTTON_BG });
        let _ = canvas.fill_rect(no_button_rect);
        canvas.set_draw_color(DIALOG_BORDER);
        let _ = canvas.draw_rect(no_button_rect);

        if let Ok(no_surface) = font.render("No").blended(TEXT_COLOR) {
            if let Ok(no_texture) = texture_creator.create_texture_from_surface(&no_surface) {
                let text_width = no_surface.width();
                let text_height = no_surface.height();
                let text_x = no_button_rect.x() + (button_width as i32 - text_width as i32) / 2;
                let text_y = no_button_rect.y() + (button_height as i32 - text_height as i32) / 2;
                let text_rect = Rect::new(text_x, text_y, text_width, text_height);
                let _ = canvas.copy(&no_texture, None, text_rect);
            }
        }

        // Draw Yes button
        let yes_hovered = yes_button_rect.contains_point(mouse_pos);
        canvas.set_draw_color(if yes_hovered { BUTTON_YES_HOVER } else { BUTTON_YES });
        let _ = canvas.fill_rect(yes_button_rect);
        canvas.set_draw_color(DIALOG_BORDER);
        let _ = canvas.draw_rect(yes_button_rect);

        if let Ok(yes_surface) = font.render("Yes").blended(TEXT_COLOR) {
            if let Ok(yes_texture) = texture_creator.create_texture_from_surface(&yes_surface) {
                let text_width = yes_surface.width();
                let text_height = yes_surface.height();
                let text_x = yes_button_rect.x() + (button_width as i32 - text_width as i32) / 2;
                let text_y = yes_button_rect.y() + (button_height as i32 - text_height as i32) / 2;
                let text_rect = Rect::new(text_x, text_y, text_width, text_height);
                let _ = canvas.copy(&yes_texture, None, text_rect);
            }
        }

        canvas.present();
    }

    result.unwrap_or(false)
}

/// Shows a confirmation dialog for closing the last tab/pane
pub fn confirm_quit(canvas: &mut Canvas<Window>, event_pump: &mut EventPump, font: &Font, scale_factor: f32) -> bool {
    show_confirmation_dialog(
        canvas,
        event_pump,
        font,
        scale_factor,
        "Sure to close the app?",
        "This is the last terminal. Are you sure you want to quit?",
    )
}

/// Show terminal history search dialog at screen center
/// Returns Ok(()) if user selected an item, Err if cancelled
pub fn terminal_history_search_dialog(
    canvas: &mut Canvas<Window>,
    event_pump: &mut EventPump,
    font: &Font,
    scale_factor: f32,
    terminal_history: Vec<String>,
    terminal: Option<Arc<Mutex<Terminal>>>,
) -> Result<(), String> {
    let texture_creator = &canvas.texture_creator();

    // Capture current screen content as a texture background
    let (_window_width, _window_height) = canvas.window().size_in_pixels();
    let background_texture = canvas
        .read_pixels(None)
        .ok()
        .and_then(|surface| texture_creator.create_texture_from_surface(&surface).ok());

    eprintln!("[DIALOG] Starting terminal history search dialog");
    eprintln!("[DIALOG] Terminal history items: {}", terminal_history.len());

    // 1. Read shell history and combine with terminal history
    let shell_history = history::read_shell_history(1000);
    eprintln!("[DIALOG] Shell history items: {}", shell_history.len());

    // 2. Combine and deduplicate (keep newest first)
    // Use a large limit so we have the full history for filtering
    let combined_history = history::combine_and_deduplicate(
        shell_history,
        terminal_history,
        1000, // max_rows - large enough for full searchable history
    );

    eprintln!("[DIALOG] Combined history items: {}", combined_history.len());
    for (i, cmd) in combined_history.iter().enumerate().take(5) {
        eprintln!("[DIALOG]   [{}]: {}", i, cmd);
    }

    if combined_history.is_empty() {
        eprintln!("[DIALOG] No history available, returning error");
        return Err("No history available".to_string());
    }

    // 3. Calculate dialog dimensions
    let (window_width, window_height) = canvas.window().size_in_pixels();
    // Clamp dialog width between 800px and 80% of screen width, whatever is bigger
    let eighty_percent_width = (window_width as f32 * 0.8) as u32;
    let dialog_width = ((800.0 * scale_factor) as u32).max(eighty_percent_width).min(window_width - 40);
    let max_rows = 8;
    let row_height = (45.0 * scale_factor) as usize;
    let padding = (20.0 * scale_factor) as usize;
    // Height = padding + input row + max_rows list rows + padding
    let dialog_height = ((max_rows + 1) * row_height + padding * 2) as u32;

    let dialog_x = (window_width - dialog_width) / 2;
    let dialog_y = (window_height - dialog_height) / 2;

    eprintln!("[DIALOG] Dialog dimensions: {}x{} at ({}, {})", dialog_width, dialog_height, dialog_x, dialog_y);

    // 4. Create filtered list
    let rows: Vec<ListRow> = combined_history.into_iter().map(|cmd| ListRow::new(cmd)).collect();

    eprintln!("[DIALOG] Creating FilteredList with {} rows", rows.len());
    // Adjust position and size to account for padding
    let list_x = dialog_x as i32 + padding as i32;
    let list_y = dialog_y as i32 + padding as i32;
    let list_width = dialog_width - (padding * 2) as u32;
    let list_height = dialog_height - (padding * 2) as u32;
    let mut filtered_list = FilteredList::new(rows, max_rows, list_width, list_height, scale_factor);
    filtered_list.set_position(list_x, list_y);
    filtered_list.set_focused(true); // Set focus so it can handle input
    eprintln!("[DIALOG] FilteredList created and positioned");

    // 5. Set selection callback to insert command into terminal
    let terminal_clone = terminal;
    filtered_list.set_on_select(Box::new(move |row: &ListRow| {
        eprintln!("[DIALOG] Selection callback fired! Command: {}", row.text);
        if let Some(ref term) = terminal_clone {
            if let Ok(mut t) = term.lock() {
                // Insert command WITHOUT Enter - use send_paste to ensure it's flushed
                eprintln!("[DIALOG] Pasting command to terminal...");
                t.send_paste(&row.text);
                eprintln!("[DIALOG] Command pasted");
            }
        } else {
            eprintln!("[DIALOG] ERROR: No terminal reference!");
        }
    }));

    // 6. Run modal event loop
    let mut result = None;
    'dialog_loop: while result.is_none() {
        // Process events
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    result = Some(Err("Quit requested".to_string()));
                    break 'dialog_loop;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    result = Some(Err("Cancelled".to_string()));
                    break 'dialog_loop;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    // Handle Enter key for selection
                    if filtered_list.handle_event(&event) {
                        // If Return was pressed and callback fired, exit successfully
                        result = Some(Ok(()));
                        break 'dialog_loop;
                    }
                }
                _ => {
                    // Pass other events to filtered list (typing, arrows, etc.)
                    // Don't break on these - they just update the UI
                    filtered_list.handle_event(&event);
                }
            }
        }

        // Render the filtered list (it draws its own background)
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 0));
        canvas.clear();

        // Draw captured background (if available) to preserve content behind dialog
        if let Some(ref bg_texture) = background_texture {
            let _ = canvas.copy(bg_texture, None, None);
        }

        // Draw semi-transparent overlay for slight shading (25% opacity)
        // Enable blend mode for transparency to work
        canvas.set_blend_mode(BlendMode::Blend);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 128));
        let overlay_rect = Rect::new(0, 0, window_width, window_height);
        let _ = canvas.fill_rect(overlay_rect);
        canvas.set_blend_mode(BlendMode::None); // Reset blend mode

        if let Err(e) = filtered_list.render(canvas, font, &canvas.texture_creator()) {
            result = Some(Err(format!("Render error: {}", e)));
            break 'dialog_loop;
        }

        canvas.present();
    }

    result.unwrap_or_else(|| Err("Dialog closed".to_string()))
}

/// Shows an AI command generation dialog with text input, loader, and suggestion display
///
/// Returns Ok(()) if command was accepted and sent to terminal, Err otherwise
pub fn ai_command_dialog(
    canvas: &mut Canvas<Window>,
    event_pump: &mut EventPump,
    font: &Font,
    scale_factor: f32,
    settings: &Settings,
    terminal_history: Vec<String>,
    terminal: Option<Arc<Mutex<Terminal>>>,
) -> Result<(), String> {
    let texture_creator = &canvas.texture_creator();

    // Capture current screen content as background
    let (window_width, window_height) = canvas.window().size_in_pixels();
    let background_texture = canvas
        .read_pixels(None)
        .ok()
        .and_then(|surface| texture_creator.create_texture_from_surface(&surface).ok());

    eprintln!("[AI_DIALOG] Starting AI command generation dialog");

    // Calculate dialog dimensions
    // Clamp dialog width between 800px and 80% of screen width, whatever is bigger
    let eighty_percent_width = (window_width as f32 * 0.8) as u32;
    let dialog_width = ((800.0 * scale_factor) as u32).max(eighty_percent_width).min(window_width - 40);
    let dialog_height = (400.0 * scale_factor) as u32;
    let dialog_x = (window_width - dialog_width) / 2;
    let dialog_y = (window_height - dialog_height) / 2;
    let padding = (20.0 * scale_factor) as i32;

    // Create text input for user prompt
    let input_height = (40.0 * scale_factor) as u32;
    let mut text_input = TextInput::new(dialog_width - (padding * 2) as u32, input_height, scale_factor);
    text_input.set_position(dialog_x as i32 + padding, dialog_y as i32 + padding);
    text_input.set_focused(true);

    // Dialog state
    enum DialogState {
        Input,
        Loading,
        ShowingSuggestion(String),
        Error(String),
    }
    let mut state = DialogState::Input;

    // Channel for receiving async results
    let mut receiver: Option<Receiver<Result<String, String>>> = None;

    // Button dimensions
    let button_width = (100.0 * scale_factor) as u32;
    let button_height = (40.0 * scale_factor) as u32;
    let button_y = dialog_y + dialog_height - button_height as u32 - padding as u32;
    let cancel_button_x = dialog_x + dialog_width - button_width - padding as u32;
    let accept_button_x = cancel_button_x - button_width - (10.0 * scale_factor) as u32;

    let mut mouse_x = 0.0_f32;
    let mut mouse_y = 0.0_f32;

    loop {
        // Check for async results
        if let Some(ref rx) = receiver {
            match rx.try_recv() {
                Ok(Ok(command)) => {
                    eprintln!("[AI_DIALOG] Received generated command: {}", command);
                    state = DialogState::ShowingSuggestion(command);
                    receiver = None;
                }
                Ok(Err(error)) => {
                    eprintln!("[AI_DIALOG] Error generating command: {}", error);
                    state = DialogState::Error(error);
                    receiver = None;
                }
                Err(TryRecvError::Empty) => {
                    // Still waiting
                }
                Err(TryRecvError::Disconnected) => {
                    eprintln!("[AI_DIALOG] Channel disconnected");
                    state = DialogState::Error("Connection lost".to_string());
                    receiver = None;
                }
            }
        }

        // Process events
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    return Err("Quit requested".to_string());
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    return Err("Cancelled".to_string());
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    match state {
                        DialogState::Input | DialogState::Error(_) => {
                            // Both Input and Error states: send the request
                            let prompt = text_input.get_text().trim().to_string();
                            if !prompt.is_empty() {
                                eprintln!("[AI_DIALOG] Generating command for: {}", prompt);
                                state = DialogState::Loading;

                                // Create channel for async communication
                                let (tx, rx) = channel();
                                receiver = Some(rx);

                                // Determine OS name for the prompt
                                let os_name = if cfg!(target_os = "linux") {
                                    "Linux"
                                } else if cfg!(target_os = "macos") {
                                    "macOS"
                                } else if cfg!(target_os = "windows") {
                                    "Windows"
                                } else {
                                    "Unix"
                                };

                                // Spawn async task to generate command
                                let settings_clone = settings.clone();
                                let history_clone = terminal_history.clone();
                                let enhanced_prompt = format!("Generate a {} command: {}", os_name, prompt);
                                let prompt_clone = enhanced_prompt.clone();

                                std::thread::spawn(move || {
                                    let rt = tokio::runtime::Runtime::new().unwrap();
                                    let result = rt.block_on(async {
                                        match generate_command(&settings_clone, &prompt_clone, &history_clone).await {
                                            Ok(cmd) => Ok(cmd),
                                            Err(e) => Err(format!("Failed to generate command: {}", e)),
                                        }
                                    });
                                    let _ = tx.send(result);
                                });
                            }
                        }
                        DialogState::ShowingSuggestion(ref suggested_cmd) => {
                            // Accept the suggestion on Enter
                            eprintln!("[AI_DIALOG] Enter pressed - accepting command");
                            if let Some(ref term) = terminal {
                                if let Ok(mut t) = term.lock() {
                                    t.send_paste(suggested_cmd);
                                    eprintln!("[AI_DIALOG] Command sent via Enter key");
                                }
                            }
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    mouse_x = x;
                    mouse_y = y;
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    if let DialogState::ShowingSuggestion(ref suggested_cmd) = state {
                        // Check Accept button
                        let accept_rect = Rect::new(accept_button_x as i32, button_y as i32, button_width, button_height);
                        if accept_rect.contains_point((x as i32, y as i32)) {
                            eprintln!("[AI_DIALOG] Accept clicked - sending command to terminal");
                            if let Some(ref term) = terminal {
                                if let Ok(mut t) = term.lock() {
                                    t.send_paste(suggested_cmd);
                                    eprintln!("[AI_DIALOG] Command sent");
                                }
                            }
                            return Ok(());
                        }

                        // Check Cancel button
                        let cancel_rect = Rect::new(cancel_button_x as i32, button_y as i32, button_width, button_height);
                        if cancel_rect.contains_point((x as i32, y as i32)) {
                            eprintln!("[AI_DIALOG] Cancel clicked - resetting dialog");
                            // Keep text for editing
                            state = DialogState::Input;
                        }
                    }
                }
                _ => {
                    // Allow text input only in Input and Error states
                    match state {
                        DialogState::Input | DialogState::Error(_) => {
                            text_input.handle_event(&event);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Render
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 0));
        canvas.clear();

        // Draw captured background
        if let Some(ref bg_texture) = background_texture {
            let _ = canvas.copy(bg_texture, None, None);
        }

        // Draw semi-transparent overlay
        canvas.set_blend_mode(BlendMode::Blend);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 128));
        let overlay_rect = Rect::new(0, 0, window_width, window_height);
        let _ = canvas.fill_rect(overlay_rect);

        // Draw dialog background
        canvas.set_draw_color(DIALOG_BG);
        let dialog_rect = Rect::new(dialog_x as i32, dialog_y as i32, dialog_width, dialog_height);
        let _ = canvas.fill_rect(dialog_rect);

        // Draw dialog border
        canvas.set_draw_color(DIALOG_BORDER);
        let _ = canvas.draw_rect(dialog_rect);

        canvas.set_blend_mode(BlendMode::None);

        // Render text input
        if let Err(e) = text_input.render(canvas, font, texture_creator) {
            eprintln!("[AI_DIALOG] Failed to render text input: {}", e);
        }

        // Render state-specific content
        match &state {
            DialogState::Input => {
                // Show centered instructional text
                let help_text = "Describe what you want to do, and I'll suggest a command";

                if let Ok(surface) = font.render(help_text).blended(Color::RGB(150, 150, 150)) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        let query = texture.query();

                        // Calculate the area below the input
                        let content_area_top = dialog_y as i32 + padding + input_height as i32 + padding;
                        let content_area_bottom = dialog_y as i32 + dialog_height as i32 - padding;
                        let content_area_height = content_area_bottom - content_area_top;

                        // Center vertically in the available space, then move up 20px
                        let text_y = content_area_top + (content_area_height - query.height as i32) / 2 - (10.0 * scale_factor) as i32;

                        // Center horizontally in the dialog
                        let text_x = dialog_x as i32 + (dialog_width as i32 - query.width as i32) / 2;

                        let text_rect = Rect::new(text_x, text_y, query.width, query.height);
                        let _ = canvas.copy(&texture, None, text_rect);
                    }
                }
            }
            DialogState::Loading => {
                // Show loading indicator
                let loading_y = dialog_y as i32 + padding + input_height as i32 + padding;
                let loading_text = "Generating command... (this may take a few seconds)";
                if let Ok(surface) = font.render(loading_text).blended(TEXT_COLOR) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        let query = texture.query();
                        let text_x = dialog_x as i32 + padding;
                        let text_rect = Rect::new(text_x, loading_y, query.width, query.height);
                        let _ = canvas.copy(&texture, None, text_rect);
                    }
                }
            }
            DialogState::Error(error_msg) => {
                // Show error message with wrapping
                let error_y = dialog_y as i32 + padding + input_height as i32 + padding;
                let error_text = format!("Error: {}", error_msg);
                let max_text_width = dialog_width - (padding * 2) as u32;
                let line_height = (font.height() as f32 * 1.2) as i32;

                let wrapped_lines = wrap_text(&error_text, font, max_text_width);

                for (i, line) in wrapped_lines.iter().enumerate() {
                    if let Ok(surface) = font.render(line).blended(Color::RGB(255, 100, 100)) {
                        if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                            let query = texture.query();
                            let text_x = dialog_x as i32 + padding;
                            let text_y = error_y + (i as i32 * line_height);
                            let text_rect = Rect::new(text_x, text_y, query.width, query.height);
                            let _ = canvas.copy(&texture, None, text_rect);
                        }
                    }
                }

                // Show hint to press Enter
                let hint_y = error_y + (wrapped_lines.len() as i32 * line_height) + (10.0 * scale_factor) as i32;
                let hint_text = "Press Enter to try again or Escape to close";
                if let Ok(surface) = font.render(hint_text).blended(Color::RGB(150, 150, 150)) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        let query = texture.query();
                        let text_x = dialog_x as i32 + padding;
                        let text_rect = Rect::new(text_x, hint_y, query.width, query.height);
                        let _ = canvas.copy(&texture, None, text_rect);
                    }
                }
            }
            DialogState::ShowingSuggestion(suggested_cmd) => {
                // Show suggested command in a box
                let suggestion_y = dialog_y as i32 + padding + input_height as i32 + padding;
                // Calculate available height between input and buttons (with gap)
                let gap_before_buttons = padding;
                let suggestion_height = (button_y as i32 - suggestion_y - gap_before_buttons).max(50) as u32;

                // Draw suggestion box
                canvas.set_draw_color(Color::RGB(40, 40, 40));
                let suggestion_rect = Rect::new(dialog_x as i32 + padding, suggestion_y, dialog_width - (padding * 2) as u32, suggestion_height);
                let _ = canvas.fill_rect(suggestion_rect);

                canvas.set_draw_color(Color::RGB(80, 80, 80));
                let _ = canvas.draw_rect(suggestion_rect);

                // Wrap and render suggested command text
                let text_x = dialog_x as i32 + padding + (10.0 * scale_factor) as i32;
                let text_y_start = suggestion_y + (10.0 * scale_factor) as i32;
                let max_text_width = dialog_width - (padding * 2) as u32 - (20.0 * scale_factor) as u32;
                let line_height = (font.height() as f32 * 1.2) as i32;

                let wrapped_lines = wrap_text(suggested_cmd, font, max_text_width);

                for (i, line) in wrapped_lines.iter().enumerate() {
                    if let Ok(surface) = font.render(line).blended(Color::RGB(100, 255, 100)) {
                        if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                            let query = texture.query();
                            let text_y = text_y_start + (i as i32 * line_height);
                            let text_rect = Rect::new(text_x, text_y, query.width, query.height);
                            let _ = canvas.copy(&texture, None, text_rect);
                        }
                    }
                }

                // Draw Accept button
                let accept_rect = Rect::new(accept_button_x as i32, button_y as i32, button_width, button_height);
                let accept_hover = accept_rect.contains_point((mouse_x as i32, mouse_y as i32));
                canvas.set_draw_color(if accept_hover { BUTTON_YES_HOVER } else { BUTTON_YES });
                let _ = canvas.fill_rect(accept_rect);
                canvas.set_draw_color(DIALOG_BORDER);
                let _ = canvas.draw_rect(accept_rect);

                // Draw "Accept" text
                if let Ok(surface) = font.render("Accept").blended(TEXT_COLOR) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        let query = texture.query();
                        let text_x = accept_button_x as i32 + (button_width as i32 - query.width as i32) / 2;
                        let text_y = button_y as i32 + (button_height as i32 - query.height as i32) / 2;
                        let text_rect = Rect::new(text_x, text_y, query.width, query.height);
                        let _ = canvas.copy(&texture, None, text_rect);
                    }
                }

                // Draw Cancel button
                let cancel_rect = Rect::new(cancel_button_x as i32, button_y as i32, button_width, button_height);
                let cancel_hover = cancel_rect.contains_point((mouse_x as i32, mouse_y as i32));
                canvas.set_draw_color(if cancel_hover { BUTTON_HOVER } else { BUTTON_BG });
                let _ = canvas.fill_rect(cancel_rect);
                canvas.set_draw_color(DIALOG_BORDER);
                let _ = canvas.draw_rect(cancel_rect);

                // Draw "Clear" text
                if let Ok(surface) = font.render("Clear").blended(TEXT_COLOR) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        let query = texture.query();
                        let text_x = cancel_button_x as i32 + (button_width as i32 - query.width as i32) / 2;
                        let text_y = button_y as i32 + (button_height as i32 - query.height as i32) / 2;
                        let text_rect = Rect::new(text_x, text_y, query.width, query.height);
                        let _ = canvas.copy(&texture, None, text_rect);
                    }
                }
            }
        }

        canvas.present();
    }
}
