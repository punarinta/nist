//! Custom cell renderer for specific block drawing characters
//!
//! This module provides manual rendering for block drawing characters that may not
//! render correctly with fonts, ensuring pixel-perfect display of terminal graphics.

use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::Canvas;
use sdl3::video::Window;

/// Checks if a character can be rendered manually by this module
pub fn can_render_custom(ch: char) -> bool {
    matches!(
        ch,
        '\u{2580}' | // Upper half block ▀
        '\u{2584}' | // Lower half block ▄
        '\u{2588}' | // Full block █
        '\u{258C}' | // Left half block ▌
        '\u{2590}' | // Right half block ▐
        '\u{2598}' | // Quadrant upper left ▘
        '\u{259B}' | // Quadrant upper left and upper right and lower left ▛
        '\u{259C}' | // Quadrant upper left and upper right and lower right ▜
        '\u{259D}' // Quadrant upper right ▝
    )
}

/// Manually renders a block drawing character using SDL primitives
pub fn render_custom_cell(canvas: &mut Canvas<Window>, ch: char, x: i32, y: i32, cell_width: u32, cell_height: u32, r: u8, g: u8, b: u8) -> Result<(), String> {
    // Set the draw color
    canvas.set_draw_color(Color::RGB(r, g, b));

    // Calculate halves with proper remainder handling to avoid gaps
    let half_width = cell_width / 2;
    let half_height = cell_height / 2;
    let second_half_width = cell_width - half_width;
    let second_half_height = cell_height - half_height;

    match ch {
        // Full block █
        '\u{2588}' => {
            let rect = Rect::new(x, y, cell_width, cell_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Upper half block ▀
        '\u{2580}' => {
            let rect = Rect::new(x, y, cell_width, half_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Lower half block ▄
        '\u{2584}' => {
            let rect = Rect::new(x, y + half_height as i32, cell_width, second_half_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Left half block ▌
        '\u{258C}' => {
            let rect = Rect::new(x, y, half_width, cell_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Right half block ▐
        '\u{2590}' => {
            let rect = Rect::new(x + half_width as i32, y, second_half_width, cell_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Quadrant upper left ▘
        '\u{2598}' => {
            let rect = Rect::new(x, y, half_width, half_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Quadrant upper right ▝
        '\u{259D}' => {
            let rect = Rect::new(x + half_width as i32, y, second_half_width, half_height);
            canvas.fill_rect(rect).map_err(|e| e.to_string())?;
        }

        // Quadrant upper left and upper right and lower left ▛
        '\u{259B}' => {
            // Upper left
            let rect1 = Rect::new(x, y, half_width, half_height);
            canvas.fill_rect(rect1).map_err(|e| e.to_string())?;
            // Upper right
            let rect2 = Rect::new(x + half_width as i32, y, second_half_width, half_height);
            canvas.fill_rect(rect2).map_err(|e| e.to_string())?;
            // Lower left
            let rect3 = Rect::new(x, y + half_height as i32, half_width, second_half_height);
            canvas.fill_rect(rect3).map_err(|e| e.to_string())?;
        }

        // Quadrant upper left and upper right and lower right ▜
        '\u{259C}' => {
            // Upper left
            let rect1 = Rect::new(x, y, half_width, half_height);
            canvas.fill_rect(rect1).map_err(|e| e.to_string())?;
            // Upper right
            let rect2 = Rect::new(x + half_width as i32, y, second_half_width, half_height);
            canvas.fill_rect(rect2).map_err(|e| e.to_string())?;
            // Lower right
            let rect3 = Rect::new(x + half_width as i32, y + half_height as i32, second_half_width, second_half_height);
            canvas.fill_rect(rect3).map_err(|e| e.to_string())?;
        }

        _ => {
            return Err(format!("Character {:?} not supported by custom renderer", ch));
        }
    }

    Ok(())
}
