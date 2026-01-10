mod ansi;
mod font_discovery;
mod input;
mod pane_layout;
mod screen_buffer;
mod sdl_renderer;
mod settings;
mod state;
mod system;
mod tab_gui;
mod terminal;
mod terminal_config;
mod ui;

#[cfg(feature = "test-server")]
mod test_server;

use ui::render;

use crate::tab_gui::TabBarGui;
use crate::terminal::Terminal;
use crate::terminal_config::TerminalLibrary;

use sdl3::event::Event;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// Build-time version information
const BUILD_DATE: &str = env!("BUILD_DATE");
const GIT_HASH: &str = env!("GIT_HASH");
const DEFAULT_SCROLLBACK_LINES: usize = 10000;

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

            for (_pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
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
    new_pane_id: crate::pane_layout::PaneId,
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

        for (pane_id, rect, terminal, _is_active, _is_selected) in pane_rects {
            let (cols, rows) = crate::ui::render::calculate_terminal_size(rect.width(), rect.height(), char_width, char_height);

            match terminal.lock() {
                Ok(mut t) => {
                    // Only clear screen for the newly created pane, not existing ones
                    let clear_screen = pane_id == new_pane_id;
                    if t.width != cols || t.height != rows {
                        eprintln!(
                            "[RESIZE] Pane {:?}: {}x{} -> {}x{} (clear={})",
                            pane_id, t.width, t.height, cols, rows, clear_screen
                        );
                        t.set_size(cols, rows, clear_screen);
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
    eprintln!("[MAIN] Nisdos Terminal starting (built: {})", BUILD_DATE);

    // Print feature flags
    #[cfg(feature = "test-server")]
    eprintln!("[MAIN] Feature: test-server enabled");

    // Parse command-line arguments (exits if --help or --version)
    let cli_args = system::cli::parse_args(BUILD_DATE, GIT_HASH);

    // Initialize TTF context (must outlive fonts)
    let ttf_context = sdl3::ttf::init().map_err(|e| e.to_string())?;

    // Initialize all components (SDL, fonts, terminals, etc.)
    let app = system::init::initialize(&ttf_context, cli_args.test_port, DEFAULT_SCROLLBACK_LINES)?;

    // Destructure for easier access
    let mut canvas = app.canvas;
    let texture_creator = app.texture_creator;
    let mut event_pump = app.event_pump;
    let mut font = app.fonts.font;
    let tab_font = app.fonts.tab_font;
    let button_font = app.fonts.button_font;
    let cpu_font = app.fonts.cpu_font;
    let context_menu_font = app.fonts.context_menu_font;
    let emoji_font = app.fonts.emoji_font;
    let unicode_fallback_font = app.fonts.unicode_fallback_font;
    let mut char_width = app.char_dims.width;
    let mut char_height = app.char_dims.height;
    let scale_factor = app.scale_info.scale_factor;
    let mouse_coords_need_scaling = app.scale_info.mouse_coords_need_scaling;
    let tab_bar_height = app.tab_bar_height;
    let mut tab_bar = app.tab_bar;
    let tab_bar_gui = app.tab_bar_gui;
    let mut settings = app.settings;
    let mut sys = app.sys;
    let ctrl_keys = app.ctrl_keys;
    let mut mouse_state = app.mouse_state;
    let mut glyph_cache = app.glyph_cache;

    #[cfg(target_os = "linux")]
    let clipboard_tx = app.clipboard_tx;
    #[cfg(target_os = "linux")]
    let clipboard_rx = app.clipboard_rx;

    #[cfg(not(target_os = "windows"))]
    let signal_rx = app.signal_rx;

    #[cfg(feature = "test-server")]
    let test_server = app.test_server;

    // CPU monitoring state
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

    // Pending operations
    let mut pending_pane_split: Option<crate::pane_layout::SplitDirection> = None;
    let mut pending_new_tab = false;
    let mut last_cache_clear = Instant::now();

    // Store font path for reloading when font size changes
    let font_path = if settings.terminal.font_family == "auto" {
        font_discovery::find_best_monospace_font().unwrap_or_default()
    } else {
        settings.terminal.font_family.clone()
    };

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
        // Use 1ms timeout for responsive PTY output rendering
        // PTY data can arrive at any time, and we need to wake up quickly to render it
        let first_event = event_pump.wait_event_timeout(1);
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

        // Late dirty check: PTY data may have arrived during event wait
        // This catches screen updates that happened after the initial dirty check
        if !needs_render {
            let late_dirty_content = {
                match tab_bar_gui.try_lock() {
                    Ok(gui) => {
                        let terminals = gui.get_active_tab_terminals();
                        terminals.iter().any(|term| {
                            if let Ok(t) = term.try_lock() {
                                if let Ok(sb) = t.screen_buffer.try_lock() {
                                    sb.is_dirty()
                                } else {
                                    true // Assume dirty if mutex is locked (PTY thread likely processing)
                                }
                            } else {
                                true // Assume dirty if terminal is locked
                            }
                        })
                    }
                    Err(_) => true, // Assume dirty if GUI is locked
                }
            };

            if late_dirty_content {
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
                                    Err(e) => eprintln!("❌ Failed to open settings file | Error: {} | Location: {:?}", e, path),
                                    Ok(_) => {
                                        eprintln!("✓ Settings file opened | Location: {:?} | Editor should now be in foreground", path);

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
                                eprintln!("❌ Failed to get settings path | Error: {}", e);
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
                            "kill_shell" => {
                                if let Some(terminal_arc) = pane_layout.root.find_terminal(pane_id) {
                                    if let Ok(mut terminal) = terminal_arc.lock() {
                                        let _ = terminal.kill();
                                    }
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
                        if let Some((_, rect, _, _, _)) = pane_rects.iter().find(|(id, _, _, _, _)| *id == pane_layout_state.pane_layout.active_pane) {
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
                    let new_pane_id = if let Some(pane_layout) = gui.get_active_pane_layout() {
                        pane_layout.split_active_pane(direction, new_terminal.clone());
                        pane_layout.active_pane()
                    } else {
                        crate::pane_layout::PaneId(0)
                    };
                    drop(gui); // Release lock before calling resize function

                    // Resize all terminals to match their new pane dimensions
                    let (w, h) = canvas.window().size_in_pixels();
                    resize_terminals_after_split(&tab_bar_gui, char_width, char_height, tab_bar_height, w, h, new_pane_id);

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
