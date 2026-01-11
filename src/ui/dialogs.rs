//! Custom confirmation dialog with DPI scaling support
//!
//! SDL3's native message box doesn't respect DPI scaling, so we implement
//! our own modal dialog using SDL3 rendering primitives.

use crate::history;
use crate::terminal::Terminal;
use crate::ui::filtered_list::{FilteredList, ListRow};
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, Canvas};
use sdl3::ttf::Font;
use sdl3::video::Window;
use sdl3::EventPump;
use std::sync::{Arc, Mutex};

const DIALOG_BG: Color = Color::RGB(50, 50, 50);
const DIALOG_BORDER: Color = Color::RGB(100, 100, 100);
const BUTTON_BG: Color = Color::RGB(70, 70, 70);
const BUTTON_HOVER: Color = Color::RGB(90, 90, 90);
const BUTTON_YES: Color = Color::RGB(60, 120, 180);
const BUTTON_YES_HOVER: Color = Color::RGB(80, 140, 200);
const TEXT_COLOR: Color = Color::RGB(255, 255, 255);

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
    let (window_width, window_height) = canvas.window().size_in_pixels();
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
