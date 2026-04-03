use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::{Mode, PointCoordinate, ScrollViewport};
use libghostty_vt::{RenderState, Terminal, TerminalOptions};
use sdl3::pixels::Color;
use std::sync::{mpsc, Arc, Mutex};

use crate::kitty_graphics::KittyGraphicsState;

/// Cursor style as set by DECSCUSR escape sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl CursorStyle {
    /// Convert from settings string ("pipe", "block", etc.) to CursorStyle.
    pub fn from_settings_string(s: &str) -> Self {
        match s {
            "pipe" | "bar" => CursorStyle::SteadyBar,
            "underline" => CursorStyle::SteadyUnderline,
            "block" => CursorStyle::SteadyBlock,
            "blinking_block" => CursorStyle::BlinkingBlock,
            "blinking_bar" | "blinking_pipe" => CursorStyle::BlinkingBar,
            "blinking_underline" => CursorStyle::BlinkingUnderline,
            _ => CursorStyle::SteadyBar,
        }
    }

    /// DECSCUSR escape sequence bytes to set this cursor style in the terminal.
    fn decscusr_bytes(self) -> &'static [u8] {
        match self {
            CursorStyle::BlinkingBlock => b"\x1b[1 q",
            CursorStyle::SteadyBlock => b"\x1b[2 q",
            CursorStyle::BlinkingUnderline => b"\x1b[3 q",
            CursorStyle::SteadyUnderline => b"\x1b[4 q",
            CursorStyle::BlinkingBar => b"\x1b[5 q",
            CursorStyle::SteadyBar => b"\x1b[6 q",
        }
    }
}

pub const DEFAULT_BG_COLOR: Color = Color::RGB(20, 20, 20);

/// Context passed to the `render_with` closure.
pub struct RenderContext<'snap, 'alloc> {
    pub snapshot: &'snap libghostty_vt::render::Snapshot<'alloc, 'snap>,
    pub row_iter: &'snap mut RowIterator<'alloc>,
    pub cell_iter: &'snap mut CellIterator<'alloc>,
}

/// Mouse tracking mode, kept here after MouseTrackingMode is removed from
/// terminal/main.rs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseTrackingMode {
    Disabled,
    X10,
    VT200Normal,
    ButtonEvent,
    AnyEvent,
}

/// Wraps the libghostty terminal plus its render infrastructure and the
/// incoming-bytes channel used by the background PTY reader thread.
///
/// # Safety
///
/// `libghostty_vt::Terminal` is `!Send + !Sync`.  We mark `GhosttyBuffer`
/// as `Send + Sync` because, in practice, the `terminal` field is only ever
/// accessed from the **main render thread**.  The background reader thread
/// only touches `incoming_bytes`, which is itself a safe `Arc<Mutex<Vec<u8>>>`.
pub struct GhosttyBuffer {
    pub terminal: Box<Terminal<'static, 'static>>,
    render_state: RenderState<'static>,
    row_iter: RowIterator<'static>,
    cell_iter: CellIterator<'static>,
    /// Incoming raw bytes from the PTY reader thread.
    pub incoming_bytes: Arc<Mutex<Vec<u8>>>,
    /// Dirty flag – set true when bytes are processed.
    pub dirty: bool,
    width: u16,
    height: u16,
    /// Cell pixel dimensions for pixel-aware terminal queries (XTWINOPS).
    cell_width_px: u32,
    cell_height_px: u32,
    /// Shared size state read by the XTWINOPS size callback.
    /// Tuple: (rows, cols, cell_width_px, cell_height_px).
    size_state: Arc<Mutex<(u16, u16, u32, u32)>>,
    /// Non-blocking channel for sending responses back to the PTY.
    /// The dedicated writer thread drains this and does the actual write_all.
    pty_write_tx: mpsc::SyncSender<Vec<u8>>,
    /// Kitty Graphics Protocol state.
    pub kitty_graphics: KittyGraphicsState,
}

// SAFETY: `terminal` is only accessed from the main render thread.
// The background thread only touches `incoming_bytes`.
unsafe impl Send for GhosttyBuffer {}
unsafe impl Sync for GhosttyBuffer {}

impl GhosttyBuffer {
    pub fn new(
        initial_cols: u16,
        initial_rows: u16,
        max_scrollback: usize,
        pty_write_tx: mpsc::SyncSender<Vec<u8>>,
        default_cursor: CursorStyle,
    ) -> (Self, Arc<Mutex<Vec<u8>>>) {
        let mut terminal = Box::new(
            Terminal::new(TerminalOptions {
                cols: initial_cols,
                rows: initial_rows,
                max_scrollback,
            })
            .expect("GhosttyBuffer: terminal init failed"),
        );

        // Wire PTY write-back handler (query responses, device attributes, etc.)
        // IMPORTANT: terminal must be heap-allocated (Box) before registering
        // callbacks: on_pty_write stores &self.vtable as a raw C pointer, so the
        // Terminal must not be moved after registration or the pointer dangles.
        //
        // Use try_send (non-blocking) so vt_write() can never stall the main thread
        // even if the slave process is not reading its stdin right now.
        let tx_clone = pty_write_tx.clone();
        terminal
            .on_pty_write(move |data| {
                let _ = tx_clone.try_send(data.to_vec());
            })
            .expect("GhosttyBuffer: on_pty_write failed");

        // Wire size callback for XTWINOPS queries (CSI 14t / 16t / 18t).
        // `icat` and other graphics apps use these to determine pixel dimensions.
        let size_state = Arc::new(Mutex::new((
            initial_rows,
            initial_cols,
            0u32,
            0u32,
        )));
        let size_state_clone = Arc::clone(&size_state);
        terminal
            .on_size(move || {
                size_state_clone.lock().map(|s| *s).unwrap_or((0, 0, 0, 0))
            })
            .expect("GhosttyBuffer: on_size failed");

        // Configure the default color theme to match our palette.
        terminal
            .set_default_fg_color(Some(RgbColor { r: 255, g: 255, b: 255 }))
            .expect("GhosttyBuffer: set fg color failed");
        terminal
            .set_default_bg_color(Some(RgbColor { r: 20, g: 20, b: 20 }))
            .expect("GhosttyBuffer: set bg color failed");
        terminal
            .set_default_color_palette(Some(build_palette()))
            .expect("GhosttyBuffer: set palette failed");

        // Apply the user-configured default cursor style via DECSCUSR so that
        // libghostty's internal state reflects it from the start.
        terminal.vt_write(default_cursor.decscusr_bytes());

        let render_state =
            RenderState::new().expect("GhosttyBuffer: render_state init failed");
        let row_iter = RowIterator::new().expect("GhosttyBuffer: row_iter init failed");
        let cell_iter = CellIterator::new().expect("GhosttyBuffer: cell_iter init failed");

        let incoming_bytes = Arc::new(Mutex::new(Vec::<u8>::new()));
        let incoming_clone = Arc::clone(&incoming_bytes);

        let gb = GhosttyBuffer {
            terminal,
            render_state,
            row_iter,
            cell_iter,
            incoming_bytes,
            dirty: true,
            width: initial_cols,
            height: initial_rows,
            cell_width_px: 0,
            cell_height_px: 0,
            size_state,
            pty_write_tx,
            kitty_graphics: KittyGraphicsState::new(),
        };

        (gb, incoming_clone)
    }

    /// Drain pending bytes from the reader thread and feed them to the
    /// terminal.  Sets `dirty = true` if any bytes were processed.
    ///
    /// At most `MAX_BYTES_PER_FRAME` bytes are consumed per call so that a
    /// fast-producing program cannot starve the main event loop long enough
    /// for the OS to mark the window as "not responding".  Any remainder
    /// stays in `incoming_bytes` and is picked up on the next frame.
    pub fn process_pending_bytes(&mut self) {
        const MAX_BYTES_PER_FRAME: usize = 65_536;

        let bytes = {
            let Ok(mut incoming) = self.incoming_bytes.lock() else {
                return;
            };
            if incoming.is_empty() {
                return;
            }
            if incoming.len() <= MAX_BYTES_PER_FRAME {
                std::mem::take(&mut *incoming)
            } else {
                incoming.drain(..MAX_BYTES_PER_FRAME).collect()
            }
        };

        // Intercept Kitty Graphics Protocol sequences for image rendering.
        let kitty_seqs = crate::kitty_graphics::find_kitty_sequences(&bytes);
        if kitty_seqs.is_empty() {
            self.terminal.vt_write(&bytes);
        } else {
            let mut last = 0usize;
            for (seq_start, seq_end) in kitty_seqs {
                // Feed everything before this sequence to the VT parser first
                // so the cursor advances to the correct position.
                if last < seq_start {
                    self.terminal.vt_write(&bytes[last..seq_start]);
                }
                // Snapshot cursor position at placement point.
                let cx = self.cursor_x() as u16;
                let cy = self.cursor_y() as u16;
                let scrollback_len = self.scrollback_len();
                // Handle the Kitty sequence ourselves (image storage + response).
                if let Some(response) = self.kitty_graphics.process_raw_sequence(
                    &bytes[seq_start..seq_end], cx, cy, scrollback_len,
                ) {
                    // Non-blocking send — never stalls the main thread.
                    let _ = self.pty_write_tx.try_send(response);
                }
                // Also pass to libghostty so it can manage virtual placeholder
                // cells, cursor advancement, and scrollback.
                let cy_before = self.cursor_y();
                self.terminal.vt_write(&bytes[seq_start..seq_end]);
                let cy_after = self.cursor_y();

                // If libghostty didn't advance the cursor (e.g. couldn't decode the file),
                // scroll the terminal by emitting newlines so the image doesn't overlap text.
                // cursor-down (ESC[nB) doesn't scroll; newlines do.
                if cy_after == cy_before {
                    if let Some(placement) = self.kitty_graphics.placements.last() {
                        if placement.abs_row == scrollback_len + cy as usize {
                            let n_rows = if self.cell_height_px > 0 {
                                (placement.pixel_height + self.cell_height_px - 1) / self.cell_height_px
                            } else {
                                0
                            };
                            if n_rows > 0 {
                                self.terminal.vt_write("\n".repeat(n_rows as usize).as_bytes());
                            }
                        }
                    }
                }
                last = seq_end;
            }
            if last < bytes.len() {
                self.terminal.vt_write(&bytes[last..]);
            }
        }

        self.dirty = true;
    }

    // ── dimensions ──────────────────────────────────────────────────────────

    pub fn width(&self) -> usize {
        self.width as usize
    }

    pub fn height(&self) -> usize {
        self.height as usize
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2) as u16;
        let rows = rows.max(2) as u16;
        self.width = cols;
        self.height = rows;
        if let Ok(mut s) = self.size_state.lock() {
            s.0 = rows;
            s.1 = cols;
        }
        let _ = self.terminal.resize(cols, rows, self.cell_width_px, self.cell_height_px);
    }

    /// Update the cell pixel dimensions used for pixel-aware terminal queries
    /// (XTWINOPS CSI 14t / 16t).  Call this whenever the font metrics change.
    pub fn set_cell_pixel_size(&mut self, cell_width_px: u32, cell_height_px: u32) {
        self.cell_width_px = cell_width_px;
        self.cell_height_px = cell_height_px;
        if let Ok(mut s) = self.size_state.lock() {
            s.2 = cell_width_px;
            s.3 = cell_height_px;
        }
        let _ = self.terminal.resize(self.width, self.height, cell_width_px, cell_height_px);
    }

    // ── cursor ───────────────────────────────────────────────────────────────

    /// Cursor column in the current viewport (0-based).
    pub fn cursor_x(&self) -> usize {
        self.terminal.cursor_x().unwrap_or(0) as usize
    }

    /// Cursor row in the current viewport (0-based).
    pub fn cursor_y(&self) -> usize {
        self.terminal.cursor_y().unwrap_or(0) as usize
    }

    /// Whether DEC mode 25 (cursor visible) is active.
    pub fn cursor_visible(&self) -> bool {
        self.terminal
            .mode(Mode::CURSOR_VISIBLE)
            .unwrap_or(true)
    }

    /// Cursor visual style derived from the render state.
    /// Falls back to BlinkingBlock if the render state can't be queried.
    pub fn cursor_style(&mut self) -> CursorStyle {
        let GhosttyBuffer { terminal, render_state, .. } = self;
        if let Ok(snap) = render_state.update(terminal) {
            let blinking = snap.cursor_blinking().unwrap_or(true);
            let style = snap
                .cursor_visual_style()
                .unwrap_or(libghostty_vt::render::CursorVisualStyle::Block);
            use libghostty_vt::render::CursorVisualStyle as CVS;
            return match (style, blinking) {
                (CVS::Bar, true) => CursorStyle::BlinkingBar,
                (CVS::Bar, false) => CursorStyle::SteadyBar,
                (CVS::Block | CVS::BlockHollow, true) => CursorStyle::BlinkingBlock,
                (CVS::Block | CVS::BlockHollow, false) => CursorStyle::SteadyBlock,
                (CVS::Underline, true) => CursorStyle::BlinkingUnderline,
                (CVS::Underline, false) => CursorStyle::SteadyUnderline,
            };
        }
        CursorStyle::BlinkingBlock
    }

    // ── scrollback ───────────────────────────────────────────────────────────

    /// Number of lines currently in the scrollback buffer.
    pub fn scrollback_len(&self) -> usize {
        self.terminal.scrollback_rows().unwrap_or(0)
    }

    /// Number of rows the viewport is scrolled back from the live bottom.
    /// 0 means we are viewing the live screen.
    pub fn scroll_offset(&self) -> usize {
        self.terminal
            .scrollbar()
            .map(|sb| {
                let bottom = (sb.total as usize).saturating_sub(sb.len as usize);
                bottom.saturating_sub(sb.offset as usize)
            })
            .unwrap_or(0)
    }

    /// Returns `true` when the viewport is at the live (bottom) position.
    pub fn is_at_bottom(&self) -> bool {
        self.terminal
            .scrollbar()
            .map(|sb| sb.offset + sb.len >= sb.total)
            .unwrap_or(true)
    }

    /// Scroll the viewport toward older history (up).
    pub fn scroll_view_up(&mut self, n: usize) {
        if n > 0 {
            self.terminal
                .scroll_viewport(ScrollViewport::Delta(-(n as isize)));
            self.dirty = true;
        }
    }

    /// Scroll the viewport toward live output (down).
    pub fn scroll_view_down(&mut self, n: usize) {
        if n > 0 {
            self.terminal
                .scroll_viewport(ScrollViewport::Delta(n as isize));
            self.dirty = true;
        }
    }

    /// Scroll the viewport all the way to the live bottom.
    pub fn reset_view_offset(&mut self) {
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
        self.dirty = true;
    }

    // ── dirty tracking ───────────────────────────────────────────────────────

    /// Returns `true` if the buffer has changed since the last `clear_dirty`.
    /// Also processes any pending bytes from the reader thread.
    pub fn is_dirty(&mut self) -> bool {
        self.process_pending_bytes();
        self.dirty
    }

    /// Returns `true` if there is unprocessed work (either unprocessed bytes or
    /// the dirty flag is set) WITHOUT consuming any bytes.  This is cheap to call
    /// on every event-loop iteration; use `is_dirty()` / `process_pending_bytes()`
    /// only when you are actually going to render.
    pub fn has_pending_bytes(&self) -> bool {
        self.dirty
            || self
                .incoming_bytes
                .try_lock()
                .map_or(true, |b| !b.is_empty())
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    // ── terminal modes ───────────────────────────────────────────────────────

    /// DEC mode 5 – reverse video (DECSCNM).
    pub fn reverse_video_mode(&self) -> bool {
        self.terminal
            .mode(Mode::REVERSE_COLORS)
            .unwrap_or(false)
    }

    /// Application cursor keys (DECCKM) – used by send_key().
    pub fn application_cursor_keys(&self) -> bool {
        self.terminal.mode(Mode::DECCKM).unwrap_or(false)
    }

    /// Bracketed paste mode – used by send_paste().
    pub fn bracketed_paste(&self) -> bool {
        self.terminal
            .mode(Mode::BRACKETED_PASTE)
            .unwrap_or(false)
    }

    /// Whether any mouse tracking mode is active.
    pub fn is_mouse_tracking(&self) -> bool {
        self.terminal.is_mouse_tracking().unwrap_or(false)
    }

    /// Current mouse tracking mode.
    pub fn mouse_tracking_mode(&self) -> MouseTrackingMode {
        if self.terminal.mode(Mode::ANY_MOUSE).unwrap_or(false) {
            MouseTrackingMode::AnyEvent
        } else if self.terminal.mode(Mode::BUTTON_MOUSE).unwrap_or(false) {
            MouseTrackingMode::ButtonEvent
        } else if self.terminal.mode(Mode::NORMAL_MOUSE).unwrap_or(false) {
            MouseTrackingMode::VT200Normal
        } else if self.terminal.mode(Mode::X10_MOUSE).unwrap_or(false) {
            MouseTrackingMode::X10
        } else {
            MouseTrackingMode::Disabled
        }
    }

    /// SGR mouse encoding mode – used by send_mouse_event().
    pub fn mouse_sgr_mode(&self) -> bool {
        self.terminal.mode(Mode::SGR_MOUSE).unwrap_or(false)
    }

    // ── rendering ────────────────────────────────────────────────────────────

    /// Update the render state from the terminal and pass a `RenderContext`
    /// to the closure.  Uses struct-field destructuring to satisfy the borrow
    /// checker while `render_state.update(&terminal)` holds references to both.
    pub fn render_with<R, F>(&mut self, f: F) -> R
    where
        F: FnOnce(RenderContext<'_, '_>) -> R,
    {
        let GhosttyBuffer {
            terminal,
            render_state,
            row_iter,
            cell_iter,
            ..
        } = self;
        let snapshot = render_state
            .update(terminal)
            .expect("render_state update failed");
        f(RenderContext {
            snapshot: &snapshot,
            row_iter,
            cell_iter,
        })
    }

    // ── grid access (for selection / history reads) ──────────────────────────

    /// Read the grapheme cluster at an absolute screen row (0 = oldest
    /// scrollback line) and column.  Returns an empty vec on failure or if
    /// the cell has no text.
    pub fn graphemes_at(&self, col: usize, abs_row: usize) -> Vec<char> {
        use libghostty_vt::terminal::Point;
        let point = Point::Screen(PointCoordinate {
            x: col as u16,
            y: abs_row as u32,
        });
        let Ok(grid_ref) = self.terminal.grid_ref(point) else {
            return Vec::new();
        };
        let mut buf = ['\0'; 8];
        match grid_ref.graphemes(&mut buf) {
            Ok(n) => buf[..n].iter().filter(|&&c| c != '\0').copied().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Width tag of the cell at the given absolute position.
    /// Returns `1` for normal cells, `2` for wide, `0` for spacer tails.
    pub fn cell_width_at(&self, col: usize, abs_row: usize) -> u8 {
        use libghostty_vt::screen::CellWide;
        use libghostty_vt::terminal::Point;
        let point = Point::Screen(PointCoordinate {
            x: col as u16,
            y: abs_row as u32,
        });
        let Ok(grid_ref) = self.terminal.grid_ref(point) else {
            return 1;
        };
        let Ok(cell) = grid_ref.cell() else {
            return 1;
        };
        match cell.wide() {
            Ok(CellWide::Wide) => 2,
            Ok(CellWide::SpacerTail | CellWide::SpacerHead) => 0,
            _ => 1,
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> RgbColor {
    RgbColor { r, g, b }
}

/// Build a 256-color palette matching the legacy hardcoded colors from ansi.rs.
fn build_palette() -> [RgbColor; 256] {
    let mut p = [RgbColor { r: 0, g: 0, b: 0 }; 256];

    // 0-7: standard colors
    p[0] = rgb(20, 20, 20);      // Black (DEFAULT_BG_COLOR)
    p[1] = rgb(255, 80, 80);     // Red
    p[2] = rgb(80, 255, 80);     // Green
    p[3] = rgb(255, 255, 80);    // Yellow
    p[4] = rgb(80, 80, 255);     // Blue
    p[5] = rgb(255, 80, 255);    // Magenta
    p[6] = rgb(80, 255, 255);    // Cyan
    p[7] = rgb(255, 255, 255);   // White (DEFAULT_FG_COLOR)

    // 8-15: bright colors
    p[8]  = rgb(128, 128, 128);  // Bright Black (Gray)
    p[9]  = rgb(255, 128, 128);  // Bright Red
    p[10] = rgb(128, 255, 128);  // Bright Green
    p[11] = rgb(255, 255, 128);  // Bright Yellow
    p[12] = rgb(128, 128, 255);  // Bright Blue
    p[13] = rgb(255, 128, 255);  // Bright Magenta
    p[14] = rgb(128, 255, 255);  // Bright Cyan
    p[15] = rgb(255, 255, 255);  // Bright White

    // 16-231: 6x6x6 color cube
    for i in 16u8..=231 {
        let idx = i - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        p[i as usize] = rgb(
            if r == 0 { 0 } else { 55 + r * 40 },
            if g == 0 { 0 } else { 55 + g * 40 },
            if b == 0 { 0 } else { 55 + b * 40 },
        );
    }

    // 232-255: grayscale ramp
    for i in 232u8..=255 {
        let gray = 8 + (i - 232) * 10;
        p[i as usize] = rgb(gray, gray, gray);
    }

    p
}
