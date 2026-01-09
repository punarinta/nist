//! SDL3-based rendering utilities for the terminal emulator
//! Provides font rendering, drawing primitives, and UI elements

use crate::input::hotkeys::SequentialHotkeyState;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{Canvas, TextureCreator};
use sdl3::ttf::Font;
use sdl3::video::Window;

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
    pub dragging_tab: Option<usize>,
    pub drag_start_x: i32,
    pub drag_offset_x: i32,
    pub sequential_hotkey_state: SequentialHotkeyState,
    pub first_visible_tab_index: usize,
    pub left_scroll_button_rect: ClickableRect,
    pub right_scroll_button_rect: ClickableRect,
}

impl TabBar {
    pub fn new(height: u32) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            tab_rects: Vec::new(),
            close_button_rects: Vec::new(),
            add_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            sequential_hotkey_state: SequentialHotkeyState::new(),
            minimize_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            close_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            cpu_indicator_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            height,
            editing_tab: None,
            edit_text: String::new(),
            mouse_x: 0,
            mouse_y: 0,
            dragging_tab: None,
            drag_start_x: 0,
            drag_offset_x: 0,
            first_visible_tab_index: 0,
            left_scroll_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
            right_scroll_button_rect: ClickableRect::new(Rect::new(0, 0, 0, 0)),
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

    pub fn start_dragging_tab(&mut self, tab_index: usize, mouse_x: i32) {
        if tab_index < self.tab_rects.len() {
            self.dragging_tab = Some(tab_index);
            self.drag_start_x = mouse_x;
            self.drag_offset_x = 0;
        }
    }

    pub fn update_drag(&mut self, mouse_x: i32) {
        if self.dragging_tab.is_some() {
            self.drag_offset_x = mouse_x - self.drag_start_x;
        }
    }

    pub fn stop_dragging_tab(&mut self) -> Option<(usize, usize)> {
        if let Some(dragging_idx) = self.dragging_tab {
            let result = self.calculate_drop_position(dragging_idx);
            self.dragging_tab = None;
            self.drag_offset_x = 0;
            self.drag_start_x = 0;
            result
        } else {
            None
        }
    }

    fn calculate_drop_position(&self, dragging_idx: usize) -> Option<(usize, usize)> {
        if dragging_idx >= self.tab_rects.len() {
            return None;
        }

        let dragged_rect = self.tab_rects[dragging_idx].rect;
        let dragged_center_x = dragged_rect.x() + self.drag_offset_x + (dragged_rect.width() as i32 / 2);

        // Find which tab position this corresponds to
        for (idx, tab_rect) in self.tab_rects.iter().enumerate() {
            if idx == dragging_idx {
                continue;
            }

            let tab_center_x = tab_rect.rect.x() + (tab_rect.rect.width() as i32 / 2);

            // If dragged tab center is past this tab's center, we should swap
            if (dragging_idx < idx && dragged_center_x > tab_center_x) || (dragging_idx > idx && dragged_center_x < tab_center_x) {
                return Some((dragging_idx, idx));
            }
        }

        None
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

    pub fn scroll_left(&mut self) {
        // Scroll left by one tab (move to previous tab)
        if self.first_visible_tab_index > 0 {
            self.first_visible_tab_index -= 1;
        }
    }

    pub fn scroll_right(&mut self) {
        // Scroll right by one tab (move to next tab)
        if self.first_visible_tab_index + 1 < self.tabs.len() {
            self.first_visible_tab_index += 1;
        }
    }

    pub fn render<T>(
        &mut self,
        canvas: &mut Canvas<Window>,
        font: &Font,
        _button_font: &Font,
        cpu_font: &Font,
        texture_creator: &TextureCreator<T>,
        window_width: u32,
        cpu_usage: f32,
    ) -> Result<(), String> {
        // Clear tab bar area
        canvas.set_draw_color(BG_DARK);
        canvas.fill_rect(Rect::new(0, 0, window_width, self.height)).map_err(|e| e.to_string())?;

        let mut x = 6;
        let y = 3;

        // CPU indicator - use smaller font with horizontal padding
        let cpu_text = format!("{:02.0}%", cpu_usage.min(99.0));
        let surface = cpu_font.render(&cpu_text).blended(TEXT_WHITE).map_err(|e| e.to_string())?;
        let texture = texture_creator.create_texture_from_surface(&surface).map_err(|e| e.to_string())?;
        let cpu_width = 70; // Fixed width to prevent jumping when numbers change
        let cpu_rect = Rect::new(x, y, cpu_width, self.height - 6);

        // Store CPU indicator rect for click detection
        self.cpu_indicator_rect = ClickableRect::new(cpu_rect);

        canvas.set_draw_color(BG_MEDIUM);
        canvas.fill_rect(cpu_rect).map_err(|e| e.to_string())?;

        // Vertically center the CPU text
        let text_y = y + ((self.height - 6 - surface.height()) / 2) as i32;
        let text_rect = Rect::new(x + 12, text_y, surface.width(), surface.height()); // Increased left padding
        canvas.copy(&texture, None, text_rect).map_err(|e| e.to_string())?;

        x += cpu_width as i32 + 12;

        // Calculate available space for tabs
        // Reserve space for: add button + dev mode indicator + window controls
        let button_size = (self.height - 12) as i32;
        let window_controls_width = (button_size + 6) * 2 + 6; // Two buttons + spacing
        #[cfg(not(production))]
        let dev_mode_width = 150; // Approximate width for "[DEV MODE]"
        #[cfg(production)]
        let dev_mode_width = 0;
        // Adjust reserved space based on whether dev mode indicator is present
        #[cfg(not(production))]
        let right_reserved_space = window_controls_width + dev_mode_width + 180; // More padding when dev mode present
        #[cfg(production)]
        let right_reserved_space = window_controls_width + 80; // Less padding in production
        let add_button_width = button_size + 24;

        // Calculate total tabs width (without scroll offset)
        let mut total_tabs_width = 0i32;
        for (idx, tab_name) in self.tabs.iter().enumerate() {
            let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { tab_name };
            let (text_width, _, _) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                (surface.width(), surface.height(), Some(()))
            } else {
                (40, 16, Some(()))
            };
            let close_size = self.height - 12;
            let tab_width = 24 + text_width + close_size + 30;
            total_tabs_width += tab_width as i32 + 1;
        }

        // Determine if we need scroll buttons
        let available_width_for_tabs = window_width as i32 - x - add_button_width - right_reserved_space;
        let scroll_button_size = 30u32; // Fixed smaller size for scroll buttons
        let needs_scrolling = total_tabs_width > available_width_for_tabs;

        // Clamp first_visible_tab_index to valid range
        if self.first_visible_tab_index >= self.tabs.len() {
            self.first_visible_tab_index = self.tabs.len().saturating_sub(1);
        }

        // Prevent overscrolling: ensure we don't scroll past the point where all remaining tabs fit
        if needs_scrolling {
            // Calculate width of tabs starting from first_visible_tab_index
            let mut visible_width = 0i32;
            let available_for_visible = available_width_for_tabs - (scroll_button_size as i32 * 2) - 12;

            for idx in self.first_visible_tab_index..self.tabs.len() {
                let tab_name = &self.tabs[idx];
                let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { tab_name };
                let (text_width, _, _) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                    (surface.width(), surface.height(), Some(()))
                } else {
                    (40, 16, Some(()))
                };
                let close_size = self.height - 12;
                let tab_width = 24 + text_width + close_size + 30;
                visible_width += tab_width as i32 + 1;
            }

            // If all remaining tabs fit, move first_visible_tab_index back
            while self.first_visible_tab_index > 0 && visible_width <= available_for_visible {
                // Try including the previous tab
                self.first_visible_tab_index -= 1;
                let idx = self.first_visible_tab_index;
                let tab_name = &self.tabs[idx];
                let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { tab_name };
                let (text_width, _, _) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                    (surface.width(), surface.height(), Some(()))
                } else {
                    (40, 16, Some(()))
                };
                let close_size = self.height - 12;
                let tab_width = 24 + text_width + close_size + 30;
                visible_width += tab_width as i32 + 1;

                // If it doesn't fit, undo
                if visible_width > available_for_visible {
                    self.first_visible_tab_index += 1;
                    break;
                }
            }
        }

        let mut tabs_start_x = x;
        let mut tabs_end_x = x + available_width_for_tabs;

        // Draw left scroll button if needed
        if needs_scrolling {
            // Vertically center the scroll button
            let scroll_btn_y = y + ((self.height - 6 - scroll_button_size) / 2) as i32;
            let left_scroll_rect = Rect::new(x, scroll_btn_y, scroll_button_size, scroll_button_size);
            self.left_scroll_button_rect = ClickableRect::new(left_scroll_rect);

            // Draw background
            canvas.set_draw_color(BG_MEDIUM);
            canvas.fill_rect(left_scroll_rect).map_err(|e| e.to_string())?;

            // Draw "<" using SDL primitives (left-pointing chevron)
            canvas.set_draw_color(TEXT_WHITE);
            let center_x = x + (scroll_button_size as i32 / 2);
            let center_y = scroll_btn_y + (scroll_button_size as i32 / 2);
            let size = scroll_button_size as i32 / 4; // Smaller icon

            // Draw two lines forming "<"
            for i in 0..2 {
                // Top line of chevron (pointing left)
                let _ = canvas.draw_line((center_x + size / 2, center_y - size + i), (center_x - size / 2, center_y + i));
                // Bottom line of chevron (pointing left)
                let _ = canvas.draw_line((center_x - size / 2, center_y + i), (center_x + size / 2, center_y + size + i));
            }

            x += scroll_button_size as i32 + 6;
            tabs_start_x = x; // Update tabs_start_x to prevent overlap with scroll button
            tabs_end_x = x + available_width_for_tabs - scroll_button_size as i32 - 6 - scroll_button_size as i32 - 6;
        }

        // Clear old rects
        self.tab_rects.clear();
        self.close_button_rects.clear();

        // Render tabs (in two passes: non-dragged tabs first, then dragged tab on top)
        // Render from first_visible_tab_index (no pixel offset)
        let mut dragged_tab_data: Option<(usize, String, i32, u32)> = None;

        for (idx, tab_name) in self.tabs.iter().enumerate() {
            // Skip tabs before first_visible_tab_index (unless being dragged)
            if idx < self.first_visible_tab_index && Some(idx) != self.dragging_tab {
                // Still need to store empty rects for proper indexing
                self.tab_rects.push(ClickableRect::new(Rect::new(-1000, -1000, 0, 0)));
                self.close_button_rects.push(ClickableRect::new(Rect::new(-1000, -1000, 0, 0)));
                continue;
            }

            let close_size = self.height - 12;
            let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { tab_name };
            let (text_width, _, _) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                (surface.width(), surface.height(), Some(()))
            } else {
                (40, 16, Some(()))
            };
            let tab_width = 24 + text_width + close_size + 30;

            // If this tab is being dragged, save it for later rendering
            if Some(idx) == self.dragging_tab {
                dragged_tab_data = Some((idx, tab_name.clone(), x, tab_width));

                // Still need to advance x and store rect for drop position calculation
                let tab_rect = Rect::new(x, y, tab_width, self.height - 6);
                self.tab_rects.push(ClickableRect::new(tab_rect));
                let close_x = x + tab_width as i32 - (close_size as i32) - 6;
                let close_y = y + 6;
                let close_rect = Rect::new(close_x, close_y, close_size, close_size);
                self.close_button_rects.push(ClickableRect::new(close_rect));
                x += tab_width as i32 + 1;
                continue;
            }

            // Check if tab is completely out of view or partially clipped
            let tab_right = x + tab_width as i32;
            let tab_out_of_view = (tab_right < tabs_start_x) || (x > tabs_end_x);
            let tab_clipped_left = x < tabs_start_x && tab_right > tabs_start_x;
            let tab_clipped_right = x < tabs_end_x && tab_right > tabs_end_x;

            if tab_out_of_view || (needs_scrolling && (tab_clipped_left || tab_clipped_right)) {
                // Still need to store rect and advance x for proper positioning
                let tab_rect = Rect::new(x, y, tab_width, self.height - 6);
                self.tab_rects.push(ClickableRect::new(tab_rect));
                let close_x = x + tab_width as i32 - (close_size as i32) - 6;
                let close_y = y + 6;
                let close_rect = Rect::new(close_x, close_y, close_size, close_size);
                self.close_button_rects.push(ClickableRect::new(close_rect));
                x += tab_width as i32 + 1;
                continue;
            }

            let is_active = idx == self.active_tab;
            let is_editing = Some(idx) == self.editing_tab;
            let bg_color = if is_editing {
                Color::RGB(50, 80, 120) // Blue-ish tint for editing mode
            } else if is_active {
                BG_LIGHT
            } else {
                BG_DARK
            };

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
            let tab_rect = Rect::new(x, y, tab_width, self.height - 6);

            // Draw tab background
            canvas.set_draw_color(bg_color);
            canvas.fill_rect(tab_rect).map_err(|e| e.to_string())?;

            // Draw text (if available) with increased left padding
            if let Some(texture) = text_texture {
                let text_x = x + 24; // Increased left padding
                let text_y = y + ((self.height - 6 - text_height) / 2) as i32;
                let text_rect = Rect::new(text_x, text_y, text_width, text_height);
                let _ = canvas.copy(&texture, None, text_rect);
            }

            // Draw cursor if editing this tab
            if is_editing {
                let cursor_x = x + 24 + text_width as i32 + 3; // Updated for new left padding
                let cursor_y = y + 6;
                let cursor_height = self.height - 12;
                canvas.set_draw_color(Color::RGB(255, 255, 255));
                let _ = canvas.fill_rect(Rect::new(cursor_x, cursor_y, 2, cursor_height));
            }

            // Store clickable areas first
            let tab_clickable = ClickableRect::new(tab_rect);
            self.tab_rects.push(tab_clickable);

            // Draw close button (only visible on hover, but always reserve space)
            let close_x = x + tab_width as i32 - (close_size as i32) - 6;
            let close_y = y + 6;
            let close_rect = Rect::new(close_x, close_y, close_size, close_size);

            // Check if this tab is currently hovered (recalculate based on current mouse position)
            let is_tab_hovered = tab_rect.contains_point((self.mouse_x, self.mouse_y));
            if is_tab_hovered {
                // Draw close button "×" manually with SDL primitives
                canvas.set_draw_color(TEXT_WHITE);
                let center_x = close_x + (close_size as i32 / 2);
                let center_y = close_y + (close_size as i32 / 2);
                let half_size = close_size as i32 * 4 / 10; // 40% of button size

                // Draw X as two diagonal lines
                // Top-left to bottom-right
                for i in 0..3 {
                    let _ = canvas.draw_line(
                        (center_x - half_size / 2 + i, center_y - half_size / 2),
                        (center_x + half_size / 2 + i, center_y + half_size / 2),
                    );
                }
                // Top-right to bottom-left
                for i in 0..3 {
                    let _ = canvas.draw_line(
                        (center_x + half_size / 2 + i, center_y - half_size / 2),
                        (center_x - half_size / 2 + i, center_y + half_size / 2),
                    );
                }
            }
            // Space is always reserved even when not visible to prevent width collapse

            self.close_button_rects.push(ClickableRect::new(close_rect));

            x += tab_width as i32 + 1;
        }

        // Now render the dragged tab on top with visual feedback
        if let Some((idx, tab_name, original_x, tab_width)) = dragged_tab_data {
            let dragged_x = original_x + self.drag_offset_x;
            let is_active = idx == self.active_tab;
            let is_editing = Some(idx) == self.editing_tab;

            // Elevated appearance with slightly different color
            let bg_color = if is_editing {
                Color::RGB(60, 90, 140) // Brighter blue for dragging
            } else if is_active {
                Color::RGB(60, 60, 60) // Brighter for active
            } else {
                Color::RGB(40, 40, 40) // Slightly brighter
            };

            let display_text = if Some(idx) == self.editing_tab { &self.edit_text } else { &tab_name };

            // Render text
            let (text_width, text_height, text_texture) = if let Some(surface) = safe_render_text(font, display_text, TEXT_GRAY) {
                let width = surface.width();
                let height = surface.height();
                match texture_creator.create_texture_from_surface(&surface) {
                    Ok(tex) => (width, height, Some(tex)),
                    Err(_) => {
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
                (40, 16, None)
            };

            // Draw shadow effect (slightly offset darker rectangle)
            let shadow_rect = Rect::new(dragged_x + 2, y + 2, tab_width, self.height - 6);
            canvas.set_draw_color(Color::RGBA(0, 0, 0, 100));
            let _ = canvas.fill_rect(shadow_rect);

            // Draw dragged tab
            let tab_rect = Rect::new(dragged_x, y, tab_width, self.height - 6);
            canvas.set_draw_color(bg_color);
            canvas.fill_rect(tab_rect).map_err(|e| e.to_string())?;

            // Draw border to make it stand out
            canvas.set_draw_color(Color::RGB(100, 100, 100));
            let _ = canvas.draw_rect(tab_rect);

            // Draw text
            if let Some(texture) = text_texture {
                let text_x = dragged_x + 24;
                let text_y = y + ((self.height - 6 - text_height) / 2) as i32;
                let text_rect = Rect::new(text_x, text_y, text_width, text_height);
                let _ = canvas.copy(&texture, None, text_rect);
            }

            // Draw cursor if editing
            if is_editing {
                let cursor_x = dragged_x + 24 + text_width as i32 + 3;
                let cursor_y = y + 6;
                let cursor_height = self.height - 12;
                canvas.set_draw_color(Color::RGB(255, 255, 255));
                let _ = canvas.fill_rect(Rect::new(cursor_x, cursor_y, 2, cursor_height));
            }
        }

        // Draw right scroll button if needed
        if needs_scrolling {
            x = tabs_end_x + 6;
            // Vertically center the scroll button
            let scroll_btn_y = y + ((self.height - 6 - scroll_button_size) / 2) as i32;
            let right_scroll_rect = Rect::new(x, scroll_btn_y, scroll_button_size, scroll_button_size);
            self.right_scroll_button_rect = ClickableRect::new(right_scroll_rect);

            // Draw background
            canvas.set_draw_color(BG_MEDIUM);
            canvas.fill_rect(right_scroll_rect).map_err(|e| e.to_string())?;

            // Draw ">" using SDL primitives (right-pointing chevron)
            canvas.set_draw_color(TEXT_WHITE);
            let center_x = x + (scroll_button_size as i32 / 2);
            let center_y = scroll_btn_y + (scroll_button_size as i32 / 2);
            let size = scroll_button_size as i32 / 4; // Smaller icon

            // Draw two lines forming ">"
            for i in 0..2 {
                // Top line of chevron
                let _ = canvas.draw_line((center_x - size / 2, center_y - size + i), (center_x + size / 2, center_y + i));
                // Bottom line of chevron
                let _ = canvas.draw_line((center_x - size / 2, center_y + size + i), (center_x + size / 2, center_y + i));
            }

            x += scroll_button_size as i32 + 12;
        } else {
            // Reset scroll button rects when not needed
            self.left_scroll_button_rect = ClickableRect::new(Rect::new(0, 0, 0, 0));
            self.right_scroll_button_rect = ClickableRect::new(Rect::new(0, 0, 0, 0));
            x += 12;
        }

        // Add button - bigger and perfectly vertically centered
        let add_size = self.height - 12;
        let add_y = y + 6;
        let add_rect = Rect::new(x, add_y, add_size, add_size);

        // Draw add button "+" manually with SDL primitives
        canvas.set_draw_color(TEXT_WHITE);
        let center_x = x + (add_size as i32 / 2);
        let center_y = y + (self.height as i32 / 2) - 3;
        let half_size = add_size as i32 * 4 / 10; // 40% of button size
        let thickness = 3;

        // Draw horizontal line
        for i in 0..thickness {
            let _ = canvas.draw_line(
                (center_x - half_size / 2, center_y + i - thickness / 2),
                (center_x + half_size / 2, center_y + i - thickness / 2),
            );
        }
        // Draw vertical line
        for i in 0..thickness {
            let _ = canvas.draw_line(
                (center_x + i - thickness / 2, center_y - half_size / 2),
                (center_x + i - thickness / 2, center_y + half_size / 2),
            );
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
                    let dev_x = window_width as i32 - dev_width as i32 - 225; // 225px from right for window controls
                    let dev_y = y + ((self.height - 6 - dev_height) / 2) as i32;
                    let dev_rect = Rect::new(dev_x, dev_y, dev_width, dev_height);
                    let _ = canvas.copy(&dev_texture, None, dev_rect);
                    dev_width + 24 // Return width plus some padding
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
        let button_size = (self.height - 12) as i32;
        let button_y = y + 6;
        let mut right_x = window_width as i32 - button_size - 6;

        // Close window button - draw "×" manually with SDL primitives
        let close_rect = Rect::new(right_x, button_y, button_size as u32, button_size as u32);
        canvas.set_draw_color(TEXT_WHITE);
        let center_x = right_x + (button_size / 2);
        let center_y = button_y + (button_size / 2);
        let half_size = button_size * 4 / 10; // 40% of button size

        // Draw X as two diagonal lines
        // Top-left to bottom-right
        for i in 0..3 {
            let _ = canvas.draw_line(
                (center_x - half_size / 2 + i, center_y - half_size / 2),
                (center_x + half_size / 2 + i, center_y + half_size / 2),
            );
        }
        // Top-right to bottom-left
        for i in 0..3 {
            let _ = canvas.draw_line(
                (center_x + half_size / 2 + i, center_y - half_size / 2),
                (center_x - half_size / 2 + i, center_y + half_size / 2),
            );
        }
        self.close_button_rect = ClickableRect::new(close_rect);
        right_x -= button_size + 6;

        // Minimize button - draw a custom horizontal line
        let min_rect = Rect::new(right_x, button_y, button_size as u32, button_size as u32);
        canvas.set_draw_color(TEXT_WHITE);
        // Draw a narrow horizontal line positioned slightly below center
        let line_width = button_size * 5 / 10; // 50% of button width
        let line_x_start = right_x + (button_size - line_width) / 2;
        let line_y = (self.height * 13 / 20) as i32; // Positioned at 65% from top
        let line_thickness = 2;
        let line_rect = Rect::new(line_x_start, line_y, line_width as u32, line_thickness);
        let _ = canvas.fill_rect(line_rect);
        self.minimize_button_rect = ClickableRect::new(min_rect);

        // Draw bottom border for tab bar (matching pane navigation bar style)
        canvas.set_draw_color(Color::RGB(60, 60, 60));
        let border_y = self.height as i32 - 1;
        canvas.draw_line((0, border_y), (window_width as i32, border_y)).map_err(|e| e.to_string())?;

        Ok(())
    }
}

/// Safe text rendering that filters out characters the font can't render
/// Returns None if the text can't be rendered at all
fn safe_render_text(font: &Font, text: &str, color: Color) -> Option<sdl3::surface::Surface<'static>> {
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
