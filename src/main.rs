mod ansi;
mod font_discovery;
mod input;
mod pane_layout;
mod screen_buffer;
mod sdl_renderer;
mod settings;
mod state;
mod tab_gui;
mod terminal;
mod terminal_config;
mod ui;

use ui::render;

use crate::tab_gui::TabBarGui;
use crate::terminal::Terminal;
use crate::terminal_config::TerminalLibrary;

use arboard::Clipboard;
use sdl3::event::Event;

use sdl3::video::Window;
#[cfg(not(target_os = "windows"))]
use signal_hook::consts::signal::*;
#[cfg(not(target_os = "windows"))]
use signal_hook::iterator::Signals;
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use std::sync::mpsc::channel;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::System;

#[cfg(feature = "test-server")]
mod test_server;
#[cfg(feature = "test-server")]
use crate::test_server::TestServer;

// Build-time version information
const BUILD_DATE: &str = env!("BUILD_DATE");
const GIT_HASH: &str = env!("GIT_HASH");
const DEFAULT_SCROLLBACK_LINES: usize = 10000;

/// Set the window icon from embedded PNG data
fn set_window_icon(window: &mut Window) {
    const ICON_DATA: &[u8] = include_bytes!("../icon.png");

    match image::load_from_memory(ICON_DATA) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let pixels = rgba.into_raw();

            match create_sdl_surface_from_rgba(width, height, pixels) {
                Ok(surface) => {
                    window.set_icon(surface);
                }
                Err(e) => {
                    eprintln!("[MAIN] Failed to create icon surface: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("[MAIN] Failed to load window icon: {}", e);
        }
    }
}

/// Create an SDL surface from RGBA pixel data
fn create_sdl_surface_from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<sdl3::surface::Surface<'static>, String> {
    let mut surface =
        sdl3::surface::Surface::new(width, height, sdl3::pixels::PixelFormat::RGBA32).map_err(|e| format!("Failed to create SDL surface: {}", e))?;

    // Copy pixel data
    surface.with_lock_mut(|buffer: &mut [u8]| {
        buffer.copy_from_slice(&pixels);
    });

    Ok(surface)
}

/// Load context menu images from statically embedded data
fn load_context_menu_images() -> Result<crate::pane_layout::ContextMenuImages, String> {
    Ok(crate::pane_layout::ContextMenuImages::load())
}

/// Resize all terminals in the active tab to match their pane dimensions
fn resize_terminals_to_panes(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    window_width: u32,
    window_height: u32,
) {
    // Non-blocking lock - skip resize if lock is busy (will retry on next event)
    if let Ok(gui) = tab_bar_gui.try_lock() {
        if let Some(pane_layout) = gui.tab_states.get(gui.active_tab) {
            let pane_area_y = tab_bar_height as i32;
            let pane_area_height = window_height - tab_bar_height;
            let pane_rects = pane_layout.pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

            for (_pane_id, rect, terminal, _is_active) in pane_rects {
                let (cols, rows) = crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), char_width, char_height);

                if let Ok(mut t) = terminal.lock() {
                    // Only resize if dimensions have changed
                    if t.width != cols || t.height != rows {
                        t.set_size(cols, rows, false);
                    }
                }
            }
        }
    } else {
        eprintln!("[PERF] Skipped terminal resize - lock busy");
    }
}

/// Resize all terminals after a pane split, clearing screen buffers to prevent stale content
fn resize_terminals_after_split(
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    window_width: u32,
    window_height: u32,
) {
    // Use blocking lock - resize after split MUST happen
    let gui = match tab_bar_gui.lock() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[RESIZE] CRITICAL: Failed to acquire GUI lock after split: {}", e);
            return;
        }
    };

    if let Some(pane_layout) = gui.tab_states.get(gui.active_tab) {
        let pane_area_y = tab_bar_height as i32;
        let pane_area_height = window_height - tab_bar_height;
        let pane_rects = pane_layout.pane_layout.get_pane_rects(0, pane_area_y, window_width, pane_area_height);

        eprintln!("[RESIZE] Resizing {} terminals after split", pane_rects.len());

        for (pane_id, rect, terminal, _is_active) in pane_rects {
            let (cols, rows) = crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), char_width, char_height);

            match terminal.lock() {
                Ok(mut t) => {
                    // Always resize with clear_screen=true to prevent stale content after split
                    if t.width != cols || t.height != rows {
                        eprintln!("[RESIZE] Pane {:?}: {}x{} -> {}x{}", pane_id, t.width, t.height, cols, rows);
                        t.set_size(cols, rows, true);
                    } else {
                        eprintln!("[RESIZE] Pane {:?}: already {}x{}", pane_id, cols, rows);
                    }
                }
                Err(e) => {
                    eprintln!("[RESIZE] CRITICAL: Failed to lock terminal for pane {:?}: {}", pane_id, e);
                }
            }
        }
    } else {
        eprintln!("[RESIZE] No active pane layout found");
    }
}

fn main() -> Result<(), String> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut test_port: Option<u16> = None;

    // Handle --help and --version before initializing SDL
    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            println!("Nisdos Terminal v{} ({}, built {})", env!("CARGO_PKG_VERSION"), GIT_HASH, BUILD_DATE);
            println!();
            println!("USAGE:");
            println!("    nist [OPTIONS]");
            println!();
            println!("OPTIONS:");
            println!("    -h, --help          Print help information");
            println!("    -v, --version       Print version information");
            println!("    --test-port <PORT>  Enable test server on specified port");
            std::process::exit(0);
        } else if arg == "--version" || arg == "-v" {
            println!("Nisdos Terminal {} ({}, built {})", env!("CARGO_PKG_VERSION"), GIT_HASH, BUILD_DATE);
            std::process::exit(0);
        }
    }

    for (i, arg) in args.iter().enumerate() {
        if arg == "--test-port" && i + 1 < args.len() {
            if let Ok(port) = args[i + 1].parse::<u16>() {
                test_port = Some(port);
                eprintln!("[MAIN] Test server will be enabled on port {}", port);
            }
        }
    }

    eprintln!("[MAIN] Nisdos Terminal starting (built: {})", BUILD_DATE);

    // Print feature flags
    #[cfg(feature = "test-server")]
    eprintln!("[MAIN] Feature: test-server enabled");

    // Set up signal handlers to save state on OS termination (reboot, kill, etc.)
    // Create a Signals iterator for SIGTERM, SIGINT, and SIGHUP
    #[cfg(not(target_os = "windows"))]
    let signal_rx = {
        let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).map_err(|e| format!("Failed to register signal handlers: {}", e))?;
        eprintln!("[MAIN] Registered signal handlers for SIGTERM, SIGINT, SIGHUP");

        // Move signals iterator to a thread that can interrupt the main loop
        let (signal_tx, signal_rx) = channel::<i32>();
        std::thread::spawn(move || {
            for sig in signals.forever() {
                eprintln!("[SIGNAL] Received signal: {}", sig);
                let _ = signal_tx.send(sig);
            }
        });
        signal_rx
    };

    let (window_width, window_height) = (2376_u32, 1593_u32);

    let sdl_context = sdl3::init().unwrap();

    // Set window class name for proper desktop integration
    sdl3::hint::set("SDL_VIDEO_X11_WMCLASS", "nist");
    sdl3::hint::set("SDL_VIDEO_WAYLAND_WMCLASS", "nist");
    sdl3::hint::set("SDL_VIDEO_WAYLAND_APP_ID", "nist");
    sdl3::hint::set("SDL_APP_ID", "nist");
    sdl3::hint::set("SDL_APP_NAME", "Nisdos Terminal");

    let video_subsystem = sdl_context.video().unwrap();
    let ttf_context = sdl3::ttf::init().map_err(|e| e.to_string())?;

    // Create window with high DPI awareness (borderless for headless mode)
    let mut window = video_subsystem
        .window("Nisdos Terminal", window_width, window_height)
        .position_centered()
        .resizable()
        .maximized()
        .borderless()
        .high_pixel_density()
        .build()
        .map_err(|e| e.to_string())?;

    // Set window icon
    set_window_icon(&mut window);

    // Create canvas for rendering
    let mut canvas = window.into_canvas();

    // Get initial scale factor before setting render scale
    let initial_scale = canvas.window().display_scale();
    eprintln!("[MAIN] Initial display scale before render setup: {:.2}", initial_scale);

    // Enable VSync to limit rendering to display refresh rate (60 Hz)
    // SDL3 requires explicit VSync enable via SDL_SetRenderVSync (not part of canvas builder)
    let vsync_success = unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) };

    if vsync_success {
        // Verify VSync was actually set by reading it back
        let mut vsync_value: std::os::raw::c_int = 0;
        let get_success = unsafe { sdl3::sys::render::SDL_GetRenderVSync(canvas.raw(), &mut vsync_value) };
        if get_success && vsync_value == 1 {
            eprintln!("[MAIN] Canvas created with VSync enabled (verified: vsync={})", vsync_value);
        } else if get_success {
            eprintln!("[MAIN] WARNING: VSync set to unexpected value: {}", vsync_value);
        } else {
            eprintln!("[MAIN] WARNING: Could not verify VSync setting");
        }
    } else {
        eprintln!("[MAIN] WARNING: Failed to enable VSync! CPU usage may be high.");
        eprintln!("[MAIN] Canvas created without VSync");
    }

    // Get window sizes
    let (window_width_logical, window_height_logical) = canvas.window().size();
    let (drawable_width, drawable_height) = canvas.window().size_in_pixels();

    eprintln!("[MAIN] Raw SDL3 window data:");
    eprintln!("[MAIN]   - window.size() = {}x{}", window_width_logical, window_height_logical);
    eprintln!("[MAIN]   - window.size_in_pixels() = {}x{}", drawable_width, drawable_height);
    eprintln!("[MAIN]   - window.pixel_density() = {:.2}", canvas.window().pixel_density());

    // Try to detect real DPI scaling by querying display information
    // First, try the simple ratio between drawable and logical sizes
    let mut scale_factor = if window_width_logical > 0 {
        drawable_width as f32 / window_width_logical as f32
    } else {
        1.0
    };

    // SDL3 provides SDL_GetWindowDisplayScale which combines pixel density and content scale
    // Use this as the authoritative source for DPI scaling
    let window_display_scale = canvas.window().display_scale();

    // If window_display_scale is valid and different from our calculated value, use it
    if window_display_scale > 0.0 {
        eprintln!("[MAIN] SDL3 window display scale: {:.2}", window_display_scale);
        if (window_display_scale - scale_factor).abs() > 0.01 {
            eprintln!("[MAIN] Using SDL3 display scale instead of calculated ratio");
            scale_factor = window_display_scale;
        }
    }

    // If scale_factor is still 1.0, try to detect scaling from pixel density
    if scale_factor == 1.0 {
        let pixel_density = canvas.window().pixel_density();
        if pixel_density > 1.0 {
            scale_factor = pixel_density;
            eprintln!("[MAIN] Detected DPI scaling from pixel density: {:.2}", scale_factor);
        }
    }

    eprintln!(
        "[MAIN] Calculated dimensions: {}x{} logical, {}x{} drawable, final scale: {:.2}",
        window_width_logical, window_height_logical, drawable_width, drawable_height, scale_factor
    );

    // Calculate what the real physical size should be if OS scaling is 2x
    let expected_physical_w = (window_width_logical as f32 * scale_factor) as u32;
    let expected_physical_h = (window_height_logical as f32 * scale_factor) as u32;
    eprintln!(
        "[MAIN] Expected physical size with {:.2}x scaling: {}x{} pixels",
        scale_factor, expected_physical_w, expected_physical_h
    );

    // Don't use SDL_SetRenderScale - it causes blurry scaling
    // Instead, we'll render at physical pixel size for crisp output
    eprintln!("[MAIN] Rendering at physical pixel size for crisp output");

    // Detect if mouse coordinates need scaling: true when window size != drawable size
    // This handles different SDL2 behaviors across platforms without hardcoding OS checks
    let mouse_coords_need_scaling = scale_factor > 1.0 && {
        let (w_width, _) = canvas.window().size();
        let (d_width, _) = canvas.window().size_in_pixels();
        w_width != d_width
    };
    eprintln!(
        "[MAIN] Mouse coordinate scaling: {} (window != drawable: {})",
        if mouse_coords_need_scaling { "ENABLED" } else { "DISABLED" },
        canvas.window().size() != canvas.window().size_in_pixels()
    );

    // Load settings to get font configuration
    let mut settings = settings::load_settings().unwrap_or_else(|e| {
        eprintln!("[MAIN] Failed to load settings, using defaults: {}", e);
        settings::Settings::default()
    });

    // Load monospace font for terminal
    // Use fontSize from settings (defaults to 12.0 if not set)
    // Scale font size for physical pixel rendering
    let font_size = settings.terminal.font_size * scale_factor;

    // Determine font path: use fontFamily from settings, or "auto" to use discovery
    let font_path = if settings.terminal.font_family == "auto" {
        font_discovery::find_best_monospace_font().ok_or_else(|| {
            let error_msg = "\
[ERROR] No suitable monospace font found on your system!

Please install one of these recommended fonts:
  - On macOS: Menlo, Monaco, or SF Mono (usually pre-installed)
  - On Linux: Install fonts-hack, fonts-jetbrains-mono, or fonts-dejavu
    Example: sudo apt-get install fonts-hack fonts-dejavu
  - On Windows: Consolas, Cascadia Code (usually pre-installed), or download JetBrains Mono

Searched directories:
  - C:\\Windows\\Fonts (Windows)
  - %LOCALAPPDATA%\\Microsoft\\Windows\\Fonts (Windows)
  - %USERPROFILE%\\AppData\\Local\\Microsoft\\Windows\\Fonts (Windows)
  - /System/Library/Fonts (macOS)
  - /Library/Fonts (macOS)
  - ~/Library/Fonts (macOS)
  - /usr/share/fonts (Linux)
  - /usr/local/share/fonts (Linux)
  - ~/.local/share/fonts (Linux)
  - ~/.fonts (Linux)
";
            eprintln!("{}", error_msg);
            error_msg.to_string()
        })?
    } else {
        // Use the font family path specified in settings
        let font_path = settings.terminal.font_family.clone();
        eprintln!("[MAIN] Using font from settings: {}", font_path);

        // Validate that the font file exists
        if !std::path::Path::new(&font_path).exists() {
            let error_msg = format!("[ERROR] Font file not found: {}\n\nFalling back to automatic font discovery.", font_path);
            eprintln!("{}", error_msg);

            // Fallback to automatic discovery
            font_discovery::find_best_monospace_font().ok_or_else(|| {
                let error_msg = "\
[ERROR] No suitable monospace font found on your system!

Please install one of these recommended fonts:
  - On macOS: Menlo, Monaco, or SF Mono (usually pre-installed)
  - On Linux: Install fonts-hack, fonts-jetbrains-mono, or fonts-dejavu
    Example: sudo apt-get install fonts-hack fonts-dejavu
  - On Windows: Consolas, Cascadia Code (usually pre-installed), or download JetBrains Mono

Searched directories:
  - C:\\Windows\\Fonts (Windows)
  - %LOCALAPPDATA%\\Microsoft\\Windows\\Fonts (Windows)
  - %USERPROFILE%\\AppData\\Local\\Microsoft\\Windows\\Fonts (Windows)
  - /System/Library/Fonts (macOS)
  - /Library/Fonts (macOS)
  - ~/Library/Fonts (macOS)
  - /usr/share/fonts (Linux)
  - /usr/local/share/fonts (Linux)
  - ~/.local/share/fonts (Linux)
  - ~/.fonts (Linux)
";
                eprintln!("{}", error_msg);
                error_msg.to_string()
            })?
        } else {
            font_path
        }
    };

    let mut font = ttf_context.load_font(&font_path, font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load font from {}: {}", font_path, e);
        format!("Font loading failed from {}: {}", font_path, e)
    })?;

    eprintln!(
        "[MAIN] Loaded monospace font: {} at size {} (from settings: fontSize={}, fontFamily={})",
        font_path,
        font_size,
        settings.terminal.font_size,
        if settings.terminal.font_family == "auto" {
            "auto (discovered)"
        } else {
            &settings.terminal.font_family
        }
    );

    // Load proportional UI font with emoji support for tabs, menus, and window controls
    let ui_font_path = font_discovery::find_best_ui_font().ok_or_else(|| {
        let error_msg = "\
[ERROR] No suitable UI font found on your system!

Please install one of these recommended fonts:
  - On macOS: SF Pro, Helvetica (usually pre-installed)
  - On Linux: Install fonts-noto, fonts-ubuntu, or fonts-dejavu
    Example: sudo apt-get install fonts-noto fonts-dejavu
  - On Windows: Segoe UI (usually pre-installed) or download Noto Sans

Searched directories:
  - C:\\Windows\\Fonts (Windows)
  - %LOCALAPPDATA%\\Microsoft\\Windows\\Fonts (Windows)
  - %USERPROFILE%\\AppData\\Local\\Microsoft\\Windows\\Fonts (Windows)
  - /System/Library/Fonts (macOS)
  - /Library/Fonts (macOS)
  - ~/Library/Fonts (macOS)
  - /usr/share/fonts (Linux)
  - /usr/local/share/fonts (Linux)
  - ~/.local/share/fonts (Linux)
  - ~/.fonts (Linux)
";
        eprintln!("{}", error_msg);
        error_msg.to_string()
    })?;

    // Load smaller font for CPU indicator in tab bar (use UI font for emoji support)
    let cpu_font_size = 13.0 * scale_factor;
    let cpu_font = ttf_context.load_font(&ui_font_path, cpu_font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load CPU font from {}: {}", ui_font_path, e);
        format!("CPU font loading failed from {}: {}", ui_font_path, e)
    })?;

    // Load smaller font for tab names (use UI font for emoji support)
    let tab_font_size = 18.0 * scale_factor;
    let tab_font = ttf_context.load_font(&ui_font_path, tab_font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load tab font from {}: {}", ui_font_path, e);
        format!("Tab font loading failed from {}: {}", ui_font_path, e)
    })?;

    // Load smaller font for context menu (use UI font for emoji support)
    let context_menu_font_size = 12.0 * scale_factor;
    let context_menu_font = ttf_context.load_font(&ui_font_path, context_menu_font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load context menu font from {}: {}", ui_font_path, e);
        format!("Context menu font loading failed from {}: {}", ui_font_path, e)
    })?;

    // Load larger font for buttons (window controls and add button)
    let button_font_size = 27.0 * scale_factor;
    let button_font = ttf_context.load_font(&ui_font_path, button_font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load button font from {}: {}", ui_font_path, e);
        format!("Button font loading failed from {}: {}", ui_font_path, e)
    })?;

    eprintln!("[MAIN] Loaded UI font: {} for tabs, menus, and controls", ui_font_path);

    // Load emoji fallback font for emoji rendering in terminal
    let emoji_font_path = font_discovery::find_emoji_font().unwrap_or_else(|| {
        eprintln!("[MAIN] WARNING: No emoji font found, emoji rendering may not work properly");
        ui_font_path.clone()
    });
    let emoji_font = ttf_context.load_font(&emoji_font_path, font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load emoji font from {}: {}", emoji_font_path, e);
        format!("Emoji font loading failed from {}: {}", emoji_font_path, e)
    })?;
    eprintln!("[MAIN] Loaded emoji font: {} for emoji rendering", emoji_font_path);

    // Load Unicode fallback font for symbols not in monospace font (use UI font which has broad Unicode coverage)
    let unicode_fallback_font = ttf_context.load_font(&ui_font_path, font_size).map_err(|e| {
        eprintln!("[MAIN] Failed to load Unicode fallback font from {}: {}", ui_font_path, e);
        format!("Unicode fallback font loading failed from {}: {}", ui_font_path, e)
    })?;
    eprintln!("[MAIN] Loaded Unicode fallback font: {} for symbol rendering", ui_font_path);

    // Measure character dimensions
    let test_char = 'M';
    let (char_width_i32, char_height_i32) = font.size_of_char(test_char).map_err(|e| e.to_string())?;
    // Font is already scaled by scale_factor, no extra scaling needed
    let mut char_width = char_width_i32 as f32;
    let mut char_height = char_height_i32 as f32;

    eprintln!("[MAIN] Character dimensions: {:.2}x{:.2} pixels", char_width, char_height);

    let texture_creator = canvas.texture_creator();

    let mut event_pump = sdl_context.event_pump().map_err(|e| e.to_string())?;

    // Enable text input for terminal typing
    canvas.window().subsystem().text_input().start(canvas.window());
    eprintln!("[MAIN] Text input enabled");

    // Channel for receiving clipboard objects from background threads
    // This avoids blocking the main thread with clipboard operations
    #[cfg(target_os = "linux")]
    let (clipboard_tx, clipboard_rx): (Sender<Clipboard>, Receiver<Clipboard>) = channel();

    // CPU monitoring state
    let mut sys = System::new_all();
    let mut cpu_usage = 0.0_f32;
    let mut last_cpu_update = Instant::now();
    let cpu_update_interval = std::time::Duration::from_secs(1);

    // Cursor blinking state
    let mut cursor_visible = true;
    let mut last_cursor_blink = Instant::now();
    let cursor_blink_interval = std::time::Duration::from_millis(1000);

    // Cursor blink debounce: keep cursor visible after keyboard input
    let mut last_keyboard_input = Instant::now();
    let cursor_debounce_duration = std::time::Duration::from_millis(500);

    // Get terminal library with hardcoded knowledge
    let term_library = TerminalLibrary::new();
    let shell_config = term_library.get_default_shell().clone();

    // Tab bar state - scale tab bar height for high-DPI displays
    let tab_bar_height = (36.0 * scale_factor) as u32;
    let mut tab_bar = sdl_renderer::TabBar::new(tab_bar_height);

    // Pending operations
    let mut pending_pane_split: Option<crate::pane_layout::SplitDirection> = None;
    let mut pending_new_tab = false;

    // Calculate terminal dimensions using drawable size (actual pixels with DPI scaling)
    let terminal_height = ((drawable_height - tab_bar_height) as f32 / char_height).floor() as u32;
    let terminal_width = (drawable_width as f32 / char_width).floor() as u32;

    // Initialize tab bar GUI
    let tab_bar_gui = {
        let shell_config_clone = shell_config.clone();
        let terminal_factory = |start_dir: Option<std::path::PathBuf>| {
            Arc::new(Mutex::new(Terminal::new_with_scrollback(
                terminal_width,
                terminal_height,
                shell_config_clone.clone(),
                DEFAULT_SCROLLBACK_LINES,
                start_dir,
            )))
        };

        match state::load_state(terminal_factory) {
            Ok((tab_bar_loaded, _active_tab)) => {
                eprintln!("[MAIN] Successfully loaded state");
                Arc::new(Mutex::new(tab_bar_loaded))
            }
            Err(e) => {
                eprintln!("[MAIN] Failed to load state: {}, creating default tab", e);
                let mut tab_bar_new = TabBarGui::new();
                let first_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                    terminal_width,
                    terminal_height,
                    shell_config.clone(),
                    DEFAULT_SCROLLBACK_LINES,
                    std::env::current_dir().ok(),
                )));
                tab_bar_new.add_tab(first_terminal, "Tab 1".to_string());
                Arc::new(Mutex::new(tab_bar_new))
            }
        }
    };

    // Settings are now loaded earlier (before font loading) and stored in the `settings` variable
    // This comment is kept for reference - settings initialization happens around line 313

    // Set context menu images on all pane layouts
    let context_menu_images = load_context_menu_images().ok();
    if let Some(images) = context_menu_images {
        if let Ok(mut gui) = tab_bar_gui.try_lock() {
            gui.set_context_menu_images(images);
        }
    }

    // Initialize test server if requested
    #[cfg(feature = "test-server")]
    let test_server = if let Some(port) = test_port {
        let terminals = match tab_bar_gui.try_lock() {
            Ok(gui) => gui.get_all_terminals(),
            Err(_) => {
                eprintln!("[MAIN] Failed to get terminals for test server");
                Vec::new()
            }
        };
        match TestServer::new(
            port,
            terminals,
            Arc::clone(&tab_bar_gui),
            char_width,
            char_height,
            tab_bar_height,
            drawable_width,
            drawable_height,
        ) {
            Ok(server) => {
                eprintln!("[MAIN] Test server enabled on port {}", port);
                Some(server)
            }
            Err(e) => {
                eprintln!("[MAIN] Failed to start test server: {}", e);
                None
            }
        }
    } else {
        None
    };

    let ctrl_keys = input::keyboard::create_ctrl_key_map();

    // Mouse state tracker
    let mut mouse_state = input::mouse::MouseState::new();

    // Glyph cache to avoid re-rendering characters every frame
    // Key: (character, fg_color_rgb, bg_color_rgb), Value: texture
    let mut glyph_cache: HashMap<(String, (u8, u8, u8)), sdl3::render::Texture> = HashMap::new();
    let mut last_cache_clear = Instant::now();

    let mut needs_render = true;
    let mut skip_render_count = 0;

    'running: loop {
        // Check for termination signals (SIGTERM, SIGINT, SIGHUP from OS)
        #[cfg(not(target_os = "windows"))]
        if let Ok(sig) = signal_rx.try_recv() {
            eprintln!("[MAIN] Termination signal {} received, saving state and exiting...", sig);
            if let Ok(gui) = tab_bar_gui.try_lock() {
                if let Err(e) = state::save_state(&gui) {
                    eprintln!("[MAIN] Failed to save state: {}", e);
                }
            }
            break 'running;
        }

        // Receive clipboard objects from background threads and store in PaneLayout
        #[cfg(target_os = "linux")]
        {
            if let Ok(clipboard) = clipboard_rx.try_recv() {
                if let Ok(mut gui) = tab_bar_gui.try_lock() {
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        pane_layout.primary_clipboard = Some(clipboard);
                        eprintln!("[PRIMARY] Clipboard object stored in PaneLayout");
                    }
                }
            }
        }

        // Check for dirty terminals that need rendering
        let has_dirty_content = {
            match tab_bar_gui.try_lock() {
                Ok(gui) => {
                    let terminals = gui.get_active_tab_terminals();
                    let dirty = terminals.iter().any(|term| {
                        if let Ok(t) = term.try_lock() {
                            if let Ok(sb) = t.screen_buffer.try_lock() {
                                sb.is_dirty()
                            } else {
                                true // Assume dirty if can't check
                            }
                        } else {
                            true // Assume dirty if can't check
                        }
                    });

                    dirty
                }
                Err(_) => {
                    true // Assume dirty if can't acquire lock
                }
            }
        };

        if has_dirty_content {
            needs_render = true;
        }

        // Check for completed animations and clean them up
        {
            if let Ok(mut gui) = tab_bar_gui.try_lock() {
                if let Some(pane_layout) = gui.get_active_pane_layout() {
                    if let Some(ref animation) = pane_layout.copy_animation {
                        if animation.is_complete() {
                            pane_layout.copy_animation = None;
                            needs_render = true;
                        } else {
                            // Animation is still running, keep rendering
                            needs_render = true;
                        }
                    }
                }
            }
        }

        // Update CPU usage periodically
        if last_cpu_update.elapsed() >= cpu_update_interval {
            sys.refresh_cpu();
            cpu_usage = sys.global_cpu_info().cpu_usage();
            last_cpu_update = Instant::now();
        }

        // Collect all events first
        let mut events = Vec::new();
        // Use 16ms timeout for ~60 FPS responsiveness while reducing CPU wake-ups
        // PTY reader threads mark terminals as dirty, so we don't need aggressive 1ms polling
        let first_event = event_pump.wait_event_timeout(16);
        if let Some(event) = first_event {
            events.push(event);
        }

        for event in event_pump.poll_iter() {
            events.push(event);
        }

        // Update cursor blink state
        // If we're within the debounce period after keyboard input, keep cursor visible
        let in_debounce_period = last_keyboard_input.elapsed() < cursor_debounce_duration;
        if in_debounce_period {
            if !cursor_visible {
                cursor_visible = true;
                needs_render = true;
            }
            last_cursor_blink = Instant::now(); // Reset blink timer
        } else {
            let cursor_needs_update = last_cursor_blink.elapsed() >= cursor_blink_interval;
            if cursor_needs_update {
                cursor_visible = !cursor_visible;
                last_cursor_blink = Instant::now();
                needs_render = true;
            }
        }

        // Only render if needed
        if !events.is_empty() || needs_render {
            // Print accumulated skip count before rendering
            if skip_render_count > 0 {
                skip_render_count = 0;
            }
            needs_render = false;

            // Process SDL events
            for event in &events {
                // Reset cursor debounce timer on keyboard input
                match event {
                    Event::KeyDown { .. } | Event::TextInput { .. } => {
                        last_keyboard_input = Instant::now();
                    }
                    _ => {}
                }

                let result = input::events::handle_event(
                    event,
                    &mut tab_bar,
                    &tab_bar_gui,
                    &mut mouse_state,
                    &ctrl_keys,
                    scale_factor,
                    mouse_coords_need_scaling,
                    char_width,
                    char_height,
                    tab_bar_height,
                    canvas.window(),
                    &event_pump,
                    &settings,
                    #[cfg(target_os = "linux")]
                    &clipboard_tx,
                );

                // Handle actions requested by event handler
                match result.action {
                    input::events::EventAction::RequestQuitConfirmation => {
                        // Show confirmation dialog
                        if ui::dialogs::confirm_quit(&mut canvas, &mut event_pump, &tab_font, scale_factor) {
                            // User confirmed quit
                            if let Ok(gui) = tab_bar_gui.try_lock() {
                                if let Err(e) = state::save_state(&gui) {
                                    eprintln!("[MAIN] Failed to save state: {}", e);
                                }
                            }
                            break 'running;
                        }
                        // User cancelled, continue running
                        needs_render = true;
                    }
                    input::events::EventAction::Quit => {
                        if let Ok(gui) = tab_bar_gui.try_lock() {
                            if let Err(e) = state::save_state(&gui) {
                                eprintln!("[MAIN] Failed to save state: {}", e);
                            }
                        }
                        break 'running;
                    }
                    input::events::EventAction::CloseTab(close_idx) => {
                        if let Ok(mut gui) = tab_bar_gui.try_lock() {
                            // Check if this is the last tab with one pane
                            let is_last_tab_with_one_pane = gui.tab_states.len() == 1
                                && gui
                                    .tab_states
                                    .get(close_idx)
                                    .map(|tab| tab.pane_layout.root.count_leaf_panes() == 1)
                                    .unwrap_or(false);

                            if is_last_tab_with_one_pane {
                                // Ask for confirmation before closing
                                drop(gui);
                                if !ui::dialogs::confirm_quit(&mut canvas, &mut event_pump, &tab_font, scale_factor) {
                                    // User cancelled, don't close
                                    needs_render = true;
                                    continue;
                                }
                                // User confirmed, quit
                                if let Ok(gui) = tab_bar_gui.try_lock() {
                                    if let Err(e) = state::save_state(&gui) {
                                        eprintln!("[MAIN] Failed to save state: {}", e);
                                    }
                                }
                                break 'running;
                            }

                            if gui.remove_tab(close_idx) {
                                if let Err(e) = state::save_state(&gui) {
                                    eprintln!("[MAIN] Failed to save state: {}", e);
                                }
                                break 'running; // Last tab closed
                            }
                            #[cfg(feature = "test-server")]
                            if let Some(ref server) = test_server {
                                server.update_tabs(gui.get_all_terminals());
                            }
                        }
                    }
                    input::events::EventAction::NewTab => {
                        pending_new_tab = true;
                    }
                    input::events::EventAction::SplitPane(direction) => {
                        pending_pane_split = Some(direction);
                    }

                    input::events::EventAction::SwitchTab(tab_idx) => {
                        if let Ok(mut gui) = tab_bar_gui.try_lock() {
                            gui.set_active_tab(tab_idx);
                        }
                        // Resize terminals in the newly active tab to match their pane dimensions
                        // This ensures terminals that were inactive during window resizing get properly sized
                        let (window_width, window_height) = canvas.window().size_in_pixels();
                        resize_terminals_to_panes(&tab_bar_gui, char_width, char_height, tab_bar_height, window_width, window_height);
                    }

                    input::events::EventAction::MinimizeWindow => {
                        canvas.window_mut().minimize();
                    }
                    input::events::EventAction::Resize => {
                        let (new_width, new_height) = canvas.window().size_in_pixels();
                        eprintln!("[MAIN] Window resized to {}x{}", new_width, new_height);
                        // Resize all terminals to match their pane dimensions
                        resize_terminals_to_panes(&tab_bar_gui, char_width, char_height, tab_bar_height, new_width, new_height);
                    }
                    input::events::EventAction::StartTextInput => {
                        canvas.window().subsystem().text_input().start(canvas.window());
                    }
                    input::events::EventAction::StopTextInput => {
                        // BUG FIX: When tab editing finishes (user presses Enter/Escape after renaming a tab),
                        // we stop text input to exit the tab editing mode. However, the terminal ALWAYS needs
                        // text input enabled to receive typed characters (letters, numbers, etc.).
                        // Special keys (Enter, Backspace, arrows) work via KeyDown events, but regular text
                        // requires text input to be enabled.
                        // Solution: Stop text input briefly, then immediately restart it for the terminal.
                        canvas.window().subsystem().text_input().stop(canvas.window());
                        canvas.window().subsystem().text_input().start(canvas.window());
                    }
                    input::events::EventAction::OpenSettings => {
                        match settings::get_settings_path() {
                            Ok(path) => {
                                #[cfg(target_os = "linux")]
                                let result = {
                                    // Open the file and get the child process
                                    let gio_result = std::process::Command::new("gio").args(["open", path.to_str().unwrap_or("")]).spawn();

                                    let child_result = match gio_result {
                                        Ok(child) => Ok(child),
                                        Err(_) => std::process::Command::new("xdg-open").arg(&path).spawn(),
                                    };

                                    // Spawn a thread to try activating the window after a delay
                                    if child_result.is_ok() {
                                        let path_clone = path.clone();
                                        std::thread::spawn(move || {
                                            let filename = path_clone.file_name().and_then(|s| s.to_str()).unwrap_or("settings.json");

                                            // Try multiple times with delays to catch the window as it appears
                                            for _attempt in 0..10 {
                                                std::thread::sleep(std::time::Duration::from_millis(200));

                                                // Try wmctrl first (most reliable)
                                                if std::process::Command::new("wmctrl")
                                                    .args(["-a", filename])
                                                    .output()
                                                    .map(|o| o.status.success())
                                                    .unwrap_or(false)
                                                {
                                                    break;
                                                }

                                                // Try common editor window names
                                                for editor in &["Text Editor", "gedit", "kate", "GNOME Text Editor"] {
                                                    if std::process::Command::new("wmctrl")
                                                        .args(["-a", editor])
                                                        .output()
                                                        .map(|o| o.status.success())
                                                        .unwrap_or(false)
                                                    {
                                                        return;
                                                    }
                                                }

                                                // Try xdotool as fallback
                                                if let Ok(output) = std::process::Command::new("xdotool").args(["search", "--name", filename]).output() {
                                                    if let Ok(stdout) = String::from_utf8(output.stdout) {
                                                        if let Some(wid) = stdout.lines().last() {
                                                            if !wid.is_empty() {
                                                                let _ = std::process::Command::new("xdotool").args(["windowactivate", wid]).output();
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    }

                                    child_result
                                };

                                #[cfg(target_os = "macos")]
                                let result = std::process::Command::new("open").arg(&path).spawn();

                                #[cfg(target_os = "windows")]
                                let result = std::process::Command::new("cmd")
                                    .args(&["/C", "start", "", path.to_str().unwrap_or("")])
                                    .spawn();

                                #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                                let result: Result<std::process::Child, std::io::Error> =
                                    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Unsupported platform"));

                                match result {
                                    Err(e) => {
                                        eprintln!("\n========================================");
                                        eprintln!("❌ FAILED TO OPEN SETTINGS FILE");
                                        eprintln!("========================================");
                                        eprintln!("Error: {}", e);
                                        eprintln!("Location: {:?}", path);
                                        eprintln!("========================================\n");
                                    }
                                    Ok(_) => {
                                        eprintln!("\n========================================");
                                        eprintln!("✓ SETTINGS FILE OPENED");
                                        eprintln!("========================================");
                                        eprintln!("Location: {:?}", path);
                                        eprintln!("Editor should now be in foreground");
                                        eprintln!("========================================\n");

                                        // Show desktop notification (Linux)
                                        #[cfg(target_os = "linux")]
                                        {
                                            let _ = std::process::Command::new("notify-send")
                                                .args([
                                                    "-u",
                                                    "normal",
                                                    "-t",
                                                    "3000",
                                                    "Settings Opened",
                                                    &format!("Settings file opened in your text editor\n{}", path.display()),
                                                ])
                                                .spawn();
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("\n========================================");
                                eprintln!("❌ FAILED TO GET SETTINGS PATH");
                                eprintln!("========================================");
                                eprintln!("Error: {}", e);
                                eprintln!("========================================\n");
                            }
                        }
                    }
                    input::events::EventAction::ChangeFontSize(delta) => {
                        // Update font size in settings
                        settings.terminal.font_size = (settings.terminal.font_size + delta).clamp(8.0, 48.0);
                        eprintln!("[MAIN] Font size changed to: {}", settings.terminal.font_size);

                        // Save updated settings
                        if let Err(e) = settings::save_settings(&settings) {
                            eprintln!("[MAIN] Failed to save settings: {}", e);
                        }

                        // Reload fonts at new size (scaled for physical pixels)
                        let new_font_size = settings.terminal.font_size * scale_factor;

                        match ttf_context.load_font(&font_path, new_font_size) {
                            Ok(new_font) => {
                                font = new_font;

                                // Recalculate character dimensions (font is already scaled)
                                if let Ok((w, h)) = font.size_of_char('M') {
                                    char_width = w as f32;
                                    char_height = h as f32;
                                    eprintln!("[MAIN] New character dimensions: {:.2}x{:.2} pixels", char_width, char_height);

                                    // Clear glyph cache - old glyphs are wrong size
                                    glyph_cache.clear();
                                    eprintln!("[MAIN] Glyph cache cleared");
                                } else {
                                    eprintln!("[MAIN] Failed to measure character dimensions after font reload");
                                }
                            }
                            Err(e) => {
                                eprintln!("[MAIN] Failed to reload font at new size: {}", e);
                            }
                        }
                    }
                    input::events::EventAction::None => {}
                }

                // Handle resize if needed (after pane closure or divider drag)
                if result.needs_resize {
                    let (w, h) = canvas.window().size_in_pixels();
                    resize_terminals_to_panes(&tab_bar_gui, char_width, char_height, tab_bar_height, w, h);

                    #[cfg(feature = "test-server")]
                    if let Some(ref server) = test_server {
                        server.update_tabs(tab_bar_gui.lock().unwrap().get_all_terminals());
                    }
                }

                if result.needs_render {
                    needs_render = true;
                }
            }

            // Check for dead terminals and clean up panes
            let mut need_resize = false;
            {
                let mut gui = tab_bar_gui.lock().unwrap();
                let mut tabs_to_remove = Vec::new();

                for (tab_idx, tab_state) in gui.tab_states.iter_mut().enumerate() {
                    let terminals_with_ids = tab_state.pane_layout.get_terminals_with_pane_ids();
                    let mut panes_to_close = Vec::new();

                    for (pane_id, terminal) in terminals_with_ids {
                        let mut term = terminal.lock().unwrap();
                        if term.has_process_exited() {
                            eprintln!("[MAIN] Terminal process exited for pane {:?}, closing pane", pane_id);
                            panes_to_close.push(pane_id);
                        }
                    }

                    let any_panes_closed = !panes_to_close.is_empty();
                    for pane_id in panes_to_close {
                        if tab_state.pane_layout.close_pane(pane_id) {
                            // Last pane in tab closed
                            tabs_to_remove.push(tab_idx);
                        }
                    }

                    // Track if we need to resize terminals after closing panes
                    if any_panes_closed && !tabs_to_remove.contains(&tab_idx) {
                        need_resize = true;
                    }
                }

                // Check if we're about to remove the last tab
                let is_removing_last_tab = !tabs_to_remove.is_empty() && gui.tab_states.len() == tabs_to_remove.len();

                if is_removing_last_tab {
                    // Last tab(s) being removed - ask for confirmation
                    eprintln!("[MAIN] All tabs closing (processes exited)");
                    drop(gui);

                    if !ui::dialogs::confirm_quit(&mut canvas, &mut event_pump, &tab_font, scale_factor) {
                        // User cancelled quit - spawn a new terminal to replace the dead one
                        eprintln!("[MAIN] User cancelled quit, spawning new terminal");

                        let (w, h) = canvas.window().size_in_pixels();
                        let term_height = ((h - tab_bar_height) as f32 / char_height).floor() as u32;
                        let term_width = (w as f32 / char_width).floor() as u32;

                        let new_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                            term_width,
                            term_height,
                            shell_config.clone(),
                            DEFAULT_SCROLLBACK_LINES,
                            std::env::current_dir().ok(),
                        )));

                        let mut gui = tab_bar_gui.lock().unwrap();
                        gui.add_tab(new_terminal, "Tab 1".to_string());
                        drop(gui);

                        needs_render = true;
                        need_resize = true;

                        #[cfg(feature = "test-server")]
                        if let Some(ref server) = test_server {
                            server.update_tabs(tab_bar_gui.lock().unwrap().get_all_terminals());
                        }
                    } else {
                        // User confirmed quit
                        if let Err(e) = state::save_state(&tab_bar_gui.lock().unwrap()) {
                            eprintln!("[MAIN] Failed to save state: {}", e);
                        }
                        break 'running;
                    }
                } else {
                    // Remove tabs with no panes (in reverse order to maintain indices)
                    for tab_idx in tabs_to_remove.into_iter().rev() {
                        eprintln!("[MAIN] Removing tab {} (all panes closed)", tab_idx);
                        gui.remove_tab(tab_idx);
                        #[cfg(feature = "test-server")]
                        if let Some(ref server) = test_server {
                            server.update_tabs(gui.get_all_terminals());
                        }
                    }
                }
            }

            // Resize terminals if panes were closed
            if need_resize {
                let (w, h) = canvas.window().size_in_pixels();
                resize_terminals_to_panes(&tab_bar_gui, char_width, char_height, tab_bar_height, w, h);
            }

            // Handle pending context menu actions
            {
                let mut gui = tab_bar_gui.lock().unwrap();
                if let Some(pane_layout) = gui.get_active_pane_layout() {
                    if let Some((pane_id, action)) = pane_layout.pending_context_action.take() {
                        match action.as_str() {
                            "split_vertical" => {
                                pane_layout.set_active_pane(pane_id);
                                pending_pane_split = Some(crate::pane_layout::SplitDirection::Vertical);
                            }
                            "split_horizontal" => {
                                pane_layout.set_active_pane(pane_id);
                                pending_pane_split = Some(crate::pane_layout::SplitDirection::Horizontal);
                            }
                            "to_tab" => {
                                if let Some(terminal) = pane_layout.extract_pane(pane_id) {
                                    let new_tab_index = gui.tab_states.len() + 1;
                                    gui.add_tab(terminal, format!("Tab {}", new_tab_index));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Handle pending operations
            if pending_new_tab {
                pending_new_tab = false;
                let (w, h) = canvas.window().size_in_pixels();
                let term_height = ((h - tab_bar_height) as f32 / char_height).floor() as u32;
                let term_width = (w as f32 / char_width).floor() as u32;

                // Get cwd from active terminal before creating new tab
                let start_dir = {
                    let gui = tab_bar_gui.lock().unwrap();
                    gui.get_active_terminal().and_then(|t| t.lock().unwrap().get_cwd())
                };

                let new_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                    term_width,
                    term_height,
                    shell_config.clone(),
                    DEFAULT_SCROLLBACK_LINES,
                    start_dir,
                )));

                let mut gui = tab_bar_gui.lock().unwrap();
                let new_tab_index = gui.tab_states.len() + 1;
                gui.add_tab(new_terminal, format!("Tab {}", new_tab_index));

                #[cfg(feature = "test-server")]
                if let Some(ref server) = test_server {
                    server.update_tabs(gui.get_all_terminals());
                }
            }

            if let Some(direction) = pending_pane_split.take() {
                let (w, h) = canvas.window().size_in_pixels();

                // Check if the current active pane is large enough to split
                let mut can_split = false;
                {
                    let gui = tab_bar_gui.lock().unwrap();
                    if let Some(pane_layout_state) = gui.tab_states.get(gui.active_tab) {
                        let pane_area_y = tab_bar_height as i32;
                        let pane_area_height = h - tab_bar_height;
                        let pane_rects = pane_layout_state.pane_layout.get_pane_rects(0, pane_area_y, w, pane_area_height);

                        // Find the active pane's dimensions
                        if let Some((_, rect, _, _)) = pane_rects.iter().find(|(id, _, _, _)| *id == pane_layout_state.pane_layout.active_pane) {
                            let (current_cols, current_rows) = crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), char_width, char_height);

                            // Calculate dimensions after split (accounting for 2-pixel divider)
                            let divider_chars_h = (2.0 / char_width).ceil() as u32;
                            let divider_chars_v = (2.0 / char_height).ceil() as u32;

                            match direction {
                                crate::pane_layout::SplitDirection::Horizontal => {
                                    // Each pane will be roughly half width
                                    let split_width = (current_cols.saturating_sub(divider_chars_h)) / 2;
                                    if split_width >= 10 && current_rows >= 5 {
                                        can_split = true;
                                    } else {
                                        eprintln!("[SPLIT] Cannot split horizontally: resulting width {} would be less than 10 chars", split_width);
                                    }
                                }
                                crate::pane_layout::SplitDirection::Vertical => {
                                    // Each pane will be roughly half height
                                    let split_height = (current_rows.saturating_sub(divider_chars_v)) / 2;
                                    if split_height >= 5 && current_cols >= 10 {
                                        can_split = true;
                                    } else {
                                        eprintln!("[SPLIT] Cannot split vertically: resulting height {} would be less than 5 chars", split_height);
                                    }
                                }
                            }
                        }
                    }
                }

                if !can_split {
                    eprintln!("[SPLIT] Pane too small to split (minimum: 10 chars wide, 5 chars tall)");
                    // Skip the split operation
                } else {
                    let term_height = ((h - tab_bar_height) as f32 / char_height).floor() as u32;
                    let term_width = (w as f32 / char_width).floor() as u32;

                    // Get cwd from active terminal before splitting
                    let start_dir = {
                        let gui = tab_bar_gui.lock().unwrap();
                        gui.get_active_terminal().and_then(|t| t.lock().unwrap().get_cwd())
                    };

                    let new_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                        term_width,
                        term_height,
                        shell_config.clone(),
                        DEFAULT_SCROLLBACK_LINES,
                        start_dir,
                    )));

                    let mut gui = tab_bar_gui.lock().unwrap();
                    if let Some(pane_layout) = gui.get_active_pane_layout() {
                        pane_layout.split_active_pane(direction, new_terminal);
                    }
                    drop(gui); // Release lock before calling resize function

                    // Resize all terminals to match their new pane dimensions
                    let (w, h) = canvas.window().size_in_pixels();
                    resize_terminals_after_split(&tab_bar_gui, char_width, char_height, tab_bar_height, w, h);

                    #[cfg(feature = "test-server")]
                    if let Some(ref server) = test_server {
                        let gui = tab_bar_gui.lock().unwrap();
                        server.update_tabs(gui.get_all_terminals());
                    }
                }
            }

            // Render everything using optimized render module
            // This only renders the active tab and visible content
            let any_dirty = render::render_frame(
                &mut canvas,
                &texture_creator,
                &mut tab_bar,
                &tab_bar_gui,
                &tab_font,
                &button_font,
                &cpu_font,
                &font,
                &emoji_font,
                &unicode_fallback_font,
                &context_menu_font,
                cpu_usage,
                tab_bar_height,
                scale_factor,
                char_width,
                char_height,
                cursor_visible,
                &settings,
                &mut glyph_cache,
            )?;

            if any_dirty {
                needs_render = true;
            }

            // Periodically clear glyph cache to prevent unlimited memory growth
            if last_cache_clear.elapsed().as_secs() > 60 {
                glyph_cache.clear();
                last_cache_clear = Instant::now();
            }
        } else {
            skip_render_count += 1;
            // Print skip message every 100 iterations or on first skip
        }

        // Handle test server
        #[cfg(feature = "test-server")]
        if let Some(ref server) = test_server {
            match server.handle_connections() {
                Ok(true) => {
                    eprintln!("[MAIN] Shutdown requested by test server");
                    if let Ok(gui) = tab_bar_gui.try_lock() {
                        if let Err(e) = state::save_state(&gui) {
                            eprintln!("[MAIN] Failed to save state: {}", e);
                        }
                    }
                    break 'running;
                }
                Ok(false) => {}
                Err(e) => {
                    eprintln!("[MAIN] Test server error: {}", e);
                }
            }
        }
    }

    // Note: State is already saved by all exit paths before breaking the 'running loop
    // (signal handling, quit actions, last tab closed, test server shutdown, etc.)

    Ok(())
}
