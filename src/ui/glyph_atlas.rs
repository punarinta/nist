//! Shelf-packed glyph atlas.
//!
//! All terminal glyphs live in a small number of large render-target textures.
//! Cell rendering then issues `canvas.copy` calls that all reference the same
//! texture, which SDL3's render backend batches into a single GPU draw — with
//! one texture per glyph (the previous design) every cell was a texture switch
//! and a separate draw call.
//!
//! Eviction: when every page is full the whole atlas is reset and glyphs
//! re-rasterize lazily over the next frames. This bounds memory without the
//! multi-hundred-millisecond freeze of re-rendering everything in one frame.

use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, Canvas, Texture, TextureCreator};
use sdl3::surface::Surface;
use sdl3::video::Window;
use std::collections::HashMap;

/// Maximum chars of a grapheme cluster participating in the cache key.
/// Longer clusters are truncated (vanishingly rare in terminal output).
pub const MAX_GLYPH_CHARS: usize = 7;

const MAX_PAGES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    chars: [char; MAX_GLYPH_CHARS],
    len: u8,
}

impl GlyphKey {
    pub fn from_chars(chars: &[char]) -> Self {
        let mut arr = ['\0'; MAX_GLYPH_CHARS];
        let n = chars.len().min(MAX_GLYPH_CHARS);
        arr[..n].copy_from_slice(&chars[..n]);
        GlyphKey { chars: arr, len: n as u8 }
    }
}

/// Location of a packed glyph: page index plus source rect within that page.
#[derive(Clone, Copy)]
pub struct GlyphLoc {
    pub page: u16,
    pub rect: Rect,
}

pub struct GlyphAtlas {
    pages: Vec<Texture>,
    /// `None` value = glyph was tried and is unrenderable in every font.
    map: HashMap<GlyphKey, Option<GlyphLoc>>,
    page_size: u32,
    cur_page: usize,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Number of full-atlas resets (telemetry).
    pub resets: u64,
}

impl GlyphAtlas {
    pub fn new(page_size: u32) -> Self {
        GlyphAtlas {
            pages: Vec::new(),
            map: HashMap::new(),
            page_size,
            cur_page: 0,
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            resets: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Forget all glyphs (font size change). Page textures are kept and
    /// overwritten lazily; stale pixels are never referenced.
    pub fn clear(&mut self) {
        self.map.clear();
        self.cur_page = 0;
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_h = 0;
    }

    pub fn lookup(&self, key: &GlyphKey) -> Option<Option<GlyphLoc>> {
        self.map.get(key).copied()
    }

    pub fn insert_failed(&mut self, key: GlyphKey) {
        self.map.insert(key, None);
    }

    /// Find space on the current page for a `w` x `h` block (padding included),
    /// advancing the shelf as needed. Returns the top-left position.
    fn find_spot(&mut self, w: u32, h: u32) -> Option<(i32, i32)> {
        if self.shelf_x + w > self.page_size {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + h > self.page_size {
            return None;
        }
        let pos = (self.shelf_x as i32, self.shelf_y as i32);
        self.shelf_x += w;
        self.shelf_h = self.shelf_h.max(h);
        Some(pos)
    }

    /// Pack a rasterized glyph surface into the atlas and remember its location.
    pub fn insert<T>(
        &mut self,
        canvas: &mut Canvas<Window>,
        texture_creator: &TextureCreator<T>,
        key: GlyphKey,
        surface: &Surface,
    ) -> Result<Option<GlyphLoc>, String> {
        let gw = surface.width();
        let gh = surface.height();
        // 1px padding on each side prevents sampling bleed when glyphs are scaled.
        if gw == 0 || gh == 0 || gw + 2 > self.page_size || gh + 2 > self.page_size {
            self.map.insert(key, None);
            return Ok(None);
        }

        let mut did_reset = false;
        let (x, y) = loop {
            if let Some(pos) = self.find_spot(gw + 2, gh + 2) {
                break pos;
            }
            if self.cur_page + 1 < MAX_PAGES {
                self.cur_page += 1;
                self.shelf_x = 0;
                self.shelf_y = 0;
                self.shelf_h = 0;
            } else if !did_reset {
                // Every page is full: reset; glyphs re-rasterize lazily.
                self.clear();
                self.resets += 1;
                did_reset = true;
            } else {
                self.map.insert(key, None);
                return Ok(None);
            }
        };

        while self.pages.len() <= self.cur_page {
            // Explicit RGBA format: the default render-target format has no
            // alpha channel, which would flatten glyph coverage to solid rects.
            let mut tex = texture_creator
                .create_texture_target(
                    Some(sdl3::pixels::PixelFormat::RGBA32),
                    self.page_size,
                    self.page_size,
                )
                .map_err(|e| e.to_string())?;
            tex.set_blend_mode(BlendMode::Blend);
            self.pages.push(tex);
        }

        // Upload: temp texture from the surface, copied into the page with
        // blending OFF so the page stores the glyph's straight (non-premultiplied)
        // RGBA exactly as rasterized.
        let mut temp = texture_creator
            .create_texture_from_surface(surface)
            .map_err(|e| e.to_string())?;
        temp.set_blend_mode(BlendMode::None);
        let dst = Rect::new(x + 1, y + 1, gw, gh);
        canvas
            .with_texture_canvas(&mut self.pages[self.cur_page], |tc| {
                let _ = tc.copy(&temp, None, dst);
            })
            .map_err(|e| e.to_string())?;
        unsafe { temp.destroy() };

        let loc = GlyphLoc { page: self.cur_page as u16, rect: dst };
        self.map.insert(key, Some(loc));
        Ok(Some(loc))
    }

    /// Copy a glyph from the atlas to `dst` with the given color modulation.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas<Window>,
        loc: GlyphLoc,
        dst: Rect,
        r: u8,
        g: u8,
        b: u8,
    ) -> Result<(), String> {
        let page = &mut self.pages[loc.page as usize];
        page.set_color_mod(r, g, b);
        canvas.copy(page, loc.rect, dst).map_err(|e| e.to_string())
    }
}

/// White, so the atlas entry can be tinted per-draw via color modulation.
pub const GLYPH_RENDER_COLOR: Color = Color::RGB(255, 255, 255);
