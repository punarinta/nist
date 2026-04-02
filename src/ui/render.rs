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
use crate::cell::{is_block_or_box_drawing, is_cjk_grapheme, is_emoji_grapheme, is_special_symbol};
use crate::sdl_renderer;
use crate::tab_gui::TabBarGui;
use crate::ui::context_menu::ContextMenu;
use crate::ui::custom_cell;

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
    glyph_cache: &mut HashMap<String, sdl3::render::Texture<'a>>,
    mouse_state: &crate::input::mouse::MouseState,
    voice_recording: bool,
    voice_transcribing: bool,
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
    let mut active_pane_rect: Option<Rect> = None;
    for (pane_id, rect, terminal, is_active, is_selected) in pane_rects {
        if is_active {
            active_pane_rect = Some(rect);
        }
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
            glyph_cache,
            scale_factor,
            mouse_state,
            pane_id,
        )?;
        any_dirty = any_dirty || was_dirty;
    }

    // Render voice input indicator on top of the active pane
    if voice_recording || voice_transcribing {
        if let Some(rect) = active_pane_rect {
            render_voice_indicator(canvas, rect, voice_transcribing, cursor_visible)?;
        }
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
    glyph_cache: &mut HashMap<String, sdl3::render::Texture<'a>>,
    scale_factor: f32,
    mouse_state: &crate::input::mouse::MouseState,
    pane_id: crate::pane_layout::PaneId,
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

    // Check if we should show cursor (for skipping cursor cell in main loop)
    let terminal_cursor_visible_check = t.cursor_visible.lock().unwrap();
    let terminal_cursor_vis = *terminal_cursor_visible_check;
    let is_at_bottom = sb.is_at_bottom();
    let should_show_cursor_check = terminal_cursor_vis && cursor_visible && is_active && is_at_bottom;
    drop(terminal_cursor_visible_check);

    // Render cells that fit in both the rect and the screen buffer
    for row in 0..rows {
        for col in 0..cols {
            // Skip rendering cursor position if we'll render it as a block cursor later
            use crate::screen_buffer::CursorStyle;
            let is_bar_cursor = matches!(sb.cursor_style, CursorStyle::BlinkingBar | CursorStyle::SteadyBar);
            if should_show_cursor_check && !is_bar_cursor && col == sb.cursor_x && row == sb.cursor_y {
                continue;
            }

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
                    sel.contains(col, row, sb.scroll_offset, sb.scrollback_len())
                } else {
                    false
                };

                // Apply reverse video mode if enabled (swap fg/bg globally)
                let (cell_fg, cell_bg) = if sb.reverse_video_mode {
                    (cell.bg_color, cell.fg_color)
                } else {
                    (cell.fg_color, cell.bg_color)
                };

                // Render background (selection highlight or cell background)
                // Need to consider reverse attribute when determining the actual background color
                let actual_bg = if cell.reverse {
                    // When reverse is true, foreground becomes background
                    cell_fg
                } else {
                    cell_bg
                };

                if is_selected {
                    canvas.set_draw_color(Color::RGB(70, 130, 180));
                    let cell_rect = Rect::new(x, y, actual_cell_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                } else if actual_bg.r != crate::ansi::DEFAULT_BG_COLOR.r
                    || actual_bg.g != crate::ansi::DEFAULT_BG_COLOR.g
                    || actual_bg.b != crate::ansi::DEFAULT_BG_COLOR.b
                // || cell.reverse
                {
                    // Draw background only if it differs from the default that we already filled
                    // This optimizes rendering and prevents artifacts from stale reverse video attributes
                    canvas.set_draw_color(Color::RGB(actual_bg.r, actual_bg.g, actual_bg.b));
                    let cell_rect = Rect::new(x, y, actual_cell_width as u32, char_height as u32);
                    canvas.fill_rect(cell_rect).map_err(|e| e.to_string())?;
                }

                // OPTIMIZATION: Render character if not space (skip spaces with default bg) and not invisible
                if cell.ch != ' ' && !cell.invisible {
                    // Use extended grapheme if present, otherwise use single char
                    let char_str;
                    let text = if let Some(ref extended) = cell.extended {
                        extended.as_ref()
                    } else {
                        char_str = cell.ch.to_string();
                        char_str.as_str()
                    };

                    // Handle reverse video attribute (per-cell reverse, applied after global reverse)
                    // Note: background was already drawn above (lines 280-298) using actual_bg
                    let (fg_r, fg_g, fg_b) = if cell.reverse {
                        (cell_bg.r, cell_bg.g, cell_bg.b)
                    } else {
                        (cell_fg.r, cell_fg.g, cell_fg.b)
                    };

                    // Check if this cell is part of a hovered URL (Ctrl+hover feature)
                    // Also check that the URL belongs to THIS pane to avoid highlighting in wrong pane
                    let is_hovered_url = mouse_state.ctrl_pressed
                        && mouse_state.hovered_url.as_ref().map_or(false, |url| {
                            url.row == row && col >= url.col_start && col <= url.col_end && url.pane_id == pane_id
                        });

                    // Override color to blue for hovered URLs
                    let (fg_r, fg_g, fg_b) = if is_hovered_url {
                        (70, 130, 255) // Blue color for clickable URLs
                    } else {
                        (fg_r, fg_g, fg_b)
                    };

                    // Apply underline if cell has underline attribute OR if it's part of a hovered URL
                    let should_underline = cell.underline || is_hovered_url;

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
                        fg_r,
                        fg_g,
                        fg_b,
                        actual_cell_width as u32,
                        char_height as u32,
                        scale_factor,
                        cell.bold,
                        should_underline,
                        cell.strikethrough,
                    )?;
                }
            }
        }
    }

    // Render cursor if active pane, visible (blink state), and enabled by terminal (ANSI code)
    if should_show_cursor_check {
        let cursor_x = rect.x() + pane_padding as i32 + (sb.cursor_x as f32 * char_width) as i32;
        let cursor_y = rect.y() + pane_padding as i32 + (sb.cursor_y as f32 * char_height) as i32;

        // Cursor style from DECSCUSR control codes
        use crate::screen_buffer::CursorStyle;
        match sb.cursor_style {
            CursorStyle::BlinkingBar | CursorStyle::SteadyBar => {
                // Bar cursor: thin vertical line
                canvas.set_draw_color(Color::RGB(200, 200, 200));
                let cursor_rect = Rect::new(cursor_x, cursor_y, 2, char_height as u32);
                canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;
            }
            CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
                // Underline cursor: horizontal line at bottom
                canvas.set_draw_color(Color::RGB(200, 200, 200));
                let underline_height = (char_height * 0.15).max(2.0) as u32; // 15% of char height, minimum 2px
                let cursor_rect = Rect::new(
                    cursor_x,
                    cursor_y + char_height as i32 - underline_height as i32,
                    char_width as u32,
                    underline_height,
                );
                canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;
            }
            CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock => {
                // Block cursor: use reverse video (invert fg/bg colors)
                if let Some(cell) = sb.get_cell_with_scrollback(sb.cursor_x, sb.cursor_y) {
                    // Draw background with inverted color (use foreground color, or white if fg is default)
                    let cursor_bg = if cell.fg_color.r == 255 && cell.fg_color.g == 255 && cell.fg_color.b == 255 {
                        Color::RGB(255, 255, 255) // Use white for cursor background
                    } else {
                        cell.fg_color
                    };
                    canvas.set_draw_color(cursor_bg);
                    let cursor_rect = Rect::new(cursor_x, cursor_y, char_width as u32, char_height as u32);
                    canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;

                    // Always render the character with inverted color (use background color)
                    // If background is black/dark, render text in black so it shows on white cursor
                    let char_str;
                    let text = if let Some(ref extended) = cell.extended {
                        extended.as_ref()
                    } else {
                        char_str = cell.ch.to_string();
                        char_str.as_str()
                    };

                    // Use background color for text, or dark gray if bg is default black
                    let text_color = if cell.bg_color.r == 0 && cell.bg_color.g == 0 && cell.bg_color.b == 0 {
                        Color::RGB(50, 50, 50) // Dark gray text on white cursor background
                    } else {
                        cell.bg_color
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
                        cursor_x,
                        cursor_y,
                        text_color.r,
                        text_color.g,
                        text_color.b,
                        char_width as u32,
                        char_height as u32,
                        scale_factor,
                        cell.bold,
                        cell.underline,
                        cell.strikethrough,
                    )?;
                } else {
                    // Fallback if cell doesn't exist
                    canvas.set_draw_color(Color::RGB(200, 200, 200));
                    let cursor_rect = Rect::new(cursor_x, cursor_y, char_width as u32, char_height as u32);
                    canvas.fill_rect(cursor_rect).map_err(|e| e.to_string())?;
                }
            }
        }
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
    scale_factor: f32,
    bold: bool,
    underline: bool,
    strikethrough: bool,
) -> Result<(), String> {
    let cache_key = text.to_string();

    // Try custom rendering for specific problematic block characters
    if text.chars().count() == 1 {
        if let Some(ch) = text.chars().next() {
            if custom_cell::can_render_custom(ch) {
                custom_cell::render_custom_cell(canvas, ch, x, y, cell_width, cell_height, r, g, b)?;
                // Draw decorations (underline, strikethrough) for custom-rendered glyphs
                draw_text_decorations(canvas, x, y, cell_width, cell_height, r, g, b, bold, underline, strikethrough)?;
                return Ok(());
            }
        }
    }

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
            // Symbol-range emojis (e.g. ❌ U+274C) get 1.5x scale to match symbol rendering
            let base_size = cell_width.min(cell_height);
            let target_size = if is_special_missing_symbol {
                (base_size as f32 * scale_factor) as u32
            } else {
                base_size
            };

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
            let target_size = (cell_width.min(cell_height) as f32 * scale_factor) as u32;

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

        // Draw decorations (underline, strikethrough) for cached glyphs
        draw_text_decorations(canvas, x, y, cell_width, cell_height, r, g, b, bold, underline, strikethrough)?;

        return Ok(());
    }

    // Render all glyphs in white for color modulation
    let render_color = Color::RGB(255, 255, 255);

    // Check if this is an emoji character - if so, try emoji font FIRST
    let is_likely_emoji = is_emoji_grapheme(text);

    // Check if this is a CJK character - if so, try CJK font FIRST
    let is_likely_cjk = is_cjk_grapheme(text);

    // Check if this is a special symbol (used for scaling decisions below)
    let is_special_missing_symbol = text.chars().count() == 1 && text.chars().next().map_or(false, is_special_symbol);

    if is_likely_emoji {
        // Try emoji font first for emoji characters
        let emoji_result = emoji_font.render(text).blended(render_color);
        if let Ok(surface) = emoji_result {
            if surface.width() > 0 && surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&surface) {
                    // Scale emoji to fill available space (double-width emojis get 2x cell_width)
                    // Use the smaller of width or height to maintain square aspect ratio
                    // Symbol-range emojis (e.g. ❌ U+274C) get 1.5x scale to match symbol rendering
                    let base_size = cell_width.min(cell_height);
                    let target_size = if is_special_missing_symbol {
                        (base_size as f32 * scale_factor) as u32
                    } else {
                        base_size
                    };

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

    // Check if this is a block/box drawing character that needs cell-filling
    let is_block_box_char = text.chars().count() == 1 && text.chars().next().map_or(false, is_block_or_box_drawing);

    // For special symbols that are often missing from terminal fonts, try unicode fallback font first.
    // Use find_glyph to verify the font actually has the glyph before rendering, because SDL_ttf
    // renders a .notdef box (with width > 0) for missing characters, causing false positives.
    let symbol_font_has_glyph = is_special_missing_symbol && !is_likely_emoji
        && text.chars().next().map_or(false, |ch| unicode_fallback_font.find_glyph(ch).is_some());

    if symbol_font_has_glyph {
        let unicode_fallback_result = unicode_fallback_font.render(text).blended(render_color);
        if let Ok(unicode_surface) = unicode_fallback_result {
            if unicode_surface.width() > 0 && unicode_surface.height() > 0 {
                if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&unicode_surface) {
                    let target_size = (cell_width.min(cell_height) as f32 * scale_factor) as u32;

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
    // Use solid rendering for block/box characters to eliminate padding/gaps
    let render_result = if is_block_box_char {
        if text.chars().count() == 1 {
            font.render_char(text.chars().next().unwrap()).solid(render_color)
        } else {
            font.render(text).solid(render_color)
        }
    } else if text.chars().count() == 1 {
        font.render_char(text.chars().next().unwrap()).blended(render_color)
    } else {
        font.render(text).blended(render_color)
    };

    // Try main font first
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
    }

    // Main font failed (Err) or produced empty surface - try fallback fonts
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
    // Skip if we already successfully used it above for special symbols.
    // Also use find_glyph to avoid rendering .notdef boxes for unsupported characters.
    if !symbol_font_has_glyph {
        let has_glyph = text.chars().next()
            .map_or(false, |ch| unicode_fallback_font.find_glyph(ch).is_some());
        if has_glyph {
            let unicode_fallback_result = if is_block_box_char {
                unicode_fallback_font.render(text).solid(render_color)
            } else {
                unicode_fallback_font.render(text).blended(render_color)
            };
            if let Ok(unicode_surface) = unicode_fallback_result {
                if unicode_surface.width() > 0 && unicode_surface.height() > 0 {
                    if let Ok(texture) = texture_creator.create_texture_from_surface::<&sdl3::surface::Surface>(&unicode_surface) {
                        // If this is a block/box drawing character, stretch to fill entire cell
                        if is_block_box_char {
                            let char_rect = Rect::new(x, y, cell_width, cell_height);
                            canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                            glyph_cache.insert(cache_key, texture);
                            return Ok(());
                        } else {
                            let char_rect = Rect::new(x, y, unicode_surface.width(), unicode_surface.height());
                            canvas.copy(&texture, None, char_rect).map_err(|e| e.to_string())?;
                            glyph_cache.insert(cache_key, texture);
                            return Ok(());
                        }
                    }
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

    // Draw decorations for non-cached glyphs
    draw_text_decorations(canvas, x, y, cell_width, cell_height, r, g, b, bold, underline, strikethrough)?;

    Ok(())
}

/// Draw text decorations (underline, strikethrough, bold effect)
fn draw_text_decorations(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    cell_width: u32,
    cell_height: u32,
    r: u8,
    g: u8,
    b: u8,
    _bold: bool,
    underline: bool,
    strikethrough: bool,
) -> Result<(), String> {
    canvas.set_draw_color(Color::RGB(r, g, b));

    // Draw underline
    if underline {
        let underline_y = y + cell_height as i32 - 2;
        let underline_thickness = 1;
        for dy in 0..underline_thickness {
            canvas
                .draw_line(
                    sdl3::rect::Point::new(x, underline_y + dy),
                    sdl3::rect::Point::new(x + cell_width as i32, underline_y + dy),
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // Draw strikethrough
    if strikethrough {
        let strikethrough_y = y + (cell_height as i32 / 2);
        let strikethrough_thickness = 1;
        for dy in 0..strikethrough_thickness {
            canvas
                .draw_line(
                    sdl3::rect::Point::new(x, strikethrough_y + dy),
                    sdl3::rect::Point::new(x + cell_width as i32, strikethrough_y + dy),
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // Note: Bold is typically handled by the font rendering itself or by rendering
    // the text twice with a 1-pixel offset. For now, we rely on the font's bold variant
    // or leave it as-is. SDL3's TTF library should handle bold automatically when
    // we eventually add font style support.

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

/// Render the green microphone indicator in the top-right corner of the active pane.
/// Shows solid green while recording, pulsing yellow while transcribing.
/// Render a microphone icon in the top-right corner of the given pane rect.
///
/// Layout (all sizes in logical pixels, scaled by scale_factor):
///
///   ┌──────────────────────┐  ← dark semi-transparent pill (48 × 48)
///   │         ╭──╮         │
///   │        ╭────╮        │  ← mic capsule head (14 × 22, rounded top)
///   │        │    │        │
///   │        └────┘        │
///   │     ╰──────╯         │  ← curved stand arms (two 6×3 rects)
///   │         ││           │  ← stand pole (4 × 8)
///   │      ━━━━━━━━        │  ← base bar (18 × 3)
///   └──────────────────────┘
fn render_voice_indicator(
    canvas: &mut Canvas<Window>,
    pane_rect: Rect,
    is_transcribing: bool,
    cursor_visible: bool,
) -> Result<(), String> {
    // All sizes are intentionally large so the icon is clearly visible
    const BG_SIZE: u32 = 48;
    const MARGIN: i32 = 8;

    let bx = pane_rect.right() - MARGIN - BG_SIZE as i32;
    let by = pane_rect.top() + MARGIN;
    let cx = bx + BG_SIZE as i32 / 2; // horizontal centre

    // ── dark semi-transparent background pill ────────────────────────────────
    canvas.set_blend_mode(BlendMode::Blend);
    canvas.set_draw_color(Color::RGBA(18, 18, 18, 210));
    canvas.fill_rect(Rect::new(bx, by, BG_SIZE, BG_SIZE))
        .map_err(|e| e.to_string())?;
    canvas.set_blend_mode(BlendMode::None);

    // ── icon colour ──────────────────────────────────────────────────────────
    let color = if is_transcribing {
        if cursor_visible { Color::RGB(230, 190, 0) } else { Color::RGB(110, 90, 0) }
    } else if cursor_visible {
        Color::RGB(0, 220, 60)
    } else {
        Color::RGB(0, 130, 40)
    };
    canvas.set_draw_color(color);

    // ── mic capsule head (14 px wide × 22 px tall, centred, rounded top) ───────
    const HEAD_W: u32 = 14;
    const HEAD_H: u32 = 22;
    let head_x = cx - HEAD_W as i32 / 2;
    let head_y = by + 4;
    // body
    canvas.fill_rect(Rect::new(head_x, head_y + 2, HEAD_W, HEAD_H - 2))
        .map_err(|e| e.to_string())?;
    // rounded top: row 1 — 1 px inset each side
    canvas.fill_rect(Rect::new(head_x + 1, head_y + 1, HEAD_W - 2, 1))
        .map_err(|e| e.to_string())?;
    // rounded top: row 0 — 2 px inset each side
    canvas.fill_rect(Rect::new(head_x + 2, head_y, HEAD_W - 4, 1))
        .map_err(|e| e.to_string())?;

    // ── stand arms: two small rects that suggest the curved bottom ───────────
    //   left arm
    canvas.fill_rect(Rect::new(head_x - 6, head_y + HEAD_H as i32 - 4, 6, 3))
        .map_err(|e| e.to_string())?;
    //   right arm
    canvas.fill_rect(Rect::new(head_x + HEAD_W as i32, head_y + HEAD_H as i32 - 4, 6, 3))
        .map_err(|e| e.to_string())?;

    // ── stand pole ───────────────────────────────────────────────────────────
    const POLE_W: u32 = 4;
    const POLE_H: u32 = 8;
    canvas.fill_rect(Rect::new(cx - POLE_W as i32 / 2, head_y + HEAD_H as i32 - 1, POLE_W, POLE_H))
        .map_err(|e| e.to_string())?;

    // ── base bar ─────────────────────────────────────────────────────────────
    const BASE_W: u32 = 20;
    const BASE_H: u32 = 3;
    canvas.fill_rect(Rect::new(cx - BASE_W as i32 / 2, head_y + HEAD_H as i32 + POLE_H as i32 - 1, BASE_W, BASE_H))
        .map_err(|e| e.to_string())?;

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
