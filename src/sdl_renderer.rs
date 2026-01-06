//! SDL2-based rendering utilities for the terminal emulator
//! Provides font rendering, drawing primitives, and UI elements

use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, TextureCreator};
use sdl2::ttf::Font;
use sdl2::video::Window;

/// Color constants for UI elements
pub const BG_DARK: Color = Color::RGB(30, 30, 30);
pub const BG_MEDIUM: Color = Color::RGB(40, 40, 40);
pub const BG_LIGHT: Color = Color::RGB(50, 50, 50);
pub const TEXT_WHITE: Color = Color::RGB(255, 255, 255);
pub const TEXT_GRAY: Color = Color::RGB(200, 200, 200);

/// Represents a clickable rectangle area
#[derive(Debug, Clone, Copy)]
pub struct ClickableRect {
    pub rect: Rect,
}

impl ClickableRect {
    pub fn new(rect: Rect) -> Self {
        Self { rect }
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.rect.contains_point((x, y))
    }
}

/// Tab bar state and rendering
pub struct TabBar {
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub tab_rects: Vec<ClickableRect>,
    pub close_button_rects: Vec<ClickableRect>,
    pub add_button_rect: ClickableRect,
    pub minimize_button_rect: ClickableRect,
    pub close_button_rect: ClickableRect,
    pub cpu_indicator_rect: ClickableRect,
    pub height: u32,
    pub editing_tab: Option<usize>,
    pub edit_text: String,
    pub mouse_x: i32,
    pub mouse_y: i32,
}

impl TabBar {
    pub fn new(height: u32) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            tab_rects: Vec::new(),
            close_button_rects: Vec::new(),
            add_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            minimize_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            close_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            cpu_indicator_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            height,
            editing_tab: None,
            edit_text: String::new(),
            mouse_x: 0,
            mouse_y: 0,
        }
    }

    pub fn set_tabs(&mut self, tabs: Vec<String>) {
        self.tabs = tabs;
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    pub fn start_editing(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.editing_tab = Some(index);
            self.edit_text = self.tabs[index].clone();
        }
    }

    pub fn finish_editing(&mut self, save: bool) {
        if let Some(idx) = self.editing_tab {
            if save && !self.edit_text.trim().is_empty() {
                self.tabs[idx] = self.edit_text.clone();
            }
        }
        self.editing_tab = None;
        self.edit_text.clear();
    }

    pub fn update_hover(&mut self, mouse_x: i32, mouse_y: i32) {
        // Just store the mouse position, hover will be recalculated on render
        self.mouse_x = mouse_x;
        self.mouse_y = mouse_y;
    }

    pub fn get_clicked_tab(&self, mouse_x: i32, mouse_y: i32) -> Option<usize> {
        for (idx, tab_rect) in self.tab_rects.iter().enumerate() {
            if tab_rect.contains_point(mouse_x, mouse_y) {
                return Some(idx);
            }
        }
        None
    }

    pub fn get_clicked_close_button(&self, mouse_x: i32, mouse_y: i32) -> Option<usize> {
        for (idx, close_rect) in self.close_button_rects.iter().enumerate() {
            if close_rect.contains_point(mouse_x, mouse_y) {
                return Some(idx);
            }
        }
        None
    }

    pub fn render<T>(
        &mut self,
        canvas: &mut Canvas<Window>,
        font: &Font,
        button_font: &Font,
        cpu_font: &Font,
        texture_creator: &TextureCreator<T>,
        window_width: u32,
        cpu_usage: f32,
    ) -> Result<(), String> {
        // Clear tab bar area
        canvas.set_draw_color(BG_DARK);
        canvas.fill_rect(Rect::new(0, 0, window_width, self.height))?;

        let mut x = 4;
        let y = 2;

        // CPU indicator - use smaller font with horizontal padding
        let cpu_text = format!("{:02.0}%", cpu_usage.min(99.0));
        let surface = cpu_font.render(&cpu_text).blended(TEXT_WHITE).map_err(|e| e.to_string())?;
        let texture = texture_creator.create_texture_from_surface(&surface).map_err(|e| e.to_string())?;
        let cpu_width = surface.width() + 16; // Increased horizontal padding
        let cpu_rect = Rect::new(x, y, cpu_width, self.height - 4);

        // Store CPU indicator rect for click detection
        self.cpu_indicator_rect = ClickableRect::new(cpu_rect);

        canvas.set_draw_color(BG_MEDIUM);
        canvas.fill_rect(cpu_rect)?;

        // Vertically center the CPU text
        let text_y = y + ((self.height - 4 - surface.height()) / 2) as i32;
        let text_rect = Rect::new(x + 8, text_y, surface.width(), surface.height()); // Increased left padding
        canvas.copy(&texture, None, Some(text_rect))?;

        x += cpu_width as i32 + 8;

        // Clear old rects
        self.tab_rects.clear();
        self.close_button_rects.clear();

        // Render tabs
        for (idx, tab_name) in self.tabs.iter().enumerate() {
            let is_active = idx == self.active_tab;
            let is_editing = Some(idx) == self.editing_tab;
            let bg_color = if is_editing {
                Color::RGB(50, 80, 120) // Blue-ish tint for editing mode
            } else if is_active {
                BG_LIGHT
            } else {
                BG_DARK
            };

            // Measure text
            let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { tab_name };

            // Try to render text, with fallback for unsupported characters
            let (text_width, text_height, text_texture) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                let width = surface.width();
                let height = surface.height();
                match texture_creator.create_texture_from_surface(&surface) {
                    Ok(tex) => (width, height, Some(tex)),
                    Err(_) => {
                        // Fallback to placeholder if texture creation fails
                        if let Some(fb_surface) = safe_render_text(font, "[Tab]", TEXT_GRAY) {
                            let w = fb_surface.width();
                            let h = fb_surface.height();
                            (w, h, texture_creator.create_texture_from_surface(&fb_surface).ok())
                        } else {
                            (40, 16, None)
                        }
                    }
                }
            } else {
                // Complete fallback if text can't be rendered at all
                (40, 16, None)
            };

            // Calculate tab dimensions
            // We need: left padding (16) + text + spacing (4) + close button (close_size) + right padding (4)
            let close_size = self.height - 8;
            let tab_width = 16 + text_width + close_size + 20; // Increased left padding
            let tab_rect = Rect::new(x, y, tab_width, self.height - 4);

            // Draw tab background
            canvas.set_draw_color(bg_color);
            canvas.fill_rect(tab_rect)?;

            // Draw text (if available) with increased left padding
            if let Some(texture) = text_texture {
                let text_x = x + 16; // Increased left padding
                let text_y = y + ((self.height - 4 - text_height) / 2) as i32;
                let text_rect = Rect::new(text_x, text_y, text_width, text_height);
                let _ = canvas.copy(&texture, None, Some(text_rect));
            }

            // Draw cursor if editing this tab
            if is_editing {
                let cursor_x = x + 16 + text_width as i32 + 2; // Updated for new left padding
                let cursor_y = y + 4;
                let cursor_height = self.height - 8;
                canvas.set_draw_color(Color::RGB(255, 255, 255));
                let _ = canvas.fill_rect(Rect::new(cursor_x, cursor_y, 2, cursor_height));
            }

            // Store clickable areas first
            let tab_clickable = ClickableRect::new(tab_rect);
            self.tab_rects.push(tab_clickable);

            // Draw close button (only visible on hover, but always reserve space)
            let close_x = x + tab_width as i32 - (close_size as i32) - 4;
            let close_y = y + 4;
            let close_rect = Rect::new(close_x, close_y, close_size, close_size);

            // Check if this tab is currently hovered (recalculate based on current mouse position)
            let is_tab_hovered = tab_rect.contains_point((self.mouse_x, self.mouse_y));
            if is_tab_hovered {
                // Try to render close button with larger font, perfectly centered vertically
                if let Some(close_surface) = safe_render_text(button_font, "×", TEXT_WHITE).or_else(|| safe_render_text(button_font, "X", TEXT_WHITE)) {
                    if let Ok(close_texture) = texture_creator.create_texture_from_surface(&close_surface) {
                        let text_x = close_x + ((close_size as i32 - close_surface.width() as i32) / 2);
                        // Perfect vertical centering relative to tab bar full height
                        let available_height = self.height.saturating_sub(close_surface.height());
                        let text_y = (available_height / 2) as i32;
                        let text_rect = Rect::new(text_x, text_y, close_surface.width(), close_surface.height());
                        let _ = canvas.copy(&close_texture, None, Some(text_rect));
                    }
                }
            }
            // Space is always reserved even when not visible to prevent width collapse

            self.close_button_rects.push(ClickableRect::new(close_rect));

            x += tab_width as i32 + 1;
        }

        x += 8;

        // Add button - bigger and perfectly vertically centered
        let add_size = self.height - 8;
        let add_y = y + 4;
        let add_rect = Rect::new(x, add_y, add_size, add_size);

        // Try to render add button with larger font, perfectly vertically centered
        if let Some(add_surface) = safe_render_text(button_font, "+", TEXT_WHITE) {
            if let Ok(add_texture) = texture_creator.create_texture_from_surface(&add_surface) {
                let text_x = x + ((add_size as i32 - add_surface.width() as i32) / 2);
                // Perfect vertical centering relative to tab bar full height, slightly adjusted up
                let available_height = self.height.saturating_sub(add_surface.height());
                let text_y = (available_height / 2) as i32 - 3;
                let text_rect = Rect::new(text_x, text_y, add_surface.width(), add_surface.height());
                let _ = canvas.copy(&add_texture, None, Some(text_rect));
            }
        }
        self.add_button_rect = ClickableRect::new(add_rect);

        // [DEV MODE] indicator (only in non-production builds)
        #[cfg(not(production))]
        let _dev_mode_width = {
            let dev_text = "[DEV MODE]";
            if let Some(dev_surface) = safe_render_text(font, dev_text, Color::RGB(255, 150, 50)) {
                if let Ok(dev_texture) = texture_creator.create_texture_from_surface(&dev_surface) {
                    let dev_width = dev_surface.width();
                    let dev_height = dev_surface.height();
                    // Position to the left of window controls
                    let dev_x = window_width as i32 - dev_width as i32 - 150; // 150px from right for window controls
                    let dev_y = y + ((self.height - 4 - dev_height) / 2) as i32;
                    let dev_rect = Rect::new(dev_x, dev_y, dev_width, dev_height);
                    let _ = canvas.copy(&dev_texture, None, Some(dev_rect));
                    dev_width + 16 // Return width plus some padding
                } else {
                    0
                }
            } else {
                0
            }
        };

        #[cfg(production)]
        let _dev_mode_width = 0;

        // Window controls (right side) - larger and vertically centered
        let button_size = (self.height - 8) as i32;
        let button_y = y + 4;
        let mut right_x = window_width as i32 - button_size - 4;

        // Close window button
        let close_rect = Rect::new(right_x, button_y, button_size as u32, button_size as u32);
        if let Some(close_surface) = safe_render_text(button_font, "×", TEXT_WHITE).or_else(|| safe_render_text(button_font, "X", TEXT_WHITE)) {
            if let Ok(close_texture) = texture_creator.create_texture_from_surface(&close_surface) {
                let text_x = right_x + ((button_size - close_surface.width() as i32) / 2);
                // Center vertically relative to tab bar full height
                let available_height = self.height.saturating_sub(close_surface.height());
                let text_y = (available_height / 2) as i32;
                let text_rect = Rect::new(text_x, text_y, close_surface.width(), close_surface.height());
                let _ = canvas.copy(&close_texture, None, Some(text_rect));
            }
        }
        self.close_button_rect = ClickableRect::new(close_rect);
        right_x -= button_size + 4;

        // Minimize button - draw a custom horizontal line
        let min_rect = Rect::new(right_x, button_y, button_size as u32, button_size as u32);
        canvas.set_draw_color(TEXT_WHITE);
        // Draw a narrow horizontal line positioned slightly below center
        let line_width = (button_size * 5 / 10) as i32; // 50% of button width
        let line_x_start = right_x + (button_size - line_width) / 2;
        let line_y = (self.height * 13 / 20) as i32; // Positioned at 65% from top
        let line_thickness = 2;
        let line_rect = Rect::new(line_x_start, line_y, line_width as u32, line_thickness);
        let _ = canvas.fill_rect(line_rect);
        self.minimize_button_rect = ClickableRect::new(min_rect);

        Ok(())
    }
}

/// Safe text rendering that filters out characters the font can't render
/// Returns None if the text can't be rendered at all
fn safe_render_text(font: &Font, text: &str, color: Color) -> Option<sdl2::surface::Surface<'static>> {
    // First try to render the text as-is
    if let Ok(surface) = font.render(text).blended(color) {
        if surface.width() > 0 && surface.height() > 0 {
            return Some(surface);
        }
    }

    // If that fails, try filtering out unsupported characters
    let filtered: String = text
        .chars()
        .filter(|&ch| {
            // Keep ASCII and common Latin characters
            if ch.is_ascii() || ch as u32 <= 0x024F {
                return true;
            }
            // Test if font can render this character
            if let Ok(test_surface) = font.render_char(ch).blended(color) {
                test_surface.width() > 0 && test_surface.height() > 0
            } else {
                false
            }
        })
        .collect();

    if filtered.is_empty() {
        return None;
    }

    // Try rendering the filtered text
    if let Ok(surface) = font.render(&filtered).blended(color) {
        if surface.width() > 0 && surface.height() > 0 {
            return Some(surface);
        }
    }

    None
}
