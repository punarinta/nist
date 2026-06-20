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

use crate::ghostty_buffer::{CursorStyle, DEFAULT_BG_COLOR};
use crate::cell::{is_cjk_char, is_emoji_char};
use crate::pane_layout::PaneId;
use crate::ui::glyph_atlas::{GlyphAtlas, GlyphKey, GLYPH_RENDER_COLOR, MAX_GLYPH_CHARS};
use crate::sdl_renderer;
use crate::tab_gui::TabBarGui;
use crate::ui::context_menu::ContextMenu;

/// Cached per-pane render texture.
/// Inactive panes are only re-rendered when `gb.dirty` is set.
/// The texture is cursor-free; the cursor is drawn as a canvas overlay after blitting.
pub struct PaneCacheEntry {
    pub texture: sdl3::render::Texture,
    pub width: u32,
    pub height: u32,
    pub last_is_selected: bool,
    pub last_is_active: bool,
    /// Cursor style from the last render, used for non-redrawn blink frames.
    pub last_cursor_style: CursorStyle,
    /// Per-pane Kitty Graphics texture cache.  Keyed by image_id; entries are
    /// evicted when the corresponding placement is deleted from KittyGraphicsState.
    /// Survives pane texture rebuilds (even on resize) to avoid redundant GPU uploads.
    pub kitty_texture_cache: HashMap<u32, sdl3::render::Texture>,
    /// Content hash of each rendered row (damage tracking). A row whose hash is
    /// unchanged since the last render is skipped — its pixels are already in
    /// the texture — so typing or selection changes redraw only affected rows.
    pub row_hashes: Vec<u64>,
}

impl PaneCacheEntry {
    /// Explicitly destroy all GPU textures held by this entry.
    /// Required because `unsafe_textures` disables automatic Drop for SDL textures.
    pub fn destroy_textures(self) {
        unsafe { self.texture.destroy() };
        for (_, tex) in self.kitty_texture_cache {
            unsafe { tex.destroy() };
        }
    }
}

/// Character data saved from the cell under the cursor (used for block cursor overlay).
pub struct CursorCellChar {
    pub chars: Vec<char>,
    pub fg: Color,
    pub bg: Color,
    pub underline: bool,
    pub strikethrough: bool,
}

/// Cursor rendering data returned by `render_pane` for the caller to draw as a
/// lightweight canvas overlay — completely separate from the pane texture so that
/// cursor blinks do NOT invalidate and re-render the full pane.
pub struct CursorOverlay {
    /// Cursor column in terminal cell coordinates.
    pub col: usize,
    /// Cursor row in terminal cell coordinates.
    pub row: usize,
    pub style: CursorStyle,
    /// Cell data for block cursors (None for bar/underline).
    pub cell: Option<CursorCellChar>,
}

/// FNV-1a offset basis (seed for row damage hashes).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Fold one value into an FNV-1a hash.
#[inline]
fn fnv(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(0x0000_0100_0000_01b3);
}

/// Final draw operations for one cell, buffered per row so the row's content
/// hash can be compared before any SDL call is issued.
struct CellDraw {
    col: usize,
    width_cells: usize,
    chars: [char; MAX_GLYPH_CHARS],
    nchars: u8,
    fg: (u8, u8, u8),
    /// Final background fill color (selection already applied); None = default bg.
    bg: Option<Color>,
    underline: bool,
    strikethrough: bool,
    draw_text: bool,
}

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
///
/// `force_active_redraw`: set when interactive events (mouse, keyboard) occurred
/// this frame — forces the active pane to re-render even if content hasn't changed,
/// so URL highlights and other pointer-driven visuals stay correct.
pub fn render_frame<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    tab_bar: &mut sdl_renderer::TabBar,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    tab_font: &Font,
    cpu_font: &Font,
    terminal_font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    context_menu_font: &Font,
    cpu_usage: f32,
    tab_bar_height: u32,
    char_width: f32,
    char_height: f32,
    cursor_visible: bool,
    glyph_cache: &mut GlyphAtlas,
    pane_cache: &mut HashMap<PaneId, PaneCacheEntry>,
    mouse_state: &crate::input::mouse::MouseState,
    voice_recording: bool,
    voice_transcribing: bool,
    voice_anim_t: f32,
    force_active_redraw: bool,
    is_maximized: bool,
) -> Result<bool, String> {
    let perf_t0 = std::time::Instant::now();
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
    tab_bar.render(canvas, tab_font, cpu_font, texture_creator, window_w, cpu_usage, is_maximized)?;
    let perf_tabbar = perf_t0.elapsed();

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

    // Render each pane using per-pane texture cache.
    // Active pane: re-render when dirty, cursor blink changes, or interactive events occurred.
    // Inactive pane: only re-render when gb.dirty (new bytes or scroll).
    let mut any_dirty = false;
    let mut active_pane_rect: Option<Rect> = None;
    for (pane_id, rect, terminal, is_active, is_selected) in pane_rects {
        if is_active {
            active_pane_rect = Some(rect);
        }

        let pane_w = rect.width();
        let pane_h = rect.height();

        // Peek at dirty + selection state before rendering (without clearing dirty yet).
        // cursor_visible is NOT checked here — the cursor is drawn as a canvas overlay
        // after blitting, so cursor blinks never invalidate the pane texture.
        let (gb_dirty, size_changed, selection_changed, active_changed) = {
            let t = terminal.lock().unwrap();
            let gb = t.ghostty_buffer.lock().unwrap();
            let dirty = gb.dirty;
            let size_chg = pane_cache.get(&pane_id)
                .map_or(true, |e| e.width != pane_w || e.height != pane_h);
            let sel_chg = pane_cache.get(&pane_id)
                .map_or(true, |e| e.last_is_selected != is_selected);
            let act_chg = pane_cache.get(&pane_id)
                .map_or(true, |e| e.last_is_active != is_active);
            (dirty, size_chg, sel_chg, act_chg)
        };

        // Active pane: only re-render when something actually changed.
        // force_active_redraw covers interactive events (mouse/keyboard) that affect
        // URL highlights or selection visuals without setting gb.dirty.
        // Cursor blink is intentionally excluded — it is drawn as an overlay.
        let needs_redraw = gb_dirty || size_changed || selection_changed || active_changed
            || !pane_cache.contains_key(&pane_id)
            || (is_active && force_active_redraw);

        // Anything that invalidates the whole texture (size, border state, first
        // use) disables row-damage skipping for this render.
        let full_redraw = size_changed || selection_changed || active_changed
            || !pane_cache.contains_key(&pane_id);

        // Create or recreate target texture when size changes or first use
        if size_changed || !pane_cache.contains_key(&pane_id) {
            match texture_creator.create_texture_target(None, pane_w, pane_h) {
                Ok(texture) => {
                    // Preserve kitty texture cache across resizes to avoid redundant GPU uploads.
                    let kitty_texture_cache = pane_cache.remove(&pane_id)
                        .map(|e| e.kitty_texture_cache)
                        .unwrap_or_default();
                    pane_cache.insert(pane_id, PaneCacheEntry {
                        texture,
                        width: pane_w,
                        height: pane_h,
                        last_is_selected: is_selected,
                        last_is_active: is_active,
                        last_cursor_style: CursorStyle::default(),
                        kitty_texture_cache,
                        row_hashes: Vec::new(),
                    });
                }
                Err(e) => {
                    // No cache available — fall back to direct rendering
                    eprintln!("[RENDER] Failed to create pane texture: {e}");
                    let mut tmp_kitty_cache = HashMap::new();
                    let mut tmp_row_hashes = Vec::new();
                    let (was_dirty, cursor_overlay) = render_pane(
                        canvas, texture_creator, terminal_font, emoji_font,
                        unicode_fallback_font, cjk_font, rect, terminal.clone(),
                        is_active, is_selected, pane_count, char_width, char_height,
                        glyph_cache, &mut tmp_kitty_cache, &mut tmp_row_hashes, true,
                        mouse_state, pane_id,
                    )?;
                    any_dirty = any_dirty || was_dirty;
                    if is_active && cursor_visible {
                        if let Some(ref ov) = cursor_overlay {
                            draw_cursor_overlay(canvas, texture_creator, terminal_font,
                                emoji_font, unicode_fallback_font, cjk_font, glyph_cache,
                                rect, char_width, char_height, ov)?;
                        }
                    }
                    if let Ok(t) = terminal.try_lock() {
                        if let Ok(gb) = t.ghostty_buffer.try_lock() {
                            if !gb.is_at_bottom() {
                                render_scrollback_indicator(
                                    canvas, texture_creator, terminal_font, rect,
                                    gb.scroll_offset(), get_pane_padding(),
                                )?;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        // Re-render into the cursor-free cached texture when content changed.
        let mut cursor_overlay: Option<CursorOverlay> = None;
        if needs_redraw {
            let mut render_result: Result<(bool, Option<CursorOverlay>), String> = Ok((false, None));
            {
                let entry = pane_cache.get_mut(&pane_id).unwrap();
                entry.last_is_selected = is_selected;
                entry.last_is_active = is_active;
                let texture_rect = Rect::new(0, 0, pane_w, pane_h);
                // Split-borrow entry so we can pass kitty_texture_cache into the closure
                // while with_texture_canvas holds &mut entry.texture.
                let PaneCacheEntry { texture, kitty_texture_cache, row_hashes, .. } = entry;
                canvas.with_texture_canvas(texture, |tc| {
                    render_result = render_pane(
                        tc, texture_creator, terminal_font, emoji_font,
                        unicode_fallback_font, cjk_font, texture_rect, terminal.clone(),
                        is_active, is_selected, pane_count, char_width, char_height,
                        glyph_cache, kitty_texture_cache, row_hashes, full_redraw,
                        mouse_state, pane_id,
                    );
                }).map_err(|e| e.to_string())?;
            }
            let (was_dirty, ov) = render_result?;
            any_dirty = any_dirty || was_dirty;
            // Cache the cursor style so non-redrawn blink frames use the correct shape.
            if let Some(ref o) = ov {
                if let Some(entry) = pane_cache.get_mut(&pane_id) {
                    entry.last_cursor_style = o.style;
                }
            }
            cursor_overlay = ov;
        } else if is_active {
            // Content unchanged but we still need cursor overlay data for the blit below.
            // Read position cheaply; style comes from the last render (cached in PaneCacheEntry).
            if let Ok(t) = terminal.try_lock() {
                if let Ok(gb) = t.ghostty_buffer.try_lock() {
                    if gb.cursor_visible() && gb.is_at_bottom() {
                        let style = pane_cache.get(&pane_id)
                            .map(|e| e.last_cursor_style)
                            .unwrap_or_default();
                        cursor_overlay = Some(CursorOverlay {
                            col: gb.cursor_x(),
                            row: gb.cursor_y(),
                            style,
                            // No cell data without re-rendering: block cursor draws a plain block.
                            // Character rendering is refreshed whenever content changes (gb_dirty).
                            cell: None,
                        });
                    }
                }
            }
        }

        // Blit cursor-free cached texture to the window.
        let entry = pane_cache.get(&pane_id).unwrap();
        canvas.copy(&entry.texture, None, rect).map_err(|e| e.to_string())?;

        // Draw cursor as a lightweight canvas overlay — never touches the pane texture.
        // This is the key fix: cursor blinks cost ~3 SDL calls instead of 10,000+.
        if is_active && cursor_visible {
            if let Some(ref ov) = cursor_overlay {
                draw_cursor_overlay(canvas, texture_creator, terminal_font,
                    emoji_font, unicode_fallback_font, cjk_font, glyph_cache,
                    rect, char_width, char_height, ov)?;
            }
        }

        // Scrollback indicator as a canvas overlay (never baked into the pane
        // texture, so row-damage skipping can't leave it stale or accumulated).
        if let Ok(t) = terminal.try_lock() {
            if let Ok(gb) = t.ghostty_buffer.try_lock() {
                if !gb.is_at_bottom() {
                    render_scrollback_indicator(
                        canvas, texture_creator, terminal_font, rect,
                        gb.scroll_offset(), get_pane_padding(),
                    )?;
                }
            }
        }
    }

    // Render voice input indicator on top of the active pane
    if voice_recording || voice_transcribing {
        if let Some(rect) = active_pane_rect {
            render_voice_indicator(canvas, rect, voice_transcribing, cursor_visible, voice_anim_t)?;
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

    let perf_panes = perf_t0.elapsed();
    canvas.present();
    let perf_total = perf_t0.elapsed();
    if perf_total.as_millis() > 50 {
        eprintln!(
            "[PERF2] tabbar={}ms panes={}ms present={}ms",
            perf_tabbar.as_millis(),
            (perf_panes - perf_tabbar).as_millis(),
            (perf_total - perf_panes).as_millis()
        );
    }
    Ok(any_dirty)
}

/// Render a single pane's terminal content into a cursor-free texture.
/// The cursor is NOT drawn here; the caller draws it as a canvas overlay after blitting.
/// This means cursor blinks never trigger a full pane re-render.
///
/// Returns `(was_dirty, cursor_overlay)`.  `cursor_overlay` is `Some` only for the
/// active pane at the live bottom; the caller decides whether to actually draw it
/// based on the current `cursor_visible` blink state.
fn render_pane<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    rect: Rect,
    terminal: Arc<Mutex<crate::terminal::Terminal>>,
    is_active: bool,
    is_selected: bool,
    pane_count: usize,
    char_width: f32,
    char_height: f32,
    glyph_cache: &mut GlyphAtlas,
    kitty_texture_cache: &mut HashMap<u32, sdl3::render::Texture>,
    row_hashes: &mut Vec<u64>,
    full_redraw: bool,
    mouse_state: &crate::input::mouse::MouseState,
    pane_id: crate::pane_layout::PaneId,
) -> Result<(bool, Option<CursorOverlay>), String> {
    let t = terminal.lock().unwrap();
    let mut gb = t.ghostty_buffer.lock().unwrap();

    // Platform-specific padding
    let pane_padding = get_pane_padding();

    // Calculate how many columns/rows can fit in the pane rect
    let (usable_width, usable_height) = get_usable_dimensions(rect.width(), rect.height());
    let rect_cols = (usable_width as f32 / char_width).floor() as usize;
    let rect_rows = (usable_height as f32 / char_height).floor() as usize;

    let cols = rect_cols.min(gb.width());
    let rows = rect_rows.min(gb.height());

    let selection_snapshot = *t.selection.lock().unwrap();

    let is_at_bottom = gb.is_at_bottom();
    let gb_cursor_vis = gb.cursor_visible();
    let cursor_x = gb.cursor_x();
    let cursor_y = gb.cursor_y();
    let reverse_video = gb.reverse_video_mode();
    let scroll_offset = gb.scroll_offset();
    let scrollback_len = gb.scrollback_len();
    // Whether to collect cursor overlay data (position + cell char under cursor).
    // cursor_visible is NOT checked here — the caller controls blink visibility.
    let collect_cursor = gb_cursor_vis && is_active && is_at_bottom;

    // ── row damage bookkeeping ────────────────────────────────────────────────
    // Seed folds in everything that affects pixels but isn't per-cell data:
    // the viewport scroll position (drives the scrollback indicator) and the
    // Kitty image placements (drawn over rows; any change must erase old pixels).
    let mut seed = FNV_OFFSET;
    fnv(&mut seed, scroll_offset as u64);
    for p in gb.kitty_graphics.placements.iter() {
        fnv(&mut seed, p.image_id as u64);
        fnv(&mut seed, p.abs_row as u64);
        fnv(&mut seed, p.cell_x as u64);
    }

    // row_hashes holds one hash per row plus the previous seed as last element.
    // Panes showing Kitty images always redraw fully: images are drawn over
    // arbitrary rows and partial band erases would corrupt them. (Same cost as
    // the pre-damage-tracking behavior; images are rare.)
    let full = full_redraw
        || row_hashes.len() != rows + 1
        || !gb.kitty_graphics.placements.is_empty();
    let seed_changed = !full && row_hashes[rows] != seed;
    if full {
        row_hashes.clear();
        row_hashes.resize(rows + 1, u64::MAX);
        canvas.set_draw_color(DEFAULT_BG_COLOR);
        canvas.fill_rect(rect).map_err(|e| e.to_string())?;
    } else if seed_changed {
        // Overlays (scrollback indicator, Kitty images) can extend below the
        // last row band; erase that strip so stale overlay pixels don't survive.
        let strip_y = rect.y() + pane_padding as i32 + (rows as f32 * char_height) as i32;
        let strip_h = (rect.y() + rect.height() as i32 - strip_y).max(0) as u32;
        if strip_h > 0 {
            canvas.set_draw_color(DEFAULT_BG_COLOR);
            canvas
                .fill_rect(Rect::new(rect.x(), strip_y, rect.width(), strip_h))
                .map_err(|e| e.to_string())?;
        }
    }
    row_hashes[rows] = seed;

    let (cursor_overlay_data, cursor_style, rows_drawn) = gb.render_with(|ctx| -> Result<(Option<CursorCellChar>, CursorStyle, usize), String> {
        use libghostty_vt::screen::CellWide;
        use libghostty_vt::style::Underline;
        use libghostty_vt::render::CursorVisualStyle as CVS;
        let crate::ghostty_buffer::RenderContext { snapshot, row_iter, cell_iter } = ctx;
        let colors = snapshot.colors().map_err(|e| format!("{e:?}"))?;

        // Determine cursor style from the snapshot we already have, avoiding a
        // second render_state.update() call (which cursor_style() would trigger).
        let blinking = snapshot.cursor_blinking().unwrap_or(true);
        let cs = snapshot.cursor_visual_style().unwrap_or(CVS::Block);
        let cursor_style = match (cs, blinking) {
            (CVS::Bar, true) => CursorStyle::BlinkingBar,
            (CVS::Bar, false) => CursorStyle::SteadyBar,
            (CVS::Block | CVS::BlockHollow, true) => CursorStyle::BlinkingBlock,
            (CVS::Block | CVS::BlockHollow, false) => CursorStyle::SteadyBlock,
            (CVS::Underline, true) => CursorStyle::BlinkingUnderline,
            (CVS::Underline, false) => CursorStyle::SteadyUnderline,
        };
        let is_bar_cursor = matches!(cursor_style, CursorStyle::BlinkingBar | CursorStyle::SteadyBar);

        let mut saved_cursor: Option<CursorCellChar> = None;
        let mut rows_drawn = 0usize;

        let mut row_iteration = row_iter.update(snapshot).map_err(|e| format!("{e:?}"))?;
        let mut row_idx = 0usize;

        // Per-row draw ops, buffered so the row hash can be compared before any
        // SDL call is issued. Reused across rows (no per-row allocation).
        let mut row_cells: Vec<CellDraw> = Vec::with_capacity(cols);

        while let Some(row) = row_iteration.next() {
            if row_idx >= rows {
                break;
            }

            row_cells.clear();
            let mut row_hash = seed;

            let mut cell_iteration = cell_iter.update(row).map_err(|e| format!("{e:?}"))?;
            let mut col_idx = 0usize;

            while let Some(cell) = cell_iteration.next() {
                if col_idx >= cols {
                    break;
                }

                let raw = cell.raw_cell().ok();
                let cell_w = if let Some(ref raw) = raw {
                    match raw.wide() {
                        Ok(CellWide::Wide) => 2usize,
                        Ok(CellWide::SpacerTail | CellWide::SpacerHead) => 0usize,
                        _ => 1usize,
                    }
                } else {
                    1usize
                };

                if cell_w == 0 {
                    col_idx += 1;
                    continue;
                }

                let graphemes = cell.graphemes().unwrap_or_default();
                let style = cell.style().unwrap_or_default();
                let fg_rgb = cell.fg_color().ok().flatten().unwrap_or(colors.foreground);
                let bg_rgb_opt = cell.bg_color().ok().flatten();

                let raw_fg = Color::RGB(fg_rgb.r, fg_rgb.g, fg_rgb.b);
                let raw_bg = bg_rgb_opt
                    .map(|c| Color::RGB(c.r, c.g, c.b))
                    .unwrap_or_else(|| Color::RGB(colors.background.r, colors.background.g, colors.background.b));

                let (cell_fg, cell_bg) = if reverse_video {
                    (raw_bg, raw_fg)
                } else {
                    (raw_fg, raw_bg)
                };
                let actual_bg = if style.inverse { cell_fg } else { cell_bg };

                // Collect cursor cell data for the overlay (block cursors only).
                // The cell is still rendered normally — the overlay will draw on top.
                if collect_cursor && !is_bar_cursor && col_idx == cursor_x && row_idx == cursor_y {
                    let chars = if graphemes.is_empty() {
                        vec![' ']
                    } else {
                        graphemes.clone()
                    };
                    saved_cursor = Some(CursorCellChar {
                        chars,
                        fg: cell_fg,
                        bg: cell_bg,
                        underline: style.underline != Underline::None,
                        strikethrough: style.strikethrough,
                    });
                    // No `continue` — render the cell normally; the cursor overlay covers it.
                }

                let cell_selected = if let Some(ref sel) = selection_snapshot {
                    sel.contains(col_idx, row_idx, scroll_offset, scrollback_len)
                } else {
                    false
                };

                // Final background fill color, if any.
                let bg = if cell_selected {
                    Some(Color::RGB(70, 130, 180))
                } else if actual_bg.r != DEFAULT_BG_COLOR.r
                    || actual_bg.g != DEFAULT_BG_COLOR.g
                    || actual_bg.b != DEFAULT_BG_COLOR.b
                {
                    Some(actual_bg)
                } else {
                    None
                };

                let draw_text = !graphemes.is_empty()
                    && !style.invisible
                    && !(graphemes.len() == 1 && graphemes[0] == ' ');

                if bg.is_none() && !draw_text {
                    col_idx += 1;
                    continue; // nothing to draw; absence is reflected by the hash
                }

                let is_hovered_url = draw_text
                    && mouse_state.ctrl_pressed
                    && mouse_state.hovered_url.as_ref().map_or(false, |url| {
                        url.row == row_idx
                            && col_idx >= url.col_start
                            && col_idx <= url.col_end
                            && url.pane_id == pane_id
                    });

                let (fg_r, fg_g, fg_b) = if is_hovered_url {
                    (70u8, 130u8, 255u8)
                } else if style.inverse {
                    (cell_bg.r, cell_bg.g, cell_bg.b)
                } else {
                    (cell_fg.r, cell_fg.g, cell_fg.b)
                };

                let mut chars = ['\0'; MAX_GLYPH_CHARS];
                let nchars = graphemes.len().min(MAX_GLYPH_CHARS);
                chars[..nchars].copy_from_slice(&graphemes[..nchars]);

                let cd = CellDraw {
                    col: col_idx,
                    width_cells: cell_w,
                    chars,
                    nchars: nchars as u8,
                    fg: (fg_r, fg_g, fg_b),
                    bg,
                    underline: style.underline != Underline::None || is_hovered_url,
                    strikethrough: style.strikethrough,
                    draw_text,
                };

                // Fold the cell's draw ops into the row hash.
                fnv(&mut row_hash, cd.col as u64);
                fnv(&mut row_hash, cd.width_cells as u64);
                for &c in &cd.chars[..nchars] {
                    fnv(&mut row_hash, c as u64);
                }
                fnv(&mut row_hash, ((fg_r as u64) << 16) | ((fg_g as u64) << 8) | fg_b as u64);
                fnv(&mut row_hash, match cd.bg {
                    Some(c) => 0x100_0000 | ((c.r as u64) << 16) | ((c.g as u64) << 8) | c.b as u64,
                    None => 0,
                });
                fnv(
                    &mut row_hash,
                    (cd.underline as u64) | ((cd.strikethrough as u64) << 1) | ((cd.draw_text as u64) << 2),
                );

                row_cells.push(cd);
                col_idx += 1;
            }

            // Skip rows whose pixels are already correct in the texture.
            if !full && row_hashes[row_idx] == row_hash {
                row_idx += 1;
                continue;
            }
            row_hashes[row_idx] = row_hash;
            rows_drawn += 1;

            let band_y = rect.y() + pane_padding as i32 + (row_idx as f32 * char_height) as i32;
            if !full {
                // Erase this row band (a full redraw already cleared everything).
                canvas.set_draw_color(DEFAULT_BG_COLOR);
                canvas
                    .fill_rect(Rect::new(rect.x(), band_y, rect.width(), char_height as u32))
                    .map_err(|e| e.to_string())?;
            }

            // Backgrounds first, then glyphs: consecutive atlas copies batch
            // into a single GPU draw when not interleaved with fill_rects.
            for cd in &row_cells {
                if let Some(bg) = cd.bg {
                    let x = rect.x() + pane_padding as i32 + (cd.col as f32 * char_width) as i32;
                    canvas.set_draw_color(bg);
                    canvas
                        .fill_rect(Rect::new(
                            x,
                            band_y,
                            (char_width * cd.width_cells as f32) as u32,
                            char_height as u32,
                        ))
                        .map_err(|e| e.to_string())?;
                }
            }
            for cd in &row_cells {
                if cd.draw_text {
                    let x = rect.x() + pane_padding as i32 + (cd.col as f32 * char_width) as i32;
                    render_glyph(
                        canvas,
                        texture_creator,
                        font,
                        emoji_font,
                        unicode_fallback_font,
                        cjk_font,
                        glyph_cache,
                        &cd.chars[..cd.nchars as usize],
                        x,
                        band_y,
                        cd.fg.0,
                        cd.fg.1,
                        cd.fg.2,
                        (char_width * cd.width_cells as f32) as u32,
                        char_height as u32,
                        cd.underline,
                        cd.strikethrough,
                    )?;
                }
            }

            row_idx += 1;
        }

        Ok((saved_cursor, cursor_style, rows_drawn))
    })?;

    // Note: the scrollback indicator is drawn by the caller as a canvas overlay
    // (never into the pane texture), so it can't accumulate or go stale.

    // Render Kitty Graphics Protocol images on top of text content. Skipped
    // when nothing was redrawn (the texture already contains them); the
    // deletion frame is covered by the seed change erasing all bands.
    if full || seed_changed || rows_drawn > 0 {
        let scrollback_len = gb.scrollback_len();
        let scroll_offset = gb.scroll_offset();
        render_kitty_images(
            canvas,
            texture_creator,
            &mut gb.kitty_graphics.placements,
            kitty_texture_cache,
            rect,
            pane_padding,
            char_width,
            char_height,
            scrollback_len,
            scroll_offset,
        )?;
    }

    let was_dirty = gb.dirty;
    gb.clear_dirty();
    // Check if more bytes arrived during this frame without processing them.
    let still_dirty = gb.incoming_bytes.try_lock().map_or(false, |b| !b.is_empty());

    // Build cursor overlay data for the caller (drawn on canvas, not in texture).
    let cursor_overlay = if collect_cursor {
        Some(CursorOverlay {
            col: cursor_x,
            row: cursor_y,
            style: cursor_style,
            cell: cursor_overlay_data,
        })
    } else {
        None
    };

    drop(gb);
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

    Ok((was_dirty || still_dirty, cursor_overlay))
}

/// Draw the cursor as a lightweight canvas overlay after the pane texture has been blitted.
/// This runs instead of re-rendering the full pane on every cursor blink tick.
fn draw_cursor_overlay<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    glyph_cache: &mut GlyphAtlas,
    pane_rect: Rect,
    char_width: f32,
    char_height: f32,
    overlay: &CursorOverlay,
) -> Result<(), String> {
    let padding = get_pane_padding() as i32;
    let cx = pane_rect.x() + padding + (overlay.col as f32 * char_width) as i32;
    let cy = pane_rect.y() + padding + (overlay.row as f32 * char_height) as i32;

    match overlay.style {
        CursorStyle::BlinkingBar | CursorStyle::SteadyBar => {
            canvas.set_draw_color(Color::RGB(200, 200, 200));
            canvas.fill_rect(Rect::new(cx, cy, 2, char_height as u32))
                .map_err(|e| e.to_string())?;
        }
        CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
            canvas.set_draw_color(Color::RGB(200, 200, 200));
            let underline_height = (char_height * 0.15).max(2.0) as u32;
            canvas.fill_rect(Rect::new(
                cx,
                cy + char_height as i32 - underline_height as i32,
                char_width as u32,
                underline_height,
            )).map_err(|e| e.to_string())?;
        }
        CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock => {
            if let Some(ref cd) = overlay.cell {
                let cursor_bg = if cd.fg.r == 255 && cd.fg.g == 255 && cd.fg.b == 255 {
                    Color::RGB(255, 255, 255)
                } else {
                    cd.fg
                };
                canvas.set_draw_color(cursor_bg);
                canvas.fill_rect(Rect::new(cx, cy, char_width as u32, char_height as u32))
                    .map_err(|e| e.to_string())?;

                let text_color = if cd.bg.r == 0 && cd.bg.g == 0 && cd.bg.b == 0 {
                    Color::RGB(50, 50, 50)
                } else {
                    cd.bg
                };
                render_glyph(
                    canvas, texture_creator, font, emoji_font,
                    unicode_fallback_font, cjk_font, glyph_cache,
                    &cd.chars, cx, cy,
                    text_color.r, text_color.g, text_color.b,
                    char_width as u32, char_height as u32,
                    cd.underline, cd.strikethrough,
                )?;
            } else {
                // No cell data available (non-redrawn frame) — draw a plain block
                canvas.set_draw_color(Color::RGB(200, 200, 200));
                canvas.fill_rect(Rect::new(cx, cy, char_width as u32, char_height as u32))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Render Kitty Graphics Protocol image placements for a pane.
fn render_kitty_images<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    placements: &mut Vec<crate::kitty_graphics::KittyPlacement>,
    kitty_texture_cache: &mut HashMap<u32, sdl3::render::Texture>,
    rect: Rect,
    pane_padding: u32,
    char_width: f32,
    char_height: f32,
    scrollback_len: usize,
    scroll_offset: usize,
) -> Result<(), String> {
    use sdl3::pixels::PixelFormat;

    // Evict cached textures whose placements were deleted or evicted.
    let valid_ids: std::collections::HashSet<u32> =
        placements.iter().map(|p| p.image_id).collect();
    let evicted: Vec<u32> = kitty_texture_cache.keys()
        .filter(|id| !valid_ids.contains(id))
        .copied()
        .collect();
    for id in evicted {
        if let Some(tex) = kitty_texture_cache.remove(&id) {
            unsafe { tex.destroy() };
        }
    }

    for placement in placements.iter_mut() {
        // Convert absolute row back to a viewport row accounting for scrollback.
        // abs_row = scrollback_len_at_placement + viewport_row_at_placement
        // viewport_row_now = abs_row - scrollback_len_now + scroll_offset_now
        let viewport_row = placement.abs_row as i64
            - scrollback_len as i64
            + scroll_offset as i64;

        let x = rect.x() + pane_padding as i32
            + (placement.cell_x as f32 * char_width) as i32;
        let y = rect.y() + pane_padding as i32
            + (viewport_row as f32 * char_height) as i32;

        // Skip if fully outside the pane vertically
        let display_h_approx = placement.display_rows
            .map(|r| (r as f32 * char_height) as i32)
            .unwrap_or(placement.pixel_height as i32);
        if y >= rect.y() + rect.height() as i32 || y + display_h_approx <= rect.y() {
            continue;
        }
        if x >= rect.x() + rect.width() as i32 {
            continue;
        }

        let display_w = placement
            .display_cols
            .map(|c| (c as f32 * char_width) as u32)
            .unwrap_or(placement.pixel_width);
        let display_h = placement
            .display_rows
            .map(|r| (r as f32 * char_height) as u32)
            .unwrap_or(placement.pixel_height);

        // Upload to GPU only if not already cached.
        if !kitty_texture_cache.contains_key(&placement.image_id) {
            let pitch = placement.pixel_width * 4;
            let surface = sdl3::surface::Surface::from_data(
                &mut placement.rgba_data,
                placement.pixel_width,
                placement.pixel_height,
                pitch,
                PixelFormat::RGBA32,
            )
            .map_err(|e| e.to_string())?;
            let texture = texture_creator
                .create_texture_from_surface::<&sdl3::surface::Surface>(&surface)
                .map_err(|e| e.to_string())?;
            kitty_texture_cache.insert(placement.image_id, texture);
        }

        let texture = kitty_texture_cache.get(&placement.image_id).unwrap();
        canvas
            .copy(texture, None, Rect::new(x, y, display_w, display_h))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Render a single grapheme cluster via the shared glyph atlas.
/// Every copy references an atlas page, so SDL3 batches consecutive cells
/// into a handful of GPU draws instead of one draw + texture switch per cell.
fn render_glyph<T>(
    canvas: &mut Canvas<Window>,
    texture_creator: &TextureCreator<T>,
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    glyph_cache: &mut GlyphAtlas,
    chars: &[char],
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
    cell_width: u32,
    cell_height: u32,
    underline: bool,
    strikethrough: bool,
) -> Result<(), String> {
    let key = GlyphKey::from_chars(chars);
    let is_likely_emoji = chars.iter().copied().any(is_emoji_char);

    let loc = match glyph_cache.lookup(&key) {
        Some(cached) => cached,
        None => {
            // Atlas miss: rasterize through the font fallback chain.
            // This is the only place a String is allocated for a glyph.
            let text: String = chars.iter().take(MAX_GLYPH_CHARS).collect();
            let is_likely_cjk = chars.iter().copied().any(is_cjk_char);
            match rasterize_glyph(
                font, emoji_font, unicode_fallback_font, cjk_font,
                &text, is_likely_emoji, is_likely_cjk,
            ) {
                Some(surface) => glyph_cache.insert(canvas, texture_creator, key, &surface)?,
                None => {
                    glyph_cache.insert_failed(key);
                    None
                }
            }
        }
    };

    let Some(loc) = loc else {
        // Unrenderable in every font: draw the replacement box instead.
        if !(chars.len() == 1 && chars[0] == '□') {
            render_glyph(
                canvas, texture_creator, font, emoji_font, unicode_fallback_font,
                cjk_font, glyph_cache, &['□'], x, y, r, g, b,
                cell_width, cell_height, underline, strikethrough,
            )?;
        }
        return Ok(());
    };

    let (qw, qh) = (loc.rect.width(), loc.rect.height());
    let dst = if is_likely_emoji {
        // Scale emoji to fit the cell, centered.
        let base_size = cell_width.min(cell_height);
        let scale = (base_size as f32 / qw as f32).min(base_size as f32 / qh as f32);
        let sw = (qw as f32 * scale) as u32;
        let sh = (qh as f32 * scale) as u32;
        Rect::new(
            x + (cell_width as i32 - sw as i32) / 2,
            y + (cell_height as i32 - sh as i32) / 2,
            sw,
            sh,
        )
    } else if qw > cell_width || qh > cell_height {
        let scale = (cell_width as f32 / qw as f32).min(cell_height as f32 / qh as f32);
        Rect::new(x, y, (qw as f32 * scale) as u32, (qh as f32 * scale) as u32)
    } else {
        Rect::new(x, y, qw, qh)
    };
    glyph_cache.draw(canvas, loc, dst, r, g, b)?;

    draw_text_decorations(canvas, x, y, cell_width, cell_height, r, g, b, underline, strikethrough)
}

/// Rasterize a grapheme through the font fallback chain
/// (emoji → CJK → main → emoji-fallback → CJK-fallback → symbols → '□').
fn rasterize_glyph(
    font: &Font,
    emoji_font: &Font,
    unicode_fallback_font: &Font,
    cjk_font: &Font,
    text: &str,
    is_likely_emoji: bool,
    is_likely_cjk: bool,
) -> Option<sdl3::surface::Surface<'static>> {
    let first = text.chars().next()?;
    let ok = |s: &sdl3::surface::Surface| s.width() > 0 && s.height() > 0;

    if is_likely_emoji && emoji_font.find_glyph(first).is_some() {
        if let Ok(s) = emoji_font.render(text).blended(GLYPH_RENDER_COLOR) {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    if is_likely_cjk {
        if let Ok(s) = cjk_font.render(text).blended(GLYPH_RENDER_COLOR) {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    if font.find_glyph(first).is_some() {
        let rendered = if text.chars().count() == 1 {
            font.render_char(first).blended(GLYPH_RENDER_COLOR)
        } else {
            font.render(text).blended(GLYPH_RENDER_COLOR)
        };
        if let Ok(s) = rendered {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    if !is_likely_emoji && emoji_font.find_glyph(first).is_some() {
        if let Ok(s) = emoji_font.render(text).blended(GLYPH_RENDER_COLOR) {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    if !is_likely_cjk && cjk_font.find_glyph(first).is_some() {
        if let Ok(s) = cjk_font.render(text).blended(GLYPH_RENDER_COLOR) {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    if unicode_fallback_font.find_glyph(first).is_some() {
        if let Ok(s) = unicode_fallback_font.render(text).blended(GLYPH_RENDER_COLOR) {
            if ok(&s) {
                return Some(s);
            }
        }
    }
    // Last resort: replacement box (cached under the original key so the
    // fallback chain doesn't re-run every frame for unknown glyphs).
    if let Ok(s) = font.render_char('□').blended(GLYPH_RENDER_COLOR) {
        if ok(&s) {
            return Some(s);
        }
    }
    None
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
            unsafe { texture.destroy() };
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
    voice_anim_t: f32,
) -> Result<(), String> {
    // Size pulse: ±25% over a 0.8-second period
    let pulse = 1.0_f32 + 0.25 * (voice_anim_t * std::f32::consts::TAU / 0.8).sin();

    // Base sizes (all intentionally large so the icon is clearly visible)
    const BG_SIZE_BASE: f32 = 48.0;
    const MARGIN: i32 = 8;

    let bg_size = (BG_SIZE_BASE * pulse) as u32;

    // Anchor the centre of the pill to a fixed point in the top-right corner
    let center_x = pane_rect.right() - MARGIN - (BG_SIZE_BASE / 2.0) as i32;
    let center_y = pane_rect.top() + MARGIN + (BG_SIZE_BASE / 2.0) as i32;

    let bx = center_x - bg_size as i32 / 2;
    let by = center_y - bg_size as i32 / 2;

    // ── dark semi-transparent background pill ────────────────────────────────
    canvas.set_blend_mode(BlendMode::Blend);
    canvas.set_draw_color(Color::RGBA(18, 18, 18, 210));
    canvas.fill_rect(Rect::new(bx, by, bg_size, bg_size))
        .map_err(|e| e.to_string())?;
    canvas.set_blend_mode(BlendMode::None);

    // ── icon colour ──────────────────────────────────────────────────────────
    let color = if is_transcribing {
        if cursor_visible { Color::RGB(230, 190, 0) } else { Color::RGB(110, 90, 0) }
    } else {
        Color::RGB(0, 220, 60)
    };
    canvas.set_draw_color(color);

    // Scale all icon elements by the same pulse factor
    let head_w = (14.0 * pulse) as i32;
    let head_h = (22.0 * pulse) as i32;
    let head_x = center_x - head_w / 2;
    let head_y = by + (4.0 * pulse) as i32;

    // ── mic capsule head (scaled, centred, rounded top) ──────────────────────
    // body
    canvas.fill_rect(Rect::new(head_x, head_y + (2.0 * pulse) as i32,
                               head_w as u32, (head_h - (2.0 * pulse) as i32) as u32))
        .map_err(|e| e.to_string())?;
    // rounded top: row 1 — 1 px inset each side
    canvas.fill_rect(Rect::new(head_x + 1, head_y + 1, (head_w - 2) as u32, 1))
        .map_err(|e| e.to_string())?;
    // rounded top: row 0 — 2 px inset each side
    canvas.fill_rect(Rect::new(head_x + 2, head_y, (head_w - 4) as u32, 1))
        .map_err(|e| e.to_string())?;

    // ── stand arms ───────────────────────────────────────────────────────────
    let arm_w = (6.0 * pulse) as u32;
    let arm_h = (3.0 * pulse).max(1.0) as u32;
    let arm_y = head_y + head_h - (4.0 * pulse) as i32;
    canvas.fill_rect(Rect::new(head_x - arm_w as i32, arm_y, arm_w, arm_h))
        .map_err(|e| e.to_string())?;
    canvas.fill_rect(Rect::new(head_x + head_w, arm_y, arm_w, arm_h))
        .map_err(|e| e.to_string())?;

    // ── stand pole ───────────────────────────────────────────────────────────
    let pole_w = (4.0 * pulse).max(1.0) as u32;
    let pole_h = (8.0 * pulse).max(1.0) as u32;
    canvas.fill_rect(Rect::new(center_x - pole_w as i32 / 2,
                               head_y + head_h - 1, pole_w, pole_h))
        .map_err(|e| e.to_string())?;

    // ── base bar ─────────────────────────────────────────────────────────────
    let base_w = (20.0 * pulse).max(1.0) as u32;
    let base_h = (3.0 * pulse).max(1.0) as u32;
    canvas.fill_rect(Rect::new(center_x - base_w as i32 / 2,
                               head_y + head_h + pole_h as i32 - 1, base_w, base_h))
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
