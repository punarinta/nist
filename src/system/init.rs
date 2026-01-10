//! Initialization module for setting up SDL, fonts, terminals, and all required components.
//!
//! This module handles the complex initialization sequence including:
//! - SDL3 and window setup
//! - Font loading and character metrics
//! - Terminal and GUI state initialization
//! - Signal handlers and system monitoring

use crate::font_discovery;
use crate::settings;
use crate::state;
use crate::tab_gui::TabBarGui;
use crate::terminal::Terminal;
use crate::terminal_config::TerminalLibrary;
use arboard::Clipboard;
use sdl3::render::{Canvas, TextureCreator};
use sdl3::ttf::Sdl3TtfContext;
use sdl3::video::{Window, WindowContext};
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use std::sync::mpsc::channel;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use sysinfo::System;

// TestServer is conditionally compiled at crate root level

/// Container for all loaded fonts at various sizes
pub struct Fonts<'a> {
    /// Main monospace font for terminal text
    pub font: sdl3::ttf::Font<'a>,
    /// UI font for tabs and menus
    pub tab_font: sdl3::ttf::Font<'a>,
    /// Font for window control buttons
    pub button_font: sdl3::ttf::Font<'a>,
    /// Font for CPU indicator
    pub cpu_font: sdl3::ttf::Font<'a>,
    /// Font for context menus
    pub context_menu_font: sdl3::ttf::Font<'a>,
    /// Emoji font for emoji rendering
    pub emoji_font: sdl3::ttf::Font<'a>,
    /// Unicode fallback font for symbols
    pub unicode_fallback_font: sdl3::ttf::Font<'a>,
}

/// Character dimensions in pixels
#[derive(Debug, Clone, Copy)]
pub struct CharDimensions {
    pub width: f32,
    pub height: f32,
}

/// Display scaling information
#[derive(Debug, Clone, Copy)]
pub struct ScaleInfo {
    pub scale_factor: f32,
    pub mouse_coords_need_scaling: bool,
}

/// All initialized components needed for the application
pub struct InitializedApp<'a> {
    pub canvas: Canvas<Window>,
    pub texture_creator: TextureCreator<WindowContext>,
    pub event_pump: sdl3::EventPump,
    pub fonts: Fonts<'a>,
    pub char_dims: CharDimensions,
    pub scale_info: ScaleInfo,
    pub tab_bar_height: u32,
    pub tab_bar: crate::sdl_renderer::TabBar,
    pub tab_bar_gui: Arc<Mutex<TabBarGui>>,
    pub settings: settings::Settings,
    pub sys: System,
    pub ctrl_keys: std::collections::HashMap<sdl3::keyboard::Scancode, u8>,
    pub mouse_state: crate::input::mouse::MouseState,
    pub glyph_cache: HashMap<(String, (u8, u8, u8)), sdl3::render::Texture<'a>>,
    #[cfg(target_os = "linux")]
    pub clipboard_tx: Sender<Clipboard>,
    #[cfg(target_os = "linux")]
    pub clipboard_rx: Receiver<Clipboard>,
    #[cfg(not(target_os = "windows"))]
    pub signal_rx: std::sync::mpsc::Receiver<i32>,
    #[cfg(feature = "test-server")]
    pub test_server: Option<crate::test_server::TestServer>,
}

/// Initialize SDL, create window, load fonts, and set up all required components
///
/// # Arguments
/// * `ttf_context` - TTF context that must outlive the returned fonts
/// * `test_port` - Optional port for test server
/// * `default_scrollback_lines` - Number of scrollback lines for terminals
///
/// # Returns
/// Returns initialized components with lifetimes tied to ttf_context
pub fn initialize<'a>(ttf_context: &'a Sdl3TtfContext, test_port: Option<u16>, default_scrollback_lines: usize) -> Result<InitializedApp<'a>, String> {
    // Set up signal handlers for graceful shutdown
    #[cfg(not(target_os = "windows"))]
    let signal_rx = setup_signal_handlers()?;

    let (window_width, window_height) = (2376_u32, 1593_u32);

    let sdl_context = sdl3::init().unwrap();

    // Set window class name for proper desktop integration
    configure_sdl_hints();

    let video_subsystem = sdl_context.video().unwrap();

    // Create window with high DPI awareness
    let mut window = create_window(&video_subsystem, window_width, window_height)?;

    // Set window icon
    set_window_icon(&mut window);

    // Create canvas with VSync
    let canvas = create_canvas(window)?;

    // Detect display scaling
    let scale_info = detect_scaling(&canvas);

    // Get window dimensions
    let (drawable_width, drawable_height) = canvas.window().size_in_pixels();

    // Load settings
    let settings = settings::load_settings().unwrap_or_else(|e| {
        eprintln!("[INIT] Failed to load settings, using defaults: {}", e);
        settings::Settings::default()
    });

    // Load all fonts
    let fonts = load_fonts(ttf_context, &settings, scale_info.scale_factor)?;

    // Measure character dimensions
    let char_dims = measure_char_dimensions(&fonts.font)?;

    // Set up rendering components
    let texture_creator = canvas.texture_creator();
    let event_pump = sdl_context.event_pump().map_err(|e| e.to_string())?;

    // Enable text input for terminal typing
    canvas.window().subsystem().text_input().start(canvas.window());

    // Set up clipboard channel (Linux only)
    #[cfg(target_os = "linux")]
    let (clipboard_tx, clipboard_rx) = channel();

    // Initialize system monitor
    let sys = System::new_all();

    // Set up terminal library and shell config
    let term_library = TerminalLibrary::new();
    let shell_config = term_library.get_default_shell().clone();

    // Calculate tab bar height with scaling
    let tab_bar_height = (36.0 * scale_info.scale_factor) as u32;
    let tab_bar = crate::sdl_renderer::TabBar::new(tab_bar_height);

    // Calculate terminal dimensions
    let terminal_height = ((drawable_height - tab_bar_height) as f32 / char_dims.height).floor() as u32;
    let terminal_width = (drawable_width as f32 / char_dims.width).floor() as u32;

    // Initialize tab bar GUI with state loading
    let tab_bar_gui = initialize_tab_bar_gui(terminal_width, terminal_height, shell_config, default_scrollback_lines);

    // Set context menu images
    load_and_set_context_menu_images(&tab_bar_gui);

    // Initialize test server if requested
    #[cfg(feature = "test-server")]
    let test_server = initialize_test_server(
        test_port,
        &tab_bar_gui,
        char_dims.width,
        char_dims.height,
        tab_bar_height,
        drawable_width,
        drawable_height,
    );

    // Create control key map
    let ctrl_keys = crate::input::keyboard::create_ctrl_key_map();

    // Initialize mouse state
    let mouse_state = crate::input::mouse::MouseState::new();

    // Initialize glyph cache
    let glyph_cache = HashMap::new();

    Ok(InitializedApp {
        canvas,
        texture_creator,
        event_pump,
        fonts,
        char_dims,
        scale_info,
        tab_bar_height,
        tab_bar,
        tab_bar_gui,
        settings,
        sys,
        ctrl_keys,
        mouse_state,
        glyph_cache,
        #[cfg(target_os = "linux")]
        clipboard_tx,
        #[cfg(target_os = "linux")]
        clipboard_rx,
        #[cfg(not(target_os = "windows"))]
        signal_rx,
        #[cfg(feature = "test-server")]
        test_server,
    })
}

/// Set up signal handlers for graceful shutdown on Unix systems
#[cfg(not(target_os = "windows"))]
fn setup_signal_handlers() -> Result<std::sync::mpsc::Receiver<i32>, String> {
    use signal_hook::consts::signal::*;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).map_err(|e| format!("Failed to register signal handlers: {}", e))?;

    let (signal_tx, signal_rx) = channel::<i32>();
    std::thread::spawn(move || {
        for sig in signals.forever() {
            eprintln!("[SIGNAL] Received signal: {}", sig);
            let _ = signal_tx.send(sig);
        }
    });

    Ok(signal_rx)
}

/// Configure SDL hints for proper window management
fn configure_sdl_hints() {
    sdl3::hint::set("SDL_VIDEO_X11_WMCLASS", "nist");
    sdl3::hint::set("SDL_VIDEO_WAYLAND_WMCLASS", "nist");
    sdl3::hint::set("SDL_VIDEO_WAYLAND_APP_ID", "nist");
    sdl3::hint::set("SDL_APP_ID", "nist");
    sdl3::hint::set("SDL_APP_NAME", "Nisdos Terminal");
}

/// Create the main window
fn create_window(video_subsystem: &sdl3::VideoSubsystem, width: u32, height: u32) -> Result<Window, String> {
    video_subsystem
        .window("Nisdos Terminal", width, height)
        .position_centered()
        .resizable()
        .maximized()
        .borderless()
        .high_pixel_density()
        .build()
        .map_err(|e| e.to_string())
}

/// Set the window icon from embedded PNG data
fn set_window_icon(window: &mut Window) {
    const ICON_DATA: &[u8] = include_bytes!("../../icon.png");

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
                    eprintln!("[INIT] Failed to create icon surface: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("[INIT] Failed to load window icon: {}", e);
        }
    }
}

/// Create an SDL surface from RGBA pixel data
fn create_sdl_surface_from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<sdl3::surface::Surface<'static>, String> {
    let mut surface =
        sdl3::surface::Surface::new(width, height, sdl3::pixels::PixelFormat::RGBA32).map_err(|e| format!("Failed to create SDL surface: {}", e))?;

    surface.with_lock_mut(|buffer: &mut [u8]| {
        buffer.copy_from_slice(&pixels);
    });

    Ok(surface)
}

/// Create canvas with VSync enabled
fn create_canvas(window: Window) -> Result<Canvas<Window>, String> {
    let canvas = window.into_canvas();

    let initial_scale = canvas.window().display_scale();
    eprintln!("[INIT] Initial display scale: {:.2}", initial_scale);

    // Enable VSync to limit rendering to display refresh rate
    let vsync_success = unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) };

    if vsync_success {
        let mut vsync_value: std::os::raw::c_int = 0;
        let get_success = unsafe { sdl3::sys::render::SDL_GetRenderVSync(canvas.raw(), &mut vsync_value) };
        if get_success && vsync_value == 1 {
            eprintln!("[INIT] VSync enabled (verified: vsync={})", vsync_value);
        } else if get_success {
            eprintln!("[INIT] WARNING: VSync set to unexpected value: {}", vsync_value);
        } else {
            eprintln!("[INIT] WARNING: Could not verify VSync setting");
        }
    } else {
        eprintln!("[INIT] WARNING: Failed to enable VSync! CPU usage may be high.");
    }

    Ok(canvas)
}

/// Detect display scaling factors
fn detect_scaling(canvas: &Canvas<Window>) -> ScaleInfo {
    let (window_width_logical, window_height_logical) = canvas.window().size();
    let (drawable_width, drawable_height) = canvas.window().size_in_pixels();

    eprintln!("[INIT] Window size: {}x{} logical", window_width_logical, window_height_logical);
    eprintln!("[INIT] Drawable size: {}x{} pixels", drawable_width, drawable_height);
    eprintln!("[INIT] Pixel density: {:.2}", canvas.window().pixel_density());

    // Calculate scale factor from drawable vs logical size
    let mut scale_factor = if window_width_logical > 0 {
        drawable_width as f32 / window_width_logical as f32
    } else {
        1.0
    };

    // Use SDL3's display scale as authoritative source
    let window_display_scale = canvas.window().display_scale();
    if window_display_scale > 0.0 {
        eprintln!("[INIT] SDL3 display scale: {:.2}", window_display_scale);
        if (window_display_scale - scale_factor).abs() > 0.01 {
            eprintln!("[INIT] Using SDL3 display scale instead of calculated ratio");
            scale_factor = window_display_scale;
        }
    }

    // Fallback to pixel density if scale is still 1.0
    if scale_factor == 1.0 {
        let pixel_density = canvas.window().pixel_density();
        if pixel_density > 1.0 {
            scale_factor = pixel_density;
            eprintln!("[INIT] Using pixel density for scaling: {:.2}", scale_factor);
        }
    }

    let expected_physical_w = (window_width_logical as f32 * scale_factor) as u32;
    let expected_physical_h = (window_height_logical as f32 * scale_factor) as u32;
    eprintln!(
        "[INIT] Scale factor: {:.2}x, expected physical: {}x{}",
        scale_factor, expected_physical_w, expected_physical_h
    );

    // Determine if mouse coordinates need scaling
    let mouse_coords_need_scaling = scale_factor > 1.0 && {
        let (w_width, _) = canvas.window().size();
        let (d_width, _) = canvas.window().size_in_pixels();
        w_width != d_width
    };

    eprintln!(
        "[INIT] Mouse coordinate scaling: {}",
        if mouse_coords_need_scaling { "ENABLED" } else { "DISABLED" }
    );

    ScaleInfo {
        scale_factor,
        mouse_coords_need_scaling,
    }
}

/// Load all required fonts
fn load_fonts<'a>(ttf_context: &'a Sdl3TtfContext, settings: &settings::Settings, scale_factor: f32) -> Result<Fonts<'a>, String> {
    let font_size = settings.terminal.font_size * scale_factor;

    // Load monospace font
    let font_path = get_monospace_font_path(&settings.terminal.font_family)?;
    let font = ttf_context.load_font(&font_path, font_size).map_err(|e| {
        eprintln!("[INIT] Failed to load font from {}: {}", font_path, e);
        format!("Font loading failed from {}: {}", font_path, e)
    })?;

    eprintln!(
        "[INIT] Loaded monospace font: {} at size {:.1} (fontSize={}, fontFamily={})",
        font_path,
        font_size,
        settings.terminal.font_size,
        if settings.terminal.font_family == "auto" {
            "auto"
        } else {
            &settings.terminal.font_family
        }
    );

    // Load UI font
    let ui_font_path = font_discovery::find_best_ui_font().ok_or_else(|| "[ERROR] No suitable UI font found on your system!".to_string())?;

    let tab_font_size = 18.0 * scale_factor;
    let tab_font = ttf_context
        .load_font(&ui_font_path, tab_font_size)
        .map_err(|e| format!("Tab font loading failed: {}", e))?;

    let button_font_size = 27.0 * scale_factor;
    let button_font = ttf_context
        .load_font(&ui_font_path, button_font_size)
        .map_err(|e| format!("Button font loading failed: {}", e))?;

    let cpu_font_size = 13.0 * scale_factor;
    let cpu_font = ttf_context
        .load_font(&ui_font_path, cpu_font_size)
        .map_err(|e| format!("CPU font loading failed: {}", e))?;

    let context_menu_font_size = 12.0 * scale_factor;
    let context_menu_font = ttf_context
        .load_font(&ui_font_path, context_menu_font_size)
        .map_err(|e| format!("Context menu font loading failed: {}", e))?;

    eprintln!("[INIT] Loaded UI font: {} for tabs, menus, and controls", ui_font_path);

    // Load emoji font
    let emoji_font_path = font_discovery::find_emoji_font().unwrap_or_else(|| {
        eprintln!("[INIT] WARNING: No emoji font found, using UI font as fallback");
        ui_font_path.clone()
    });
    let emoji_font = ttf_context
        .load_font(&emoji_font_path, font_size)
        .map_err(|e| format!("Emoji font loading failed: {}", e))?;
    eprintln!("[INIT] Loaded emoji font: {}", emoji_font_path);

    // Load Unicode fallback font - use FreeMono for specific missing symbols (U+23BF, U+276F, U+2588)
    // Note: This may cause minor alignment issues in some apps, but allows these symbols to render
    let unicode_fallback_font_path = font_discovery::find_specific_font("FreeMono.ttf").unwrap_or_else(|| {
        eprintln!("[INIT] WARNING: FreeMono not found, using terminal font (some symbols may not render)");
        font_path.clone()
    });
    let unicode_fallback_font = ttf_context
        .load_font(&unicode_fallback_font_path, font_size)
        .map_err(|e| format!("Unicode fallback font loading failed: {}", e))?;
    eprintln!("[INIT] Loaded Unicode fallback font: {} (for special symbols)", unicode_fallback_font_path);

    Ok(Fonts {
        font,
        tab_font,
        button_font,
        cpu_font,
        context_menu_font,
        emoji_font,
        unicode_fallback_font,
    })
}

/// Get the monospace font path from settings or auto-discovery
fn get_monospace_font_path(font_family: &str) -> Result<String, String> {
    if font_family == "auto" {
        font_discovery::find_best_monospace_font().ok_or_else(|| "[ERROR] No suitable monospace font found on your system!".to_string())
    } else {
        let path = font_family.to_string();
        if !std::path::Path::new(&path).exists() {
            eprintln!("[INIT] Font file not found: {}, falling back to auto-discovery", path);
            font_discovery::find_best_monospace_font().ok_or_else(|| "[ERROR] No suitable monospace font found on your system!".to_string())
        } else {
            Ok(path)
        }
    }
}

/// Measure character dimensions from the font
fn measure_char_dimensions(font: &sdl3::ttf::Font) -> Result<CharDimensions, String> {
    let (char_width_i32, char_height_i32) = font.size_of_char('M').map_err(|e| e.to_string())?;
    let char_width = char_width_i32 as f32;
    let char_height = char_height_i32 as f32;

    eprintln!("[INIT] Character dimensions: {:.2}x{:.2} pixels", char_width, char_height);

    Ok(CharDimensions {
        width: char_width,
        height: char_height,
    })
}

/// Initialize tab bar GUI with state loading or default terminal
fn initialize_tab_bar_gui(
    terminal_width: u32,
    terminal_height: u32,
    shell_config: crate::terminal_config::ShellConfig,
    default_scrollback_lines: usize,
) -> Arc<Mutex<TabBarGui>> {
    let shell_config_clone = shell_config.clone();
    let terminal_factory = move |start_dir: Option<std::path::PathBuf>| {
        Arc::new(Mutex::new(Terminal::new_with_scrollback(
            terminal_width,
            terminal_height,
            shell_config_clone.clone(),
            default_scrollback_lines,
            start_dir,
        )))
    };

    match state::load_state(terminal_factory) {
        Ok((tab_bar_loaded, _active_tab)) => {
            eprintln!("[INIT] Successfully loaded state");
            Arc::new(Mutex::new(tab_bar_loaded))
        }
        Err(e) => {
            eprintln!("[INIT] Failed to load state: {}, creating default tab", e);
            let mut tab_bar_new = TabBarGui::new();
            let first_terminal = Arc::new(Mutex::new(Terminal::new_with_scrollback(
                terminal_width,
                terminal_height,
                shell_config,
                default_scrollback_lines,
                std::env::current_dir().ok(),
            )));
            tab_bar_new.add_tab(first_terminal, "Tab 1".to_string());
            Arc::new(Mutex::new(tab_bar_new))
        }
    }
}

/// Load context menu images and set them on all pane layouts
fn load_and_set_context_menu_images(tab_bar_gui: &Arc<Mutex<TabBarGui>>) {
    let context_menu_images = crate::pane_layout::ContextMenuImages::load();
    if let Ok(mut gui) = tab_bar_gui.try_lock() {
        gui.set_context_menu_images(context_menu_images);
    }
}

/// Initialize test server if port is specified
#[cfg(feature = "test-server")]
fn initialize_test_server(
    test_port: Option<u16>,
    tab_bar_gui: &Arc<Mutex<TabBarGui>>,
    char_width: f32,
    char_height: f32,
    tab_bar_height: u32,
    drawable_width: u32,
    drawable_height: u32,
) -> Option<crate::test_server::TestServer> {
    test_port.and_then(|port| {
        let terminals = match tab_bar_gui.try_lock() {
            Ok(gui) => gui.get_all_terminals(),
            Err(_) => {
                eprintln!("[INIT] Failed to get terminals for test server");
                Vec::new()
            }
        };

        match crate::test_server::TestServer::new(
            port,
            terminals,
            Arc::clone(tab_bar_gui),
            char_width,
            char_height,
            tab_bar_height,
            drawable_width,
            drawable_height,
        ) {
            Ok(server) => {
                eprintln!("[INIT] Test server enabled on port {}", port);
                Some(server)
            }
            Err(e) => {
                eprintln!("[INIT] Failed to start test server: {}", e);
                None
            }
        }
    })
}
