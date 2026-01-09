//! Reusable context menu component for rendering and handling menu interactions
//!
//! This module provides a generic, reusable context menu system that can be used
//! throughout the application. The menu is defined by a list of items with images,
//! captions, and associated actions.
//!
//! # Architecture
//!
//! The context menu is designed to be:
//! - **Generic**: Uses a generic action type `A` that can be any cloneable type
//! - **Declarative**: Menu items are defined as data structures
//! - **Self-contained**: Handles both rendering and click detection
//! - **Flexible**: Supports enabled/disabled states for menu items
//!
//! # Usage
//!
//! ## Creating a Context Menu
//!
//! ```ignore
//! use crate::ui::context_menu::{ContextMenu, ContextMenuItem};
//!
//! // Define menu items with images, captions, and action identifiers
//! let items = vec![
//!     ContextMenuItem::new(icon_data1, "Action 1", "action1"),
//!     ContextMenuItem::new(icon_data2, "Action 2", "action2"),
//!     ContextMenuItem::with_enabled(icon_data3, "Disabled", "action3", false),
//! ];
//!
//! // Create menu at position (100, 200)
//! let menu = ContextMenu::new(items, (100, 200));
//! ```
//!
//! ## Rendering the Menu
//!
//! ```ignore
//! menu.render(canvas, texture_creator, font)?;
//! ```
//!
//! ## Handling Clicks
//!
//! ```ignore
//! if let Some(action) = menu.handle_click(mouse_x, mouse_y) {
//!     // Process the clicked action
//!     match action {
//!         "action1" => do_something(),
//!         "action2" => do_something_else(),
//!         _ => {}
//!     }
//! }
//! ```
//!
//! # Example: Pane Context Menu
//!
//! The pane layout uses this module to display a context menu for pane operations:
//!
//! ```ignore
//! // In PaneLayout::handle_context_menu_click()
//! let items = vec![
//!     ContextMenuItem::new(images.vertical_split, "Split vertically", "split_vertical"),
//!     ContextMenuItem::new(images.horizontal_split, "Split horizontally", "split_horizontal"),
//!     ContextMenuItem::with_enabled(images.expand, "Turn into tab", "to_tab", pane_count > 1),
//!     ContextMenuItem::new(images.kill, "Kill terminal", "kill_shell"),
//! ];
//!
//! let menu = ContextMenu::new(items, (menu_x, menu_y));
//! if let Some(action) = menu.handle_click(mouse_x, mouse_y) {
//!     self.pending_context_action = Some((pane_id, action.to_string()));
//! }
//! ```

use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{Canvas, TextureCreator};
use sdl3::surface::Surface;
use sdl3::ttf::Font;
use sdl3::video::Window;

/// A single menu item with image, caption, action, and enabled state
#[derive(Clone)]
pub struct ContextMenuItem<A: Clone> {
    /// Image data (PNG/other format) to display as icon
    pub image: &'static [u8],
    /// Text caption to display
    pub caption: String,
    /// Action identifier to return when clicked
    pub action: A,
    /// Whether this menu item is enabled (grayed out if false)
    pub enabled: bool,
}

impl<A: Clone> ContextMenuItem<A> {
    /// Create a new enabled menu item
    pub fn new(image: &'static [u8], caption: impl Into<String>, action: A) -> Self {
        Self {
            image,
            caption: caption.into(),
            action,
            enabled: true,
        }
    }

    /// Create a new menu item with explicit enabled state
    pub fn with_enabled(image: &'static [u8], caption: impl Into<String>, action: A, enabled: bool) -> Self {
        Self {
            image,
            caption: caption.into(),
            action,
            enabled,
        }
    }
}

/// A context menu with position, items, and rendering configuration
#[derive(Clone)]
pub struct ContextMenu<A: Clone> {
    /// Menu items to display
    pub items: Vec<ContextMenuItem<A>>,
    /// Menu position (x, y)
    pub position: (i32, i32),
    /// Menu width in pixels
    pub width: u32,
    /// Height of each menu item in pixels
    pub item_height: u32,
    /// Padding around the menu content
    pub padding: i32,
    /// Currently hovered item index
    pub hovered_item: Option<usize>,
}

impl<A: Clone> ContextMenu<A> {
    /// Create a new context menu with default dimensions
    pub fn new(items: Vec<ContextMenuItem<A>>, position: (i32, i32)) -> Self {
        Self {
            items,
            position,
            width: 400,
            item_height: 55,
            padding: 5,
            hovered_item: None,
        }
    }

    /// Get the bounding rectangle of the menu
    pub fn get_rect(&self) -> Rect {
        let menu_height = (self.items.len() as u32 * self.item_height) + (self.padding as u32 * 2);
        Rect::new(self.position.0, self.position.1, self.width, menu_height)
    }

    /// Check if a point is inside the menu bounds
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.get_rect().contains_point((x, y))
    }

    /// Update the hovered item based on mouse position
    pub fn update_hover(&mut self, mouse_x: i32, mouse_y: i32) {
        if !self.contains_point(mouse_x, mouse_y) {
            self.hovered_item = None;
            return;
        }

        let relative_y = mouse_y - self.position.1 - self.padding;
        if relative_y < 0 {
            self.hovered_item = None;
            return;
        }

        let item_index = (relative_y / self.item_height as i32) as usize;

        if item_index < self.items.len() {
            self.hovered_item = Some(item_index);
        } else {
            self.hovered_item = None;
        }
    }

    /// Handle a click on the menu and return the clicked action (if any)
    pub fn handle_click(&self, mouse_x: i32, mouse_y: i32) -> Option<A> {
        if !self.contains_point(mouse_x, mouse_y) {
            return None;
        }

        let relative_y = mouse_y - self.position.1 - self.padding;
        if relative_y < 0 {
            return None;
        }

        let item_index = (relative_y / self.item_height as i32) as usize;

        if item_index < self.items.len() {
            let item = &self.items[item_index];
            if item.enabled {
                return Some(item.action.clone());
            }
        }

        None
    }

    /// Render the context menu
    pub fn render<T>(&self, canvas: &mut Canvas<Window>, texture_creator: &TextureCreator<T>, font: &Font) -> Result<(), String> {
        let menu_rect = self.get_rect();

        // Draw background
        canvas.set_draw_color(Color::RGB(40, 40, 40));
        canvas.fill_rect(menu_rect).map_err(|e| e.to_string())?;

        // Draw border
        canvas.set_draw_color(Color::RGB(80, 80, 80));
        canvas.draw_rect(menu_rect).map_err(|e| e.to_string())?;

        // Draw menu items with icons
        for (i, item) in self.items.iter().enumerate() {
            let item_y = self.position.1 + self.padding + (i as i32 * self.item_height as i32);

            // Draw hover background
            if self.hovered_item == Some(i) {
                canvas.set_draw_color(Color::RGB(60, 60, 60));
                let hover_rect = Rect::new(self.position.0 + self.padding, item_y, self.width - (self.padding as u32 * 2), self.item_height);
                canvas.fill_rect(hover_rect).map_err(|e| e.to_string())?;
            }

            // Render icon
            if let Ok(img) = image::load_from_memory(item.image) {
                let rgba = img.to_rgba8();
                let (img_width, img_height) = rgba.dimensions();
                let pixels = rgba.into_raw();

                if let Ok(icon_surface) = create_sdl_surface_from_rgba(img_width, img_height, pixels) {
                    if let Ok(icon_texture) = texture_creator.create_texture_from_surface::<&Surface>(&icon_surface) {
                        let icon_size = 32u32.min(img_width).min(img_height);
                        let icon_y = item_y + ((self.item_height as i32 - icon_size as i32).max(0) / 2);
                        let icon_rect = Rect::new(self.position.0 + 10, icon_y, icon_size, icon_size);
                        canvas.copy(&icon_texture, None, icon_rect).map_err(|e| e.to_string())?;
                    }
                }
            }

            // Render text
            let text_color = if item.enabled {
                Color::RGB(200, 200, 200)
            } else {
                Color::RGB(100, 100, 100) // Grayed out
            };

            if let Ok(surface) = font.render(&item.caption).blended(text_color) {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&Surface>(&surface) {
                    let text_y = item_y + ((self.item_height as i32 - surface.height() as i32).max(0) / 2);
                    let text_rect = Rect::new(self.position.0 + 52, text_y, surface.width(), surface.height());
                    canvas.copy(&texture, None, text_rect).map_err(|e| e.to_string())?;
                }
            }
        }

        Ok(())
    }
}

/// Helper function to create an SDL surface from RGBA pixel data
fn create_sdl_surface_from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Surface<'static>, String> {
    let pitch = width * 4;
    Surface::from_data(pixels.leak(), width, height, pitch, sdl3::pixels::PixelFormat::RGBA32).map_err(|e| e.to_string())
}
