//! Optimized terminal rendering module
//!
//! This module handles rendering of terminal content with performance optimizations:
//! - Only renders the active tab (inactive tabs are not rendered)
//! - Only renders visible terminal content (no off-screen scrollback rendering)
//! - Uses glyph caching to avoid re-rendering characters
//! - Targets 60 FPS max via VSync

use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{Canvas, TextureCreator};
use sdl3::ttf::Font;
use sdl3::video::Window;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ansi::DEFAULT_BG_COLOR;
use crate::sdl_renderer;
use crate::settings::Settings;
use crate::tab_gui::TabBarGui;

/// Get the platform-specific pane padding in pixels
#[inline]
pub fn get_pane_padding() -> u32 {
    #[cfg(target_os = "windows")]
    return 6;
    #[cfg(not(target_os = "windows"))]
    return 4;
}

/// Calculate usable dimensions after accounting for padding
#[inline]
pub fn get_usable_dimensions(rect_width: u32, rect_height: u32) -> (u32, u32) {
    let padding = get_pane_padding() * 2;
    (rect_width.saturating_sub(padding), rect_height.saturating_sub(padding))
}

/// Calculate terminal columns and rows from rect dimensions
#[inline]
pub fn calculate_terminal_size(rect_width: u32, rect_height: u32, char_width: f32, char_height: f32) -> (u32, u32) {
    let (usable_width, usable_height) = get_usable_dimensions(rect_width, rect_height);
    let cols = (usable_width as f32 / char_width).floor() as u32;
    let rows = (usable_height as f32 / char_height).floor() as u32;
    (cols, rows)
}

/// Adjust mouse coordinates to account for pane padding and rect offset
#[inline]
pub fn adjust_mouse_coords_for_padding(mouse_x: i32, mouse_y: i32, rect_x: i32, rect_y: i32) -> (i32, i32) {
    let padding = get_pane_padding() as i32;
    ((mouse_x - rect_x).saturating_sub(padding), (mouse_y - rect_y).saturating_sub(padding))
}

/// Helper function to create SDL surface from RGBA pixel data
fn create_sdl_surface_from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<sdl3::surface::Surface<'static>, String> {
    let mut surface =
        sdl3::surface::Surface::new(width, height, sdl3::pixels::PixelFormat::RGBA32).map_err(|e| format!("Failed to create SDL surface: {}", e))?;

    // Copy pixel data
    surface.with_lock_mut(|buffer: &mut [u8]| {
        buffer.copy_from_slice(&pixels);
    });

    Ok(surface)
}

/// Render the entire frame including tab bar and active tab's panes
/// Returns true if any terminal content was dirty and needed re-rendering
pub fn render_frame<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    tab_bar: &mut sdl_renderer::TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_font: &Font,
    button_font: &Font,
    cpu_font: &Font,
    terminal_font: &Font,
    context_menu_font: &Font,
    cpu_usage: f32,
    tab_bar_height: u32,
    _scale_factor: f32,
    char_width: f32,
    char_height: f32,
    cursor_visible: bool,
    settings: &Settings,
    glyph_cache: &mut HashMap<(char, (u8, u8, u8)), sdl3::render::Texture<'a>>,
) -> Result<bool, String> {
    // Clear screen with terminal background color
    canvas.set_draw_color(DEFAULT_BG_COLOR);
    canvas.clear();

    // Get window dimensions (use physical pixel size for crisp rendering)
    let (window_w, window_h) = canvas.window().size_in_pixels();

    // Update and render tab bar
    let (tab_names, active_tab_idx) = {
        let gui = tab_bar_gui.lock().unwrap();
        (gui.get_tab_names(), gui.active_tab)
    };
    tab_bar.set_tabs(tab_names);
    tab_bar.set_active_tab(active_tab_idx);
    tab_bar.render(canvas, tab_font, button_font, cpu_font, texture_creator, window_w, cpu_usage)?;

    // Calculate pane area (tab_bar_height is already in physical pixels)
    let pane_area_y = tab_bar_height as i32;
    let pane_area_height = window_h - tab_bar_height;

    // Get active tab's pane layout data (quickly, then release lock)
    // OPTIMIZATION: Only render the active tab, not inactive tabs
    let (pane_rects, pane_count, dividers, context_menu_data, copy_animation_data, context_menu_images) = {
        let gui = tab_bar_gui.lock().unwrap();

        match gui.tab_states.get(gui.active_tab) {
            Some(pane_layout) => {
                let pane_rects = pane_layout.pane_layout.get_pane_rects(0, pane_area_y, window_w, pane_area_height);
                let pane_count = pane_rects.len();
                let dividers = pane_layout.pane_layout.get_divider_rects(0, pane_area_y, window_w, pane_area_height);
                let context_menu_data = pane_layout.pane_layout.context_menu_open.clone();
                let copy_animation_data = pane_layout.pane_layout.copy_animation.clone();
                let context_menu_images = pane_layout.pane_layout.context_menu_images.clone();

                (pane_rects, pane_count, dividers, context_menu_data, copy_animation_data, context_menu_images)
            }
            None => {
                // No active tab, just present empty screen
                canvas.present();
                return Ok(false);
            }
        }
    };

    // Render each pane in the active tab (inactive tabs are NOT rendered)
    let mut any_dirty = false;
    for (_pane_id, rect, terminal, is_active) in pane_rects {
        let dirty = render_pane(
            canvas,
            texture_creator,
            terminal_font,
            rect,
            terminal,
            is_active,
            pane_count,
            char_width,
            char_height,
            cursor_visible,
            settings,
            glyph_cache,
        )?;
        any_dirty = any_dirty || dirty;
    }

    // Render dividers between panes
    render_dividers(canvas, &dividers)?;

    // Render context menu if open
    if let Some((_, menu_x, menu_y)) = context_menu_data {
        render_context_menu(canvas, texture_creator, context_menu_font, menu_x, menu_y, pane_count, &context_menu_images)?;
    }

    // Render copy animation if active
    if let Some(ref animation) = copy_animation_data {
        if !animation.is_complete() {
            render_copy_animation(canvas, animation)?;
        }
    }

    canvas.present();
    Ok(any_dirty)
}

/// Render a single pane's terminal content
/// Optimizations:
/// - Only renders visible rows (no off-screen content)
/// - Uses glyph caching
/// - Skips rendering of spaces with default background
/// Returns true if the terminal content was dirty
fn render_pane<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    font: &Font,
    rect: Rect,
    terminal: Arc<Mutex<crate::terminal::Terminal>>,
    is_active: bool,
    pane_count: usize,
    char_width: f32,
    char_height: f32,
    cursor_visible: bool,
    settings: &Settings,
    glyph_cache: &mut HashMap<(char, (u8, u8, u8)), sdl3::render::Texture<'a>>,
) -> Result<bool, String> {
    let t = terminal.lock().unwrap();
    let mut sb = t.screen_buffer.lock().unwrap();

    // No need to clear pane background - terminal cells will paint their own backgrounds
    // This optimizes rendering by avoiding redundant fills

    // Platform-specific padding
    let pane_padding = get_pane_padding();

    // Calculate visible terminal grid dimensions
    let (usable_width, usable_height) = get_usable_dimensions(rect.width(), rect.height());
    let cols = (usable_width as f32 / char_width).floor() as usize;
    let rows = (usable_height as f32 / char_height).floor() as usize;

    // Get selection for highlighting
    let selection = *t.selection.lock().unwrap();

    // OPTIMIZATION: Only render visible rows (skip off-screen scrollback)
    let visible_rows = rows.min(sb.height());

    // Render only visible cells
    for row in 0..visible_rows {
        for col in 0..cols.min(sb.width()) {
            if let Some(cell) = sb.get_cell_with_scrollback(col, row) {
                let x = rect.x() + pane_padding as i32 + (col as f32 * char_width) as i32;
                let y = rect.y() + pane_padding as i32 + (row as f32 * char_height) as i32;

                // Check if cell is selected
                let is_selected = if let Some(ref sel) = selection { sel.contains(col, row) } else { false };

                // Render background (selection highlight or cell background)
                if is_selected {
                    canvas.set_draw_color(Color::RGB(70, 130, 180));
                    let cell_rect = Rect::new(x, y, char_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                } else if cell.bg_color.r != 0 || cell.bg_color.g != 0 || cell.bg_color.b != 0 {
                    canvas.set_draw_color(Color::RGB(cell.bg_color.r, cell.bg_color.g, cell.bg_color.b));
                    let cell_rect = Rect::new(x, y, char_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                }

                // OPTIMIZATION: Render character if not space (skip spaces with default bg)
                if cell.ch != ' ' {
                    render_glyph(
                        canvas,
                        texture_creator,
                        font,
                        glyph_cache,
                        cell.ch,
                        x,
                        y,
                        cell.fg_color.r,
                        cell.fg_color.g,
                        cell.fg_color.b,
                    )?;
                }
            }
        }
    }

    // Render cursor if active pane and visible
    if is_active && cursor_visible && sb.is_at_bottom() {
        let cursor_x = rect.x() + pane_padding as i32 + (sb.cursor_x as f32 * char_width) as i32;
        let cursor_y = rect.y() + pane_padding as i32 + (sb.cursor_y as f32 * char_height) as i32;
        canvas.set_draw_color(Color::RGB(200, 200, 200));

        // Cursor style from settings
        let cursor_width = if settings.terminal.cursor == "pipe" { 2 } else { char_width as u32 };
        let cursor_rect = Rect::new(cursor_x, cursor_y, cursor_width, char_height as u32);
        canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;
    }

    // Show scroll position indicator when viewing scrollback
    if !sb.is_at_bottom() {
        render_scrollback_indicator(canvas, texture_creator, font, rect, sb.scroll_offset, pane_padding)?;
    }

    // Draw border for active pane (only if multiple panes)
    if is_active && pane_count > 1 {
        canvas.set_draw_color(Color::RGB(50, 90, 130));
        canvas.draw_rect(rect).map_err(|e| e.to_string())?;
    }

    let was_dirty = sb.is_dirty();
    sb.clear_dirty();

    // Check if dirty flag was set again during render (race condition)
    let still_dirty = sb.is_dirty();

    Ok(was_dirty || still_dirty)
}

/// Render a single glyph with caching
fn render_glyph<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    font: &Font,
    glyph_cache: &mut HashMap<(char, (u8, u8, u8)), sdl3::render::Texture<'a>>,
    ch: char,
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    let fg_color = Color::RGB(r, g, b);
    let cache_key = (ch, (r, g, b));

    // Check cache first
    if let Some(cached_texture) = glyph_cache.get(&cache_key) {
        let query = cached_texture.query();
        let char_rect = Rect::new(x, y, query.width, query.height);
        canvas.copy(cached_texture, None, char_rect).map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Not in cache, render and cache it
    let render_result = font.render_char(ch).blended(fg_color);

    if let Ok(surface) = render_result {
        if surface.width() > 0 && surface.height() > 0 {
            if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                let char_rect = Rect::new(x, y, surface.width(), surface.height());
                canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                // Cache the texture for next frame
                glyph_cache.insert(cache_key, texture);
                return Ok(());
            }
        }

        // Character not supported, try fallback '□'
        let fallback_key = ('□', (r, g, b));
        if let Some(cached_fallback) = glyph_cache.get(&fallback_key) {
            let query = cached_fallback.query();
            let char_rect = Rect::new(x, y, query.width, query.height);
            canvas.copy(cached_fallback, None, char_rect).map_err(|e| e.to_string())?;
        } else if let Ok(fallback_surface) = font.render_char('□').blended(fg_color) {
            if fallback_surface.width() > 0 && fallback_surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&fallback_surface) {
                    let char_rect = Rect::new(x, y, fallback_surface.width(), fallback_surface.height());
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(fallback_key, texture);
                }
            }
        }
    }

    Ok(())
}

/// Render scrollback position indicator
fn render_scrollback_indicator<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    font: &Font,
    rect: Rect,
    scroll_offset: usize,
    pane_padding: u32,
) -> Result<(), String> {
    let scroll_text = format!("[Scrollback: {} lines]", scroll_offset);
    let text_color = Color::RGB(255, 200, 0);

    if let Ok(surface) = font.render(&scroll_text).blended(text_color) {
        if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
            let text_width = surface.width();
            let text_height = surface.height();

            // Position at bottom-right of the pane with padding
            let indicator_x = rect.x() + rect.width() as i32 - text_width as i32 - 10 - pane_padding as i32;
            let indicator_y = rect.y() + rect.height() as i32 - text_height as i32 - 5 - pane_padding as i32;

            let text_rect = Rect::new(indicator_x, indicator_y, text_width, text_height);
            canvas.copy(&texture, None, text_rect).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

/// Render dividers between panes
fn render_dividers(canvas: &mut Canvas<Window>, dividers: &[(crate::pane_layout::PaneId, Rect, crate::pane_layout::SplitDirection)]) -> Result<(), String> {
    for (_split_id, rect, _direction) in dividers {
        canvas.set_draw_color(Color::RGB(60, 60, 60));
        canvas.fill_rect(*rect).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Render context menu
fn render_context_menu<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    menu_font: &Font,
    menu_x: i32,
    menu_y: i32,
    pane_count: usize,
    context_menu_images: &Option<crate::pane_layout::ContextMenuImages>,
) -> Result<(), String> {
    let menu_width = 400;
    let item_height = 55;
    let menu_items = ["Split vertically", "Split horizontally", "Turn into a tab"];
    let menu_height = (menu_items.len() as u32 * item_height) + 10;
    let menu_rect = Rect::new(menu_x, menu_y, menu_width, menu_height);

    // Draw background
    canvas.set_draw_color(Color::RGB(40, 40, 40));
    canvas.fill_rect(menu_rect).map_err(|e| e.to_string())?;

    // Draw border
    canvas.set_draw_color(Color::RGB(80, 80, 80));
    canvas.draw_rect(menu_rect).map_err(|e| e.to_string())?;

    // Draw menu items with icons
    if let Some(ref menu_images) = context_menu_images {
        for (i, item) in menu_items.iter().enumerate() {
            let item_y = menu_y + 5 + (i as i32 * item_height as i32);

            // Get icon for this menu item
            let image_data = match i {
                0 => menu_images.vertical_split,
                1 => menu_images.horizontal_split,
                2 => menu_images.expand_into_tab,
                _ => continue,
            };

            // Render icon
            if let Ok(img) = image::load_from_memory(image_data) {
                let rgba = img.to_rgba8();
                let (img_width, img_height) = rgba.dimensions();
                let pixels = rgba.into_raw();

                if let Ok(icon_surface) = create_sdl_surface_from_rgba(img_width, img_height, pixels) {
                    if let Ok(icon_texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&icon_surface) {
                        let icon_size = 32u32.min(img_width).min(img_height);
                        let icon_y = item_y + ((item_height as i32 - icon_size as i32).max(0) / 2);
                        let icon_rect = Rect::new(menu_x + 10, icon_y, icon_size, icon_size);
                        canvas.copy(&icon_texture, None, icon_rect).map_err(|e| e.to_string())?;
                    }
                }
            }

            // Render text
            let text_color = if i == 2 && pane_count == 1 {
                Color::RGB(100, 100, 100) // Grayed out
            } else {
                Color::RGB(200, 200, 200)
            };

            if let Ok(surface) = menu_font.render(item).blended(text_color) {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                    let text_y = item_y + ((item_height as i32 - surface.height() as i32).max(0) / 2);
                    let text_rect = Rect::new(menu_x + 52, text_y, surface.width(), surface.height());
                    canvas.copy(&texture, None, text_rect).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    Ok(())
}

/// Render copy animation
fn render_copy_animation(canvas: &mut Canvas<Window>, animation: &crate::ui::animations::CopyAnimation) -> Result<(), String> {
    let current_rect = animation.current_rect();
    let opacity = animation.current_opacity();

    // Draw fading rectangle
    let color = Color::RGBA(70, 130, 180, opacity);
    canvas.set_draw_color(color);
    canvas.fill_rect(current_rect).map_err(|e| e.to_string())?;

    Ok(())
}
