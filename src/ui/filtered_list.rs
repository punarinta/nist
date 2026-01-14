//! Filtered list component with text input filtering
//!
//! Provides a list with a text input filter at the top.
//! Supports keyboard navigation (up/down) and selection via Enter.

use crate::ui::text_input::TextInput;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{Canvas, FRect, TextureCreator};
use sdl3::ttf::Font;
use sdl3::video::Window;

/// Color constants for filtered list
const LIST_BG: Color = Color::RGB(40, 40, 40);
const ROW_BG: Color = Color::RGB(50, 50, 50);
const ROW_HIGHLIGHT: Color = Color::RGB(70, 100, 140);
const ROW_BORDER: Color = Color::RGB(60, 60, 60);
const TEXT_COLOR: Color = Color::RGB(255, 255, 255);

/// A row in the filtered list
#[derive(Clone, Debug)]
pub struct ListRow {
    /// Display text for the row
    pub text: String,
}

impl ListRow {
    /// Create a new list row
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// Filtered list component
pub struct FilteredList {
    /// Text input for filtering
    text_input: TextInput,
    /// All rows in the list
    all_rows: Vec<ListRow>,
    /// Currently filtered rows
    filtered_rows: Vec<ListRow>,
    /// Indices of filtered rows in the original all_rows vector
    filtered_indices: Vec<usize>,
    /// Maximum number of items to display
    max_items: usize,
    /// Currently selected index (index into filtered_rows)
    selected_index: Option<usize>,
    /// Row height
    row_height: u32,
    /// Scale factor for DPI scaling
    scale_factor: f32,
    /// Position and size
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    /// Callback for when a row is selected (via Enter key)
    on_select: Option<Box<dyn Fn(&ListRow) + 'static>>,
}

impl FilteredList {
    /// Create a new filtered list
    pub fn new(rows: Vec<ListRow>, max_items: usize, width: u32, height: u32, scale_factor: f32) -> Self {
        let row_height = (45.0 * scale_factor) as u32;
        let text_input = TextInput::new(width, row_height, scale_factor);

        let mut filtered_list = Self {
            text_input,
            all_rows: rows,
            filtered_rows: Vec::new(),
            filtered_indices: Vec::new(),
            max_items,
            selected_index: None,
            row_height,
            scale_factor,
            x: 0,
            y: 0,
            width,
            height,
            on_select: None,
        };

        // Initial filter
        filtered_list.update_filtered_rows();

        // Select first item by default if there are any filtered rows
        if !filtered_list.filtered_rows.is_empty() {
            filtered_list.selected_index = Some(0);
        }

        filtered_list
    }

    /// Set the position of the filtered list
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.text_input.set_position(x, y);
    }

    /// Set the callback for row selection
    pub fn set_on_select<F>(&mut self, callback: F)
    where
        F: Fn(&ListRow) + 'static,
    {
        self.on_select = Some(Box::new(callback));
    }

    /// Get the currently selected row (if any)
    pub fn get_selected_row(&self) -> Option<&ListRow> {
        self.selected_index.and_then(|idx| self.filtered_rows.get(idx))
    }

    /// Set all rows (replaces the current list)
    /// Set the focus state of the text input
    pub fn set_focused(&mut self, focused: bool) {
        self.text_input.set_focused(focused);
    }

    /// Update the filtered rows based on the text input content
    pub fn update_filtered_rows(&mut self) {
        let filter_text = self.text_input.get_text().to_lowercase();

        self.filtered_rows.clear();
        self.filtered_indices.clear();

        for (idx, row) in self.all_rows.iter().enumerate() {
            if filter_text.is_empty() || row.text.to_lowercase().contains(&filter_text) {
                self.filtered_rows.push(row.clone());
                self.filtered_indices.push(idx);
            }
        }

        // Select first item by default if there are any filtered rows
        if !self.filtered_rows.is_empty() {
            self.selected_index = Some(0);
        } else {
            self.selected_index = None;
        }
    }

    /// Move selection up
    fn move_selection_up(&mut self) {
        if self.filtered_rows.is_empty() {
            return;
        }

        // Calculate visible rows (same logic as render)
        let list_height = self.height.saturating_sub(self.row_height);
        let visible_rows = ((list_height / self.row_height).min(self.max_items as u32) as usize).min(self.filtered_rows.len());

        if visible_rows == 0 {
            return;
        }

        let last_visible_idx = visible_rows - 1;

        self.selected_index = Some(match self.selected_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => last_visible_idx, // Wrap to bottom of visible list
            None => last_visible_idx,    // Select last visible item
        });
    }

    /// Move selection down
    fn move_selection_down(&mut self) {
        if self.filtered_rows.is_empty() {
            return;
        }

        // Calculate visible rows (same logic as render)
        let list_height = self.height.saturating_sub(self.row_height);
        let visible_rows = ((list_height / self.row_height).min(self.max_items as u32) as usize).min(self.filtered_rows.len());

        if visible_rows == 0 {
            return;
        }

        let last_visible_idx = visible_rows - 1;

        self.selected_index = Some(match self.selected_index {
            Some(idx) if idx < last_visible_idx => idx + 1,
            Some(_) => 0, // Wrap to top of visible list
            None => 0,    // Select first item
        });
    }

    /// Fire the on_select callback for the currently selected row
    fn fire_on_select(&self) {
        eprintln!("[FILTERED_LIST] fire_on_select called");
        if let Some(ref callback) = self.on_select {
            if let Some(row) = self.get_selected_row() {
                eprintln!("[FILTERED_LIST] Calling callback with row: {}", row.text);
                callback(row);
            } else {
                eprintln!("[FILTERED_LIST] ERROR: No selected row!");
            }
        } else {
            eprintln!("[FILTERED_LIST] ERROR: No callback!");
        }
    }

    /// Handle an SDL event
    /// Returns true if the event was consumed by this component
    pub fn handle_event(&mut self, event: &Event) -> bool {
        // First try to handle with text input
        if self.text_input.handle_event(event) {
            eprintln!("[FILTERED_LIST] Text input handled event");
            // Update filtered rows when text changes
            self.update_filtered_rows();
            return true;
        }

        // Handle keyboard navigation (only when focused)
        if self.text_input.is_focused() {
            if let Event::KeyDown { keycode, .. } = event {
                if let Some(keycode) = keycode {
                    eprintln!("[FILTERED_LIST] KeyDown: {:?}, focused: true", keycode);
                    match keycode {
                        Keycode::Up => {
                            eprintln!("[FILTERED_LIST] Moving selection up");
                            self.move_selection_up();
                            return true;
                        }
                        Keycode::Down => {
                            eprintln!("[FILTERED_LIST] Moving selection down");
                            self.move_selection_down();
                            return true;
                        }
                        Keycode::Return => {
                            eprintln!("[FILTERED_LIST] Return key pressed");
                            self.fire_on_select();
                            return true;
                        }
                        Keycode::Escape => {
                            eprintln!("[FILTERED_LIST] Escape pressed, unfocusing");
                            self.set_focused(false);
                            return true;
                        }
                        _ => {
                            eprintln!("[FILTERED_LIST] Key ignored: {:?}", keycode);
                        }
                    }
                }
            }
        } else {
            eprintln!("[FILTERED_LIST] Not focused, ignoring event");
        }

        false
    }

    /// Render the filtered list
    pub fn render<T>(&self, canvas: &mut Canvas<Window>, font: &Font, texture_creator: &TextureCreator<T>) -> Result<(), String> {
        let rect = Rect::new(self.x, self.y, self.width, self.height);

        // Draw background
        canvas.set_draw_color(LIST_BG);
        canvas.fill_rect(rect).map_err(|e| e.to_string())?;

        // Draw border
        canvas.set_draw_color(ROW_BORDER);
        canvas.draw_rect(rect).map_err(|e| e.to_string())?;

        // Render text input
        self.text_input.render(canvas, font, texture_creator)?;

        // Calculate list area (below text input)
        let list_y = self.y + self.row_height as i32;
        let list_height = self.height.saturating_sub(self.row_height);

        // Calculate how many rows can be shown
        let visible_rows = (list_height / self.row_height).min(self.max_items as u32) as usize;

        // Render visible filtered rows
        for (i, row) in self.filtered_rows.iter().take(visible_rows).enumerate() {
            let row_y = list_y + (i as i32 * self.row_height as i32);
            let row_rect = Rect::new(self.x, row_y, self.width, self.row_height);

            // Check if this row is highlighted
            let is_selected = self.selected_index == Some(i);

            // Draw row background
            canvas.set_draw_color(if is_selected { ROW_HIGHLIGHT } else { ROW_BG });
            canvas.fill_rect(row_rect).map_err(|e| e.to_string())?;

            // Draw row border
            canvas.set_draw_color(ROW_BORDER);
            canvas.draw_rect(row_rect).map_err(|e| e.to_string())?;

            // Calculate text rendering position with padding
            let padding = (8.0 * self.scale_factor) as i32;
            let text_x = self.x + padding;
            let text_y = row_y + ((self.row_height as i32 - (30.0 * self.scale_factor) as i32) / 2);

            // Render text with clipping to prevent overflow
            if !row.text.is_empty() {
                if let Ok(surface) = font.render(&row.text).blended(TEXT_COLOR) {
                    if let Ok(texture) = texture_creator.create_texture_from_surface(&surface) {
                        // Calculate available width (row width - 2*padding)
                        let available_width = (self.width as i32 - padding * 2) as u32;

                        // Clip text width if it exceeds available width
                        let text_width = surface.width().min(available_width);
                        let text_rect = Rect::new(text_x, text_y, text_width, surface.height());
                        let src_rect = FRect::new(0.0, 0.0, text_width as f32, surface.height() as f32);
                        let _ = canvas.copy(&texture, Some(src_rect), text_rect);
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_row_creation() {
        let row = ListRow::new("Test Item");
        assert_eq!(row.text, "Test Item");
    }

    #[test]
    fn test_filtered_list_creation() {
        let rows = vec![ListRow::new("Apple"), ListRow::new("Banana"), ListRow::new("Cherry")];
        let list = FilteredList::new(rows, 10, 300, 400, 1.0);

        assert_eq!(list.all_rows.len(), 3);
        assert_eq!(list.filtered_rows.len(), 3);
        assert_eq!(list.max_items, 10);
        assert!(list.selected_index.is_none());
    }

    #[test]
    fn test_selection_navigation() {
        let rows = vec![ListRow::new("Apple"), ListRow::new("Banana"), ListRow::new("Cherry")];
        let mut list = FilteredList::new(rows, 10, 300, 400, 1.0);

        // Move down - should select first item
        list.move_selection_down();
        assert_eq!(list.selected_index, Some(0));

        // Move down again
        list.move_selection_down();
        assert_eq!(list.selected_index, Some(1));

        // Move up
        list.move_selection_up();
        assert_eq!(list.selected_index, Some(0));

        // Move up from first - should wrap to last
        list.move_selection_up();
        assert_eq!(list.selected_index, Some(2));
    }

    #[test]
    fn test_selection_with_filter() {
        let rows = vec![ListRow::new("Apple"), ListRow::new("Banana"), ListRow::new("Cherry"), ListRow::new("Apricot")];
        let mut list = FilteredList::new(rows, 10, 300, 400, 1.0);

        // Select first item
        list.move_selection_down();
        assert_eq!(list.selected_index, Some(0));

        // Filter to only two items

        list.update_filtered_rows();
        assert_eq!(list.filtered_rows.len(), 2);

        // Selection should reset or be adjusted
        assert!(list.selected_index.unwrap() < list.filtered_rows.len());
    }

    #[test]
    fn test_get_selected_row() {
        let rows = vec![ListRow::new("Apple"), ListRow::new("Banana")];
        let mut list = FilteredList::new(rows, 10, 300, 400, 1.0);

        assert!(list.get_selected_row().is_none());

        list.move_selection_down();
        let selected = list.get_selected_row();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().text, "Apple");
    }

    #[test]
    fn test_max_items() {
        let rows = vec![
            ListRow::new("Apple"),
            ListRow::new("Banana"),
            ListRow::new("Cherry"),
            ListRow::new("Date"),
            ListRow::new("Elderberry"),
        ];
        let list = FilteredList::new(rows, 3, 300, 400, 1.0);

        assert_eq!(list.max_items, 3);
        assert_eq!(list.filtered_rows.len(), 5); // All rows are still filtered
                                                 // But render will only show max_items
    }
}
