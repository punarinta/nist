//! Custom confirmation dialog with DPI scaling support
//!
//! SDL2's native message box doesn't respect DPI scaling, so we implement
//! our own modal dialog using SDL2 rendering primitives.

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::Canvas;
use sdl2::ttf::Font;
use sdl2::video::Window;
use sdl2::EventPump;

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
    let texture_creator = canvas.texture_creator();

    // Scale all dimensions by DPI scale factor
    let dialog_width = (400.0 * scale_factor) as u32;
    let dialog_height = (120.0 * scale_factor) as u32;
    let button_width = (100.0 * scale_factor) as u32;
    let button_height = (30.0 * scale_factor) as u32;
    let padding = (16.0 * scale_factor) as i32;
    let button_spacing = (12.0 * scale_factor) as i32;

    let (window_width, window_height) = canvas.window().size();
    let dialog_x = (window_width as i32 - dialog_width as i32) / 2;
    let dialog_y = (window_height as i32 - dialog_height as i32) / 2;

    let dialog_rect = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Button positions
    let button_y = dialog_y + dialog_height as i32 - button_height as i32 - padding;
    let total_button_width = button_width as i32 * 2 + button_spacing;
    let buttons_start_x = dialog_x + (dialog_width as i32 - total_button_width) / 2;

    let no_button_rect = Rect::new(buttons_start_x, button_y, button_width, button_height);
    let yes_button_rect = Rect::new(buttons_start_x + button_width as i32 + button_spacing, button_y, button_width, button_height);

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
                    mouse_pos = (x, y);
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let point = (x, y);
                    if yes_button_rect.contains_point(point) {
                        result = Some(true);
                    } else if no_button_rect.contains_point(point) {
                        result = Some(false);
                    }
                }
                _ => {}
            }
        }

        // Render dialog
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 180));
        canvas.clear();

        // Draw dialog background
        canvas.set_draw_color(DIALOG_BG);
        let _ = canvas.fill_rect(dialog_rect);

        // Draw dialog border
        canvas.set_draw_color(DIALOG_BORDER);
        let _ = canvas.draw_rect(dialog_rect);

        // Draw title
        if let Ok(title_surface) = font.render(title).blended(TEXT_COLOR) {
            if let Ok(title_texture) = texture_creator.create_texture_from_surface(&title_surface) {
                let title_width = title_surface.width();
                let title_height = title_surface.height();
                let title_x = dialog_x + (dialog_width as i32 - title_width as i32) / 2;
                let title_y = dialog_y + padding;
                let title_rect = Rect::new(title_x, title_y, title_width, title_height);
                let _ = canvas.copy(&title_texture, None, Some(title_rect));
            }
        }

        // Draw message
        let message_y = dialog_y + padding * 2;
        if let Ok(msg_surface) = font.render(message).blended(TEXT_COLOR) {
            if let Ok(msg_texture) = texture_creator.create_texture_from_surface(&msg_surface) {
                let msg_width = msg_surface.width();
                let msg_height = msg_surface.height();
                let msg_x = dialog_x + (dialog_width as i32 - msg_width as i32) / 2;
                let msg_rect = Rect::new(msg_x, message_y, msg_width, msg_height);
                let _ = canvas.copy(&msg_texture, None, Some(msg_rect));
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
                let _ = canvas.copy(&no_texture, None, Some(text_rect));
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
                let _ = canvas.copy(&yes_texture, None, Some(text_rect));
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
