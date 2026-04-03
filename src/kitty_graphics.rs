//! Kitty Graphics Protocol support.
//!
//! Parses APC-G sequences (ESC _ G ... ESC \) from the byte stream,
//! accumulates chunked image transmissions, decodes PNG/RGB/RGBA data,
//! and stores placements for the renderer.
//!
//! Protocol spec: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::collections::HashMap;

use base64::Engine as _;

/// Maximum number of decoded image placements kept in memory per terminal.
/// Oldest placements are evicted when this limit is exceeded.
pub const MAX_KITTY_PLACEMENTS: usize = 20;

/// Maximum number of in-progress (multi-chunk) image transmissions buffered
/// per terminal.  Incomplete transfers beyond this limit are silently dropped.
const MAX_PENDING_IMAGES: usize = 50;

/// A decoded, positioned image placement ready for rendering.
pub struct KittyPlacement {
    /// Raw RGBA pixels (R, G, B, A bytes, row-major).
    pub rgba_data: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Viewport cell column where the image starts.
    pub cell_x: u16,
    /// Absolute row = scrollback_len_at_placement + viewport_row.
    /// Used to correctly reposition the image as the terminal scrolls.
    pub abs_row: usize,
    /// Display width override in cells (`c` param), None = use pixel_width.
    pub display_cols: Option<u32>,
    /// Display height override in cells (`r` param), None = use pixel_height.
    pub display_rows: Option<u32>,
    pub image_id: u32,
}

/// A pending (multi-chunk) image transmission.
struct PendingImage {
    format: u32,
    src_w: Option<u32>,
    src_h: Option<u32>,
    raw_data: Vec<u8>,
    cell_x: u16,
    abs_row: usize,
    display_cols: Option<u32>,
    display_rows: Option<u32>,
    action: u8, // b'T' or b't'
    transmission: u8, // b'd'=direct, b'f'=file, b't'=temp file, b's'=shm
}

/// Per-terminal Kitty graphics state.
pub struct KittyGraphicsState {
    pending: HashMap<u32, PendingImage>,
    /// Completed placements, rendered every frame.
    pub placements: Vec<KittyPlacement>,
    next_id: u32,
}

impl KittyGraphicsState {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            placements: Vec::new(),
            next_id: 1,
        }
    }

    /// Process one raw Kitty APC-G sequence (including surrounding ESC_G...ST
    /// bytes).  `cursor_x`/`cursor_y` are the viewport cell coordinates of the
    /// cursor *before* this sequence was fed to the VT parser.
    ///
    /// Returns bytes that should be written back to the PTY (e.g. OK response),
    /// or `None`.
    pub fn process_raw_sequence(
        &mut self,
        seq: &[u8],
        cursor_x: u16,
        cursor_y: u16,
        scrollback_len: usize,
    ) -> Option<Vec<u8>> {
        // Expected prefix: ESC _ G  (0x1b 0x5f 0x47)
        if seq.len() < 3 { return None; }

        // Trim suffix: ESC \ (0x1b 0x5c), C1 ST (0x9c), or BEL (0x07)
        let inner_end = if seq.ends_with(b"\x1b\\") {
            seq.len() - 2
        } else if seq.last().map_or(false, |&b| b == 0x07 || b == 0x9c) {
            seq.len() - 1
        } else {
            seq.len()
        };

        // inner = everything between ESC_G and ST
        let inner = if inner_end > 3 { &seq[3..inner_end] } else { b"" };

        // Split params from data at the first ';'
        let (params_bytes, data_b64) = if let Some(pos) = inner.iter().position(|&b| b == b';') {
            (&inner[..pos], &inner[pos + 1..])
        } else {
            (inner, b"" as &[u8])
        };

        let params = parse_params(params_bytes);

        let action = params.get("a").map(|s| s.as_bytes().first().copied().unwrap_or(b'T')).unwrap_or(b'T');
        let image_id: u32 = params.get("i").and_then(|s| s.parse().ok()).unwrap_or(0);
        let quiet: u8 = params.get("q").and_then(|s| s.parse().ok()).unwrap_or(0);
        let more: bool = params.get("m").map(|s| s == "1").unwrap_or(false);

        match action {
            // ── Query ────────────────────────────────────────────────────────
            b'q' => {
                let resp_id = if image_id > 0 { image_id } else { self.next_id };
                if quiet < 2 {
                    return Some(make_ok_response(resp_id));
                }
            }

            // ── Transmit (+ optionally display) ──────────────────────────────
            b'T' | b't' => {
                let format: u32 = params.get("f").and_then(|s| s.parse().ok()).unwrap_or(32);
                let src_w: Option<u32> = params.get("s").and_then(|s| s.parse().ok());
                let src_h: Option<u32> = params.get("v").and_then(|s| s.parse().ok());
                let display_cols: Option<u32> = params.get("c").and_then(|s| s.parse().ok());
                let display_rows: Option<u32> = params.get("r").and_then(|s| s.parse().ok());
                let transmission: u8 = params.get("t")
                    .and_then(|s| s.as_bytes().first().copied())
                    .unwrap_or(b'd');

                let actual_id = if image_id > 0 {
                    image_id
                } else {
                    let id = self.next_id;
                    self.next_id += 1;
                    id
                };

                let decoded = base64::engine::general_purpose::STANDARD.decode(data_b64).unwrap_or_default();

                let abs_row = scrollback_len + cursor_y as usize;

                if more {
                    // Accumulate chunk — but cap pending transmissions to avoid
                    // unbounded memory growth from clients that never finalise.
                    if self.pending.len() < MAX_PENDING_IMAGES
                        || self.pending.contains_key(&actual_id)
                    {
                        let pending = self.pending.entry(actual_id).or_insert_with(|| PendingImage {
                            format,
                            src_w,
                            src_h,
                            raw_data: Vec::new(),
                            cell_x: cursor_x,
                            abs_row,
                            display_cols,
                            display_rows,
                            action,
                            transmission,
                        });
                        pending.raw_data.extend_from_slice(&decoded);
                    }
                    // Don't respond until complete
                    return None;
                }

                // Final (or only) chunk — assemble from pending or use directly
                let (raw_payload, final_format, final_src_w, final_src_h,
                     final_cx, final_abs_row, final_cols, final_rows, final_action, final_transmission) =
                    if let Some(mut p) = self.pending.remove(&actual_id) {
                        p.raw_data.extend_from_slice(&decoded);
                        (p.raw_data, p.format, p.src_w, p.src_h,
                         p.cell_x, p.abs_row, p.display_cols, p.display_rows, p.action, p.transmission)
                    } else {
                        (decoded, format, src_w, src_h,
                         cursor_x, abs_row, display_cols, display_rows, action, transmission)
                    };

                // Resolve the actual image bytes based on transmission medium.
                let final_bytes = match final_transmission {
                    b'f' | b't' => {
                        // raw_payload is the base64-decoded file path
                        let path = std::str::from_utf8(&raw_payload).unwrap_or("").trim_end_matches('\0');
                        std::fs::read(path).unwrap_or_default()
                    }
                    b's' => {
                        // raw_payload is a shared-memory object name; open via /dev/shm
                        let name = std::str::from_utf8(&raw_payload).unwrap_or("").trim_matches('\0');
                        let shm_path = if name.starts_with('/') {
                            format!("/dev/shm{}", name)
                        } else {
                            format!("/dev/shm/{}", name)
                        };
                        std::fs::read(&shm_path).unwrap_or_default()
                    }
                    _ => raw_payload, // b'd' = direct inline data
                };

                if let Some(placement) = decode_image(
                    actual_id, &final_bytes, final_format,
                    final_src_w, final_src_h,
                    final_cx, final_abs_row, final_cols, final_rows,
                ) {
                    if final_action == b'T' {
                        self.placements.push(placement);
                        // Evict oldest placements to keep memory bounded.
                        if self.placements.len() > MAX_KITTY_PLACEMENTS {
                            let excess = self.placements.len() - MAX_KITTY_PLACEMENTS;
                            self.placements.drain(..excess);
                        }
                    }
                    // 't' = transmit only; could store for later `p` action
                }

                if quiet < 2 {
                    return Some(make_ok_response(actual_id));
                }
            }

            // ── Delete ───────────────────────────────────────────────────────
            b'd' => {
                let what = params.get("d").map(|s| s.as_str()).unwrap_or("A");
                match what {
                    "A" | "a" => self.placements.clear(),
                    "I" | "i" if image_id > 0 => {
                        self.placements.retain(|p| p.image_id != image_id);
                    }
                    _ => {}
                }
            }

            _ => {}
        }

        None
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_params(data: &[u8]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let s = std::str::from_utf8(data).unwrap_or("");
    for part in s.split(',') {
        if let Some(eq) = part.find('=') {
            let k = part[..eq].trim().to_string();
            let v = part[eq + 1..].trim().to_string();
            if !k.is_empty() {
                map.insert(k, v);
            }
        }
    }
    map
}

fn make_ok_response(image_id: u32) -> Vec<u8> {
    format!("\x1b_Gi={};OK\x1b\\", image_id).into_bytes()
}

fn decode_image(
    image_id: u32,
    data: &[u8],
    format: u32,
    src_w: Option<u32>,
    src_h: Option<u32>,
    cell_x: u16,
    abs_row: usize,
    display_cols: Option<u32>,
    display_rows: Option<u32>,
) -> Option<KittyPlacement> {
    if data.is_empty() {
        return None;
    }
    match format {
        100 => {
            // PNG (or any format the `image` crate understands)
            let img = image::load_from_memory(data).ok()?;
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Some(KittyPlacement {
                rgba_data: rgba.into_raw(),
                pixel_width: w,
                pixel_height: h,
                cell_x,
                abs_row,
                display_cols,
                display_rows,
                image_id,
            })
        }
        32 => {
            // Raw RGBA
            let w = src_w?;
            let h = src_h?;
            let expected = (w * h * 4) as usize;
            if data.len() < expected {
                return None;
            }
            Some(KittyPlacement {
                rgba_data: data[..expected].to_vec(),
                pixel_width: w,
                pixel_height: h,
                cell_x,
                abs_row,
                display_cols,
                display_rows,
                image_id,
            })
        }
        24 => {
            // Raw RGB → RGBA
            let w = src_w?;
            let h = src_h?;
            let expected = (w * h * 3) as usize;
            if data.len() < expected {
                return None;
            }
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for px in data[..expected].chunks_exact(3) {
                rgba.push(px[0]);
                rgba.push(px[1]);
                rgba.push(px[2]);
                rgba.push(255);
            }
            Some(KittyPlacement {
                rgba_data: rgba,
                pixel_width: w,
                pixel_height: h,
                cell_x,
                abs_row,
                display_cols,
                display_rows,
                image_id,
            })
        }
        _ => None,
    }
}

// ── byte-stream scanning ──────────────────────────────────────────────────────

/// Find all Kitty APC-G sequences in `bytes`.
/// Returns `(start, end)` pairs where `end` is exclusive (past the terminator).
pub fn find_kitty_sequences(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        // APC start: ESC _ G  (0x1b 0x5f 0x47)
        if bytes[i] == 0x1b && bytes[i + 1] == 0x5f && bytes[i + 2] == 0x47 {
            let start = i;
            i += 3;
            let mut found = false;
            while i < bytes.len() {
                if bytes[i] == 0x07 {
                    // BEL terminator
                    result.push((start, i + 1));
                    i += 1;
                    found = true;
                    break;
                } else if bytes[i] == 0x9c {
                    // C1 ST
                    result.push((start, i + 1));
                    i += 1;
                    found = true;
                    break;
                } else if i + 1 < bytes.len() && bytes[i] == 0x1b && bytes[i + 1] == 0x5c {
                    // ESC \ terminator
                    result.push((start, i + 2));
                    i += 2;
                    found = true;
                    break;
                } else {
                    i += 1;
                }
            }
            if !found {
                // Incomplete sequence at end of buffer – skip
            }
        } else {
            i += 1;
        }
    }
    result
}
