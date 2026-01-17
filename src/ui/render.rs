//! Optimized terminal rendering module
//!
//! This module handles rendering of terminal content with performance optimizations:
//! - Only renders the active tab (inactive tabs are not rendered)
//! - Only renders visible terminal content (no off-screen scrollback rendering)
//! - Uses glyph caching to avoid re-rendering characters
//! - Targets 60 FPS max via VSync

use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, Canvas, TextureCreator};
use sdl3::ttf::Font;
use sdl3::video::Window;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ansi::DEFAULT_BG_COLOR;
use crate::screen_buffer::{is_block_or_box_drawing, is_cjk_grapheme, is_emoji_grapheme, is_special_symbol};
use crate::sdl_renderer;
use crate::settings::Settings;
use crate::tab_gui::TabBarGui;
use crate::ui::context_menu::ContextMenu;

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
/// Ensures minimum size of 2x2 to prevent buffer underflow panics
#[inline]
pub fn calculate_terminal_size(rect_width: u32, rect_height: u32, char_width: f32, char_height: f32) -> (u32, u32) {
    let (usable_width, usable_height) = get_usable_dimensions(rect_width, rect_height);
    let cols = (usable_width as f32 / char_width).floor() as u32;
    let rows = (usable_height as f32 / char_height).floor() as u32;

    // Ensure minimum terminal size to prevent buffer underflow
    // This can happen when font size is too large for the available space
    let cols = cols.max(2);
    let rows = rows.max(2);

    (cols, rows)
}

/// Adjust mouse coordinates to account for pane padding and rect offset
#[inline]
pub fn adjust_mouse_coords_for_padding(mouse_x: i32, mouse_y: i32, rect_x: i32, rect_y: i32) -> (i32, i32) {
    let padding = get_pane_padding() as i32;
    ((mouse_x - rect_x).saturating_sub(padding), (mouse_y - rect_y).saturating_sub(padding))
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
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    context_menu_font: &Font,
    cpu_usage: f32,
    tab_bar_height: u32,
    scale_factor: f32,
    char_width: f32,
    char_height: f32,
    cursor_visible: bool,
    settings: &Settings,
    glyph_cache: &mut HashMap<String, sdl3::render::Texture<'a>>,
) -> Result<bool, String> {
    // Clear screen with terminal background color
    canvas.set_draw_color(DEFAULT_BG_COLOR);
    canvas.clear();

    // Get window dimensions (use physical pixel size for crisp rendering)
    let (window_w, window_h) = canvas.window().size_in_pixels();

    // Update and render tab bar
    let (tab_names, active_tab_idx, editing_tab_idx, editing_state) = {
        let gui = tab_bar_gui.lock().unwrap();
        (gui.get_tab_names(), gui.active_tab, gui.get_editing_tab_index(), gui.get_editing_state())
    };
    tab_bar.set_tabs(tab_names);
    tab_bar.set_active_tab(active_tab_idx);
    // Sync editing state from TabBarGui to TabBar for rendering
    tab_bar.editing_tab = editing_tab_idx;
    if let Some((edit_text, cursor_pos)) = editing_state {
        tab_bar.edit_text = edit_text;
        tab_bar.edit_cursor_pos = cursor_pos;
    }
    tab_bar.render(canvas, tab_font, button_font, cpu_font, texture_creator, window_w, cpu_usage)?;

    // Calculate pane area (tab_bar_height is already in physical pixels)
    let pane_area_y = tab_bar_height as i32;
    let pane_area_height = window_h - tab_bar_height;

    // Get active tab's pane layout data (quickly, then release lock)
    // OPTIMIZATION: Only render the active tab, not inactive tabs
    let (pane_rects, pane_count, dividers, context_menu, copy_animation_data) = {
        let mut gui = tab_bar_gui.lock().unwrap();

        match gui.get_active_pane_layout() {
            Some(pane_layout) => {
                let pane_rects = pane_layout.get_pane_rects(0, pane_area_y, window_w, pane_area_height);
                let pane_count = pane_rects.len();
                let dividers = pane_layout.get_divider_rects(0, pane_area_y, window_w, pane_area_height);
                let context_menu = pane_layout.context_menu.clone();
                let copy_animation_data = pane_layout.copy_animation.clone();

                (pane_rects, pane_count, dividers, context_menu, copy_animation_data)
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
    for (_pane_id, rect, terminal, is_active, is_selected) in pane_rects {
        let was_dirty = render_pane(
            canvas,
            texture_creator,
            terminal_font,
            emoji_font,
            unicode_fallback_font,
            cjk_font,
            tab_font,
            rect,
            terminal.clone(),
            is_active,
            is_selected,
            pane_count,
            char_width,
            char_height,
            cursor_visible,
            settings,
            glyph_cache,
            scale_factor,
        )?;
        any_dirty = any_dirty || was_dirty;
    }

    // Render dividers between panes
    render_dividers(canvas, &dividers)?;

    // Render context menu if open
    if let Some(ref menu) = context_menu {
        render_context_menu(canvas, texture_creator, context_menu_font, menu)?;
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
///
/// Returns true if the terminal content was dirty
fn render_pane<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    _ui_font: &Font,
    rect: Rect,
    terminal: Arc<Mutex<crate::terminal::Terminal>>,
    is_active: bool,
    is_selected: bool,
    pane_count: usize,
    char_width: f32,
    char_height: f32,
    cursor_visible: bool,
    settings: &Settings,
    glyph_cache: &mut HashMap<String, sdl3::render::Texture<'a>>,
    scale_factor: f32,
) -> Result<bool, String> {
    let t = terminal.lock().unwrap();
    let mut sb = t.screen_buffer.lock().unwrap();

    // No need to clear pane background - terminal cells will paint their own backgrounds
    // This optimizes rendering by avoiding redundant fills

    // Platform-specific padding
    let pane_padding = get_pane_padding();

    // Calculate how many columns/rows can fit in the pane rect
    let (usable_width, usable_height) = get_usable_dimensions(rect.width(), rect.height());
    let rect_cols = (usable_width as f32 / char_width).floor() as usize;
    let rect_rows = (usable_height as f32 / char_height).floor() as usize;

    // Render up to the smaller of: what fits in rect, or what's in screen buffer
    // This prevents rendering outside rect bounds (overflow into other panes)
    // while also not trying to read beyond screen buffer dimensions
    let cols = rect_cols.min(sb.width());
    let rows = rect_rows.min(sb.height());

    // Get selection for highlighting (cached once per frame to avoid locking in cell loop)
    let selection_snapshot = *t.selection.lock().unwrap();

    // Render cells that fit in both the rect and the screen buffer
    for row in 0..rows {
        for col in 0..cols {
            if let Some(cell) = sb.get_cell_with_scrollback(col, row) {
                // Skip continuation cells (used by double-width emojis)
                if cell.width == 0 || cell.ch == '\0' {
                    continue;
                }

                let x = rect.x() + pane_padding as i32 + (col as f32 * char_width) as i32;
                let y = rect.y() + pane_padding as i32 + (row as f32 * char_height) as i32;

                // Calculate actual width for this character (1 or 2 cells)
                let actual_cell_width = char_width * cell.width as f32;

                // Check if cell is selected
                let is_selected = if let Some(ref sel) = selection_snapshot {
                    sel.contains(col, row)
                } else {
                    false
                };

                // Render background (selection highlight or cell background)
                if is_selected {
                    canvas.set_draw_color(Color::RGB(70, 130, 180));
                    let cell_rect = Rect::new(x, y, actual_cell_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                } else if cell.bg_color.r != 0 || cell.bg_color.g != 0 || cell.bg_color.b != 0 {
                    canvas.set_draw_color(Color::RGB(cell.bg_color.r, cell.bg_color.g, cell.bg_color.b));
                    let cell_rect = Rect::new(x, y, actual_cell_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                }

                // OPTIMIZATION: Render character if not space (skip spaces with default bg)
                if cell.ch != ' ' {
                    // Use extended grapheme if present, otherwise use single char
                    let char_str;
                    let text = if let Some(ref extended) = cell.extended {
                        extended.as_ref()
                    } else {
                        char_str = cell.ch.to_string();
                        char_str.as_str()
                    };

                    render_glyph(
                        canvas,
                        texture_creator,
                        font,
                        emoji_font,
                        unicode_fallback_font,
                        cjk_font,
                        glyph_cache,
                        text,
                        x,
                        y,
                        cell.fg_color.r,
                        cell.fg_color.g,
                        cell.fg_color.b,
                        actual_cell_width as u32,
                        char_height as u32,
                        scale_factor,
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

    let was_dirty = sb.is_dirty();
    sb.clear_dirty();

    // Check if dirty flag was set again during render (race condition)
    let still_dirty = sb.is_dirty();

    // Release locks
    drop(sb);
    drop(t);

    // Draw border for selected panes (green) or active pane (blue)
    if is_selected && pane_count > 1 {
        // Selected panes get a green border
        canvas.set_draw_color(Color::RGB(50, 180, 80));
        let border_width = 3;
        // Top border
        canvas
            .fill_rect(Rect::new(rect.x(), rect.y(), rect.width(), border_width))
            .map_err(|e| e.to_string())?;
        // Bottom border
        canvas
            .fill_rect(Rect::new(
                rect.x(),
                rect.y() + rect.height() as i32 - border_width as i32,
                rect.width(),
                border_width,
            ))
            .map_err(|e| e.to_string())?;
        // Left border
        canvas
            .fill_rect(Rect::new(rect.x(), rect.y(), border_width, rect.height()))
            .map_err(|e| e.to_string())?;
        // Right border
        canvas
            .fill_rect(Rect::new(
                rect.x() + rect.width() as i32 - border_width as i32,
                rect.y(),
                border_width,
                rect.height(),
            ))
            .map_err(|e| e.to_string())?;
    } else if is_active && pane_count > 1 {
        // Active pane gets a blue border
        canvas.set_draw_color(Color::RGB(50, 90, 130));
        canvas.draw_rect(rect).map_err(|e| e.to_string())?;
    }

    Ok(was_dirty || still_dirty)
}

/// Render a single glyph with caching
fn render_glyph<'a, T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &'a TextureCreator<T>,
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    glyph_cache: &mut HashMap<String, sdl3::render::Texture<'a>>,
    text: &str,
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
    cell_width: u32,
    cell_height: u32,
    _scale_factor: f32,
) -> Result<(), String> {
    let cache_key = text.to_string();

    // Check cache first
    if let Some(cached_texture) = glyph_cache.get_mut(&cache_key) {
        // Apply color modulation to the white texture
        cached_texture.set_color_mod(r, g, b);
        let query = cached_texture.query();

        // Check if this is an emoji - if so, scale it to fit in cell
        let is_likely_emoji = is_emoji_grapheme(text);

        // Check if this is a special symbol that needs scaling
        let is_special_missing_symbol = text.chars().count() == 1 && text.chars().next().map_or(false, is_special_symbol);

        // Check if this is a block/box drawing character that needs cell-filling
        let is_block_box_char = text.chars().count() == 1 && text.chars().next().map_or(false, is_block_or_box_drawing);

        if is_block_box_char {
            // Stretch block/box drawing characters to fill the entire cell for ASCII art
            // No aspect ratio preservation - these characters are designed to be stretched
            let char_rect = Rect::new(x, y, cell_width, cell_height);
            canvas.copy(cached_texture, None, char_rect).map_err(|e| e.to_string())?;
        } else if is_likely_emoji {
            // Scale emoji to fill available space (double-width emojis get 2x cell_width)
            // Use the smaller of width or height to maintain square aspect ratio
            let target_size = cell_width.min(cell_height);

            let emoji_width = query.width;
            let emoji_height = query.height;

            // Calculate scaling to fit the target size while maintaining aspect ratio
            let scale_x = target_size as f32 / emoji_width as f32;
            let scale_y = target_size as f32 / emoji_height as f32;
            let scale = scale_x.min(scale_y);

            let scaled_width = (emoji_width as f32 * scale) as u32;
            let scaled_height = (emoji_height as f32 * scale) as u32;

            // Center the emoji in the cell (horizontally and vertically)
            let offset_x = (cell_width as i32 - scaled_width as i32) / 2;
            let offset_y = (cell_height as i32 - scaled_height as i32) / 2;

            let char_rect = Rect::new(x + offset_x, y + offset_y, scaled_width, scaled_height);
            canvas.copy(cached_texture, None, char_rect).map_err(|e| e.to_string())?;
        } else if is_special_missing_symbol {
            // Scale up special symbols to make them more visible (2.0x larger than default)
            let target_size = (cell_width.min(cell_height) as f32 * 2.0) as u32;

            let symbol_width = query.width;
            let symbol_height = query.height;

            // Calculate scaling to fit the target size while maintaining aspect ratio
            let scale_x = target_size as f32 / symbol_width as f32;
            let scale_y = target_size as f32 / symbol_height as f32;
            let scale = scale_x.min(scale_y);

            let scaled_width = (symbol_width as f32 * scale) as u32;
            let scaled_height = (symbol_height as f32 * scale) as u32;

            // Center the symbol in the cell
            let offset_x = (cell_width as i32 - scaled_width as i32) / 2;
            let offset_y = (cell_height as i32 - scaled_height as i32) / 2;

            let char_rect = Rect::new(x + offset_x, y + offset_y, scaled_width, scaled_height);
            canvas.copy(cached_texture, None, char_rect).map_err(|e| e.to_string())?;
        } else {
            // Regular character - use original size
            let char_rect = Rect::new(x, y, query.width, query.height);
            canvas.copy(cached_texture, None, char_rect).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    // Render all glyphs in white for color modulation
    let render_color = Color::RGB(255, 255, 255);

    // Check if this is an emoji character - if so, try emoji font FIRST
    let is_likely_emoji = is_emoji_grapheme(text);

    // Check if this is a CJK character - if so, try CJK font FIRST
    let is_likely_cjk = is_cjk_grapheme(text);

    if is_likely_emoji {
        // Try emoji font first for emoji characters
        let emoji_result = emoji_font.render(text).blended(render_color);
        if let Ok(surface) = emoji_result {
            if surface.width() > 0 && surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                    // Scale emoji to fill available space (double-width emojis get 2x cell_width)
                    // Use the smaller of width or height to maintain square aspect ratio
                    let target_size = cell_width.min(cell_height);

                    let emoji_width = surface.width();
                    let emoji_height = surface.height();

                    // Calculate scaling to fit the target size while maintaining aspect ratio
                    let scale_x = target_size as f32 / emoji_width as f32;
                    let scale_y = target_size as f32 / emoji_height as f32;
                    let scale = scale_x.min(scale_y);

                    let scaled_width = (emoji_width as f32 * scale) as u32;
                    let scaled_height = (emoji_height as f32 * scale) as u32;

                    // Center the emoji in the cell (horizontally and vertically)
                    let offset_x = (cell_width as i32 - scaled_width as i32) / 2;
                    let offset_y = (cell_height as i32 - scaled_height as i32) / 2;

                    let char_rect = Rect::new(x + offset_x, y + offset_y, scaled_width, scaled_height);
                    // Note: Emojis already rendered in white, color mod applied to cache lookup above
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    // Cache the texture for next frame
                    glyph_cache.insert(cache_key.clone(), texture);
                    return Ok(());
                }
            }
        }
    }

    // Try CJK font first for CJK characters (Chinese, Japanese, Korean)
    if is_likely_cjk {
        let cjk_result = cjk_font.render(text).blended(render_color);
        if let Ok(surface) = cjk_result {
            if surface.width() > 0 && surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                    let char_rect = Rect::new(x, y, surface.width(), surface.height());
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(cache_key.clone(), texture);
                    return Ok(());
                }
            }
        }
    }

    // Check if this is a symbol from ranges that are often missing from terminal fonts
    // but present in FreeMono: Miscellaneous Technical, Dingbats, Geometric Shapes, etc.
    let is_special_missing_symbol = text.chars().count() == 1 && text.chars().next().map_or(false, is_special_symbol);

    // Check if this is a block/box drawing character that needs cell-filling
    let is_block_box_char = text.chars().count() == 1 && text.chars().next().map_or(false, is_block_or_box_drawing);

    // For block/box drawing characters, try unicode fallback font FIRST and scale to fill cell
    if is_block_box_char && !is_likely_emoji {
        let unicode_fallback_result = unicode_fallback_font.render(text).blended(render_color);
        if let Ok(unicode_surface) = unicode_fallback_result {
            if unicode_surface.width() > 0 && unicode_surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&unicode_surface) {
                    // Stretch to fill the entire cell for ASCII art
                    // No aspect ratio preservation - these characters are designed to be stretched
                    let char_rect = Rect::new(x, y, cell_width, cell_height);
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(cache_key, texture);
                    return Ok(());
                }
            }
        }
    } else if is_special_missing_symbol && !is_likely_emoji {
        let unicode_fallback_result = unicode_fallback_font.render(text).blended(render_color);
        if let Ok(unicode_surface) = unicode_fallback_result {
            if unicode_surface.width() > 0 && unicode_surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&unicode_surface) {
                    // Scale up special symbols to make them more visible (2.0x larger than default)
                    let target_size = (cell_width.min(cell_height) as f32 * 2.0) as u32;

                    let symbol_width = unicode_surface.width();
                    let symbol_height = unicode_surface.height();

                    // Calculate scaling to fit the target size while maintaining aspect ratio
                    let scale_x = target_size as f32 / symbol_width as f32;
                    let scale_y = target_size as f32 / symbol_height as f32;
                    let scale = scale_x.min(scale_y);

                    let scaled_width = (symbol_width as f32 * scale) as u32;
                    let scaled_height = (symbol_height as f32 * scale) as u32;

                    // Center the symbol in the cell
                    let offset_x = (cell_width as i32 - scaled_width as i32) / 2;
                    let offset_y = (cell_height as i32 - scaled_height as i32) / 2;

                    let char_rect = Rect::new(x + offset_x, y + offset_y, scaled_width, scaled_height);
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(cache_key, texture);
                    return Ok(());
                }
            }
        }
    }

    // Not in cache, render and cache it (try main font for non-emoji or if emoji font failed)
    // For single characters use render_char, for grapheme clusters use render
    let render_result = if text.chars().count() == 1 {
        font.render_char(text.chars().next().unwrap()).blended(render_color)
    } else {
        font.render(text).blended(render_color)
    };

    if let Ok(surface) = render_result {
        if surface.width() > 0 && surface.height() > 0 {
            if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                // If this is a block/box drawing character, stretch to fill entire cell
                if is_block_box_char {
                    // Stretch to fill the entire cell for ASCII art
                    // No aspect ratio preservation - these characters are designed to be stretched
                    let char_rect = Rect::new(x, y, cell_width, cell_height);
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(cache_key, texture);
                    return Ok(());
                } else {
                    let char_rect = Rect::new(x, y, surface.width(), surface.height());
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    // Cache the texture for next frame
                    glyph_cache.insert(cache_key, texture);
                    return Ok(());
                }
            }
        }

        // Main font produced empty surface - try fallback fonts
        if !is_likely_emoji {
            // Try emoji font for non-emoji characters (might be symbols with emoji variants)
            let emoji_fallback_result = emoji_font.render(text).blended(render_color);
            if let Ok(emoji_surface) = emoji_fallback_result {
                if emoji_surface.width() > 0 && emoji_surface.height() > 0 {
                    if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&emoji_surface) {
                        let char_rect = Rect::new(x, y, emoji_surface.width(), emoji_surface.height());
                        canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                        glyph_cache.insert(cache_key, texture);
                        return Ok(());
                    }
                }
            }
        }

        // Try CJK font for CJK characters (Chinese, Japanese, Korean)
        let cjk_fallback_result = cjk_font.render(text).blended(render_color);
        if let Ok(cjk_surface) = cjk_fallback_result {
            if cjk_surface.width() > 0 && cjk_surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&cjk_surface) {
                    let char_rect = Rect::new(x, y, cjk_surface.width(), cjk_surface.height());
                    canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                    glyph_cache.insert(cache_key, texture);
                    return Ok(());
                }
            }
        }

        // Try Unicode fallback font (for all characters that failed emoji/main/CJK fonts)
        // Skip if we already tried it above for the 3 special symbols
        if !is_special_missing_symbol {
            let unicode_fallback_result = unicode_fallback_font.render(text).blended(render_color);
            if let Ok(unicode_surface) = unicode_fallback_result {
                if unicode_surface.width() > 0 && unicode_surface.height() > 0 {
                    if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&unicode_surface) {
                        let char_rect = Rect::new(x, y, unicode_surface.width(), unicode_surface.height());
                        canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                        glyph_cache.insert(cache_key, texture);
                        return Ok(());
                    }
                }
            }
        }

        // Character not supported in any font, try fallback '□'
        let fallback_key = "□".to_string();
        if let Some(cached_fallback) = glyph_cache.get_mut(&fallback_key) {
            cached_fallback.set_color_mod(r, g, b);
            let query = cached_fallback.query();
            let char_rect = Rect::new(x, y, query.width, query.height);
            canvas.copy(cached_fallback, None, char_rect).map_err(|e| e.to_string())?;
        } else if let Ok(fallback_surface) = font.render_char('□').blended(render_color) {
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
fn render_scrollback_indicator<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
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
fn render_context_menu<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    menu_font: &Font,
    menu: &ContextMenu<String>,
) -> Result<(), String> {
    menu.render(canvas, texture_creator, menu_font)?;
    Ok(())
}

/// Render copy animation
fn render_copy_animation(canvas: &mut Canvas<Window>, animation: &crate::ui::animations::CopyAnimation) -> Result<(), String> {
    let current_rect = animation.current_rect();
    let opacity = animation.current_opacity();

    // Enable alpha blending for transparency
    canvas.set_blend_mode(BlendMode::Blend);

    // Draw fading rectangle
    let color = Color::RGBA(70, 130, 180, opacity);
    canvas.set_draw_color(color);
    canvas.fill_rect(current_rect).map_err(|e| e.to_string())?;

    Ok(())
}
