//! Text input component with cursor support
//!
//! Provides a text box with editable text and cursor navigation.
//! Handles SDL events directly and reports text changes via a callback.

use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{Canvas, FRect, TextureCreator};
use sdl3::ttf::Font;
use sdl3::video::Window;

/// Color constants for text input
const INPUT_BG: Color = Color::RGB(60, 60, 60);
const INPUT_BORDER: Color = Color::RGB(100, 100, 100);
const INPUT_BORDER_FOCUSED: Color = Color::RGB(120, 150, 200);
const TEXT_COLOR: Color = Color::RGB(255, 255, 255);
const CURSOR_COLOR: Color = Color::RGB(255, 255, 255);

/// Text input component with cursor support
pub struct TextInput {
    text: String,
    cursor_pos: usize,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    focused: bool,
    scale_factor: f32,
    on_change: Option<Box<dyn Fn(&str) + 'static>>,
    on_enter: Option<Box<dyn Fn() + 'static>>,
}

impl TextInput {
    /// Create a new text input component
    pub fn new(width: u32, height: u32, scale_factor: f32) -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            width,
            height,
            x: 0,
            y: 0,
            focused: false,
            scale_factor,
            on_change: None,
            on_enter: None,
        }
    }

    /// Set the position of the text input
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }

    /// Get the current text
    pub fn get_text(&self) -> &str {
        &self.text
    }

    /// Set the focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Get the focus state
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Handle an SDL event
    /// Returns true if the event was consumed by this text input
    pub fn handle_event(&mut self, event: &Event) -> bool {
        if !self.focused {
            return false;
        }

        match event {
            // Handle keyboard input
            Event::KeyDown { keycode, .. } => {
                if let Some(keycode) = keycode {
                    match keycode {
                        Keycode::Left => {
                            self.move_cursor_left();
                            return true;
                        }
                        Keycode::Right => {
                            self.move_cursor_right();
                            return true;
                        }
                        Keycode::Backspace => {
                            self.backspace();
                            return true;
                        }
                        Keycode::Delete => {
                            self.delete();
                            return true;
                        }
                        Keycode::Home => {
                            self.cursor_pos = 0;
                            return true;
                        }
                        Keycode::End => {
                            self.cursor_pos = self.text.len();
                            return true;
                        }
                        Keycode::Return => {
                            // Only consume Return if there's an on_enter callback
                            // Otherwise, let it pass through for parent components to handle
                            if self.on_enter.is_some() {
                                self.fire_on_enter();
                                return true;
                            }
                            // Fall through - don't consume the event
                        }
                        Keycode::Escape => {
                            self.set_focused(false);
                            return true;
                        }
                        _ => {}
                    }
                }
            }

            // Handle text input (actual characters typed)
            Event::TextInput { text, .. } => {
                // Only insert printable characters (not control chars)
                if !text.is_empty() && text.chars().next().map(|c| !c.is_control()).unwrap_or(false) {
                    self.insert_text(text);
                    return true;
                }
            }

            _ => {}
        }

        false
    }

    /// Insert text at cursor position
    pub fn insert_text(&mut self, text: &str) {
        self.text.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.fire_on_change();
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev_char_len = self.text[..self.cursor_pos].chars().last().map_or(0, |c| c.len_utf8());
            self.cursor_pos -= prev_char_len;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.text.len() {
            let next_char_len = self.text[self.cursor_pos..].chars().next().map_or(0, |c| c.len_utf8());
            self.cursor_pos += next_char_len;
        }
    }

    /// Delete character before cursor (backspace)
    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev_char_len = self.text[..self.cursor_pos].chars().last().map_or(0, |c| c.len_utf8());
            let new_cursor_pos = self.cursor_pos - prev_char_len;
            self.text.remove(new_cursor_pos);
            self.cursor_pos = new_cursor_pos;
            self.fire_on_change();
        }
    }

    /// Delete character at cursor
    pub fn delete(&mut self) {
        if self.cursor_pos < self.text.len() {
            if let Some(index) = self.text[self.cursor_pos..].char_indices().next().map(|(i, _)| i) {
                self.text.remove(self.cursor_pos + index);
                self.fire_on_change();
            }
        }
    }

    /// Fire the on_change callback
    fn fire_on_change(&self) {
        if let Some(ref callback) = self.on_change {
            callback(&self.text);
        }
    }

    /// Fire the on_enter callback
    fn fire_on_enter(&self) {
        if let Some(ref callback) = self.on_enter {
            callback();
        }
    }

    /// Render the text input
    pub fn render<T>(&self, canvas: &mut Canvas<Window>, font: &Font, texture_creator: &TextureCreator<T>) -> Result<(), String> {
        let rect = Rect::new(self.x, self.y, self.width, self.height);

        // Draw background
        canvas.set_draw_color(INPUT_BG);
        canvas.fill_rect(rect).map_err(|e| e.to_string())?;

        // Draw border
        canvas.set_draw_color(if self.focused { INPUT_BORDER_FOCUSED } else { INPUT_BORDER });
        canvas.draw_rect(rect).map_err(|e| e.to_string())?;

        // Calculate text rendering position with padding
        let padding = (8.0 * self.scale_factor) as i32;
        let text_x = self.x + padding;
        let text_y = self.y + ((self.height as i32 - (30.0 * self.scale_factor) as i32) / 2); // Center vertically (assuming ~30px font height for UI)

        // Render text with clipping to prevent overflow
        if !self.text.is_empty() {
            if let Ok(surface) = font.render(&self.text).blended(TEXT_COLOR) {
                if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                    // Calculate available width (input width - 2*padding)
                    let available_width = (self.width as i32 - padding * 2) as u32;

                    // Clip text width if it exceeds available width
                    let text_width = surface.width().min(available_width);
                    let text_rect = Rect::new(text_x, text_y, text_width, surface.height());
                    let src_rect = FRect::new(0.0, 0.0, text_width as f32, surface.height() as f32);
                    let _ = canvas.copy(&texture, Some(src_rect), text_rect);
                }
            }
        }

        // Draw cursor if focused
        if self.focused {
            // Calculate cursor position
            let text_before_cursor: String = self.text.chars().take(self.cursor_pos).collect();
            let cursor_offset = if text_before_cursor.is_empty() {
                0
            } else if let Ok(surface) = font.render(&text_before_cursor).blended(TEXT_COLOR) {
                surface.width() as i32
            } else {
                0
            };

            let cursor_x = text_x + cursor_offset;
            let cursor_padding = (4.0 * self.scale_factor) as i32;
            let cursor_width = (2.0 * self.scale_factor) as u32;
            let cursor_rect = Rect::new(cursor_x, self.y + cursor_padding, cursor_width, self.height - (cursor_padding * 2) as u32);
            canvas.set_draw_color(CURSOR_COLOR);
            canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_input_creation() {
        let input = TextInput::new(200, 30, 1.0);
        assert_eq!(input.get_text(), "");
        assert_eq!(input.cursor_pos, 0);
        assert!(!input.is_focused());
    }

    #[test]
    fn test_insert_text() {
        let mut input = TextInput::new(200, 30);
        input.insert_text("Hello");
        assert_eq!(input.get_text(), "Hello");
        assert_eq!(input.cursor_pos, 5);
    }

    #[test]
    fn test_move_cursor() {
        let mut input = TextInput::new(200, 30);
        input.set_text("Hello".to_string());
        input.move_cursor_left();
        assert_eq!(input.cursor_pos, 4);
        input.move_cursor_right();
        assert_eq!(input.cursor_pos, 5);
    }

    #[test]
    fn test_backspace() {
        let mut input = TextInput::new(200, 30);
        input.set_text("Hello".to_string());
        input.backspace();
        assert_eq!(input.get_text(), "Hell");
        assert_eq!(input.cursor_pos, 4);
    }

    #[test]
    fn test_delete() {
        let mut input = TextInput::new(200, 30);
        input.set_text("Hello".to_string());
        input.cursor_pos = 1;
        input.delete();
        assert_eq!(input.get_text(), "Hllo");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn test_contains_point() {
        let mut input = TextInput::new(200, 30);
        input.set_position(100, 50);
        assert!(input.contains_point(150, 65));
        assert!(!input.contains_point(50, 65));
        assert!(!input.contains_point(150, 20));
    }

    #[test]
    fn test_unicode_handling() {
        let mut input = TextInput::new(200, 30);
        input.insert_text("日本語");
        assert_eq!(input.get_text(), "日本語");
        assert_eq!(input.cursor_pos, 9); // 3 chars * 3 bytes each
        input.backspace();
        assert_eq!(input.get_text(), "日本");
        assert_eq!(input.cursor_pos, 6);
    }
}
