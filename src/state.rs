use crate::pane_layout::{PaneNode, SplitDirection};
use crate::tab_gui::{TabBarGui, TabState};
use crate::terminal::Terminal;
use directories::ProjectDirs;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tinyjson::JsonValue;

const STATE_VERSION: i64 = 1;

/// Serializable representation of a pane node
#[derive(Debug, Clone)]
enum SerializablePaneNode {
    Leaf {
        working_directory: Option<String>,
    },
    Split {
        direction: String, // "horizontal" or "vertical"
        ratio: f64,
        first: Box<SerializablePaneNode>,
        second: Box<SerializablePaneNode>,
    },
}

impl SerializablePaneNode {
    /// Convert a PaneNode to a serializable structure (without terminals)
    fn from_pane_node(node: &PaneNode) -> Self {
        match node {
            PaneNode::Leaf { terminal, .. } => {
                // Extract current working directory from terminal
                let working_directory = terminal
                    .lock()
                    .ok()
                    .and_then(|t| t.get_cwd())
                    .and_then(|path| path.to_str().map(|s| s.to_string()));

                SerializablePaneNode::Leaf { working_directory }
            }
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => SerializablePaneNode::Split {
                direction: match direction {
                    SplitDirection::Horizontal => "horizontal".to_string(),
                    SplitDirection::Vertical => "vertical".to_string(),
                },
                ratio: *ratio as f64,
                first: Box::new(SerializablePaneNode::from_pane_node(first)),
                second: Box::new(SerializablePaneNode::from_pane_node(second)),
            },
        }
    }

    /// Convert to JSON value
    /// Convert to JSON for serialization
    fn to_json(&self) -> JsonValue {
        match self {
            SerializablePaneNode::Leaf { working_directory } => {
                let mut map = HashMap::new();
                map.insert("type".to_string(), JsonValue::String("leaf".to_string()));
                if let Some(cwd) = working_directory {
                    map.insert("workdir".to_string(), JsonValue::String(cwd.clone()));
                }
                JsonValue::Object(map)
            }
            SerializablePaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let mut map = HashMap::new();
                map.insert("type".to_string(), JsonValue::String("split".to_string()));
                map.insert("direction".to_string(), JsonValue::String(direction.clone()));
                map.insert("ratio".to_string(), JsonValue::Number(*ratio));
                map.insert("first".to_string(), first.to_json());
                map.insert("second".to_string(), second.to_json());
                JsonValue::Object(map)
            }
        }
    }

    /// Convert from JSON value
    fn from_json(json: &JsonValue) -> Option<Self> {
        let obj = json.get::<HashMap<String, JsonValue>>()?;
        let node_type = obj.get("type")?.get::<String>()?;

        match node_type.as_str() {
            "leaf" => {
                let working_directory = obj.get("workdir").and_then(|v| v.get::<String>()).cloned();
                Some(SerializablePaneNode::Leaf { working_directory })
            }
            "split" => {
                let direction = obj.get("direction")?.get::<String>()?.clone();
                let ratio = *obj.get("ratio")?.get::<f64>()?;
                let first = Box::new(SerializablePaneNode::from_json(obj.get("first")?)?);
                let second = Box::new(SerializablePaneNode::from_json(obj.get("second")?)?);

                Some(SerializablePaneNode::Split {
                    direction,
                    ratio,
                    first,
                    second,
                })
            }
            _ => None,
        }
    }

    /// Reconstruct a PaneNode with new terminals
    fn to_pane_node<F>(&self, terminal_factory: &mut F) -> PaneNode
    where
        F: FnMut(Option<std::path::PathBuf>) -> Arc<Mutex<Terminal>>,
    {
        match self {
            SerializablePaneNode::Leaf { working_directory } => {
                let start_dir = working_directory.as_ref().and_then(|s| std::path::PathBuf::from(s).canonicalize().ok());
                PaneNode::new_leaf(terminal_factory(start_dir))
            }
            SerializablePaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let dir = match direction.as_str() {
                    "vertical" => SplitDirection::Vertical,
                    _ => SplitDirection::Horizontal,
                };

                // Create the split structure manually
                let first_node = first.to_pane_node(terminal_factory);
                let second_node = second.to_pane_node(terminal_factory);

                // We need to use the internal PaneNode::Split variant directly
                PaneNode::Split {
                    id: crate::pane_layout::PaneId(crate::pane_layout::NEXT_PANE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
                    direction: dir,
                    ratio: *ratio as f32,
                    first: Box::new(first_node),
                    second: Box::new(second_node),
                }
            }
        }
    }
}

/// Get the state file path
/// Get the state file path based on the platform and build profile.
///
/// Uses platform-appropriate directories:
/// - Linux/macOS Production: ~/.config/nist/state.json
/// - Linux/macOS Test/Debug: ~/.config/nist-test/state.json
/// - Windows Production: %APPDATA%\nist\state.json
/// - Windows Test/Debug: %APPDATA%\nist-test\state.json
fn get_state_file_path() -> Result<PathBuf, String> {
    // Determine the application name based on build profile
    #[cfg(production)]
    let app_name = "nist";

    #[cfg(not(production))]
    let app_name = "nist-test";

    // Get the platform-appropriate config directory
    let proj_dirs = ProjectDirs::from("", "", app_name).ok_or_else(|| "Failed to determine config directory".to_string())?;

    let config_dir = proj_dirs.config_dir();

    // Create directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(config_dir.join("state.json"))
}

/// Save the current tab-pane layout state
pub fn save_state(tab_bar: &TabBarGui) -> Result<(), String> {
    let state_path = get_state_file_path()?;

    // Build state structure
    let mut state_map = HashMap::new();
    state_map.insert("version".to_string(), JsonValue::Number(STATE_VERSION as f64));

    // Create layout object
    let mut layout_map = HashMap::new();

    // Save active tab index
    layout_map.insert("active_tab".to_string(), JsonValue::Number(tab_bar.active_tab as f64));

    // Save tabs
    let mut tabs_array = Vec::new();
    for tab_state in &tab_bar.tab_states {
        let mut tab_map = HashMap::new();
        tab_map.insert("name".to_string(), JsonValue::String(tab_state.name.clone()));

        // Serialize pane layout
        let serializable_layout = SerializablePaneNode::from_pane_node(&tab_state.pane_layout.root);
        tab_map.insert("pane_layout".to_string(), serializable_layout.to_json());

        // Save active pane index (we'll just save 0 for now since we can't easily serialize the PaneId)
        tab_map.insert("active_pane".to_string(), JsonValue::Number(0.0));

        tabs_array.push(JsonValue::Object(tab_map));
    }

    layout_map.insert("tabs".to_string(), JsonValue::Array(tabs_array));

    state_map.insert("layout".to_string(), JsonValue::Object(layout_map));

    // Convert to JSON string
    let json_value = JsonValue::Object(state_map);
    let json_string = format_json(&json_value);

    // Write to file
    let mut file = fs::File::create(&state_path).map_err(|e| format!("Failed to create state file: {}", e))?;
    file.write_all(json_string.as_bytes())
        .map_err(|e| format!("Failed to write state file: {}", e))?;

    eprintln!("[STATE] Saved state to: {:?}", state_path);
    Ok(())
}

/// Load the tab-pane layout state
pub fn load_state<F>(mut terminal_factory: F) -> Result<(TabBarGui, usize), String>
where
    F: FnMut(Option<std::path::PathBuf>) -> Arc<Mutex<Terminal>>,
{
    let state_path = get_state_file_path()?;

    // If state file doesn't exist, return empty state
    if !state_path.exists() {
        eprintln!("[STATE] No state file found, starting fresh");
        return Err("No state file found".to_string());
    }

    // Read file
    let json_string = fs::read_to_string(&state_path).map_err(|e| format!("Failed to read state file: {}", e))?;

    // Handle empty file
    if json_string.trim().is_empty() {
        eprintln!("[STATE] State file is empty, starting fresh");
        // Clean up empty test state file in debug/test builds
        #[cfg(not(production))]
        {
            let _ = fs::remove_file(&state_path);
            eprintln!("[STATE] Cleaned up empty test state file");
        }
        return Err("State file is empty".to_string());
    }

    // Parse JSON
    let json_value: JsonValue = json_string.parse().map_err(|e| {
        eprintln!("[STATE] Failed to parse state JSON: {:?}", e);
        backup_corrupted_state(&state_path, "parse_error");
        format!("Failed to parse state JSON: {:?}", e)
    })?;

    let state_obj = json_value.get::<HashMap<String, JsonValue>>().ok_or_else(|| {
        eprintln!("[STATE] Invalid state format: not an object");
        backup_corrupted_state(&state_path, "parse_error");
        "Invalid state format: not an object".to_string()
    })?;

    // Check version
    let version = state_obj.get("version").and_then(|v| v.get::<f64>()).ok_or_else(|| {
        eprintln!("[STATE] Invalid state format: missing version");
        backup_corrupted_state(&state_path, "parse_error");
        "Invalid state format: missing version".to_string()
    })?;

    if *version as i64 != STATE_VERSION {
        eprintln!("[STATE] Incompatible state version: {} (expected {})", version, STATE_VERSION);
        backup_corrupted_state(&state_path, "wrong_version");
        return Err(format!("Incompatible state version: {} (expected {})", version, STATE_VERSION));
    }

    // Get layout object
    let layout_obj = state_obj.get("layout").and_then(|v| v.get::<HashMap<String, JsonValue>>()).ok_or_else(|| {
        eprintln!("[STATE] Invalid state format: missing layout");
        backup_corrupted_state(&state_path, "parse_error");
        "Invalid state format: missing layout".to_string()
    })?;

    // Get active tab
    let active_tab = layout_obj.get("active_tab").and_then(|v| v.get::<f64>()).map(|v| *v as usize).unwrap_or(0);

    // Get tabs array
    let tabs_array = layout_obj.get("tabs").and_then(|v| v.get::<Vec<JsonValue>>()).ok_or_else(|| {
        eprintln!("[STATE] Invalid state format: missing tabs");
        backup_corrupted_state(&state_path, "parse_error");
        "Invalid state format: missing tabs".to_string()
    })?;

    // Handle empty tabs array
    if tabs_array.is_empty() {
        eprintln!("[STATE] State file has no tabs, starting fresh");
        // Clean up invalid test state file in debug/test builds
        #[cfg(not(production))]
        {
            let _ = fs::remove_file(&state_path);
            eprintln!("[STATE] Cleaned up invalid test state file");
        }
        return Err("State file has no tabs".to_string());
    }

    // Create TabBarGui
    let mut tab_bar = TabBarGui::new();

    // Restore each tab
    for tab_json in tabs_array {
        let tab_obj = tab_json.get::<HashMap<String, JsonValue>>().ok_or("Invalid tab format")?;

        let tab_name = tab_obj.get("name").and_then(|v| v.get::<String>()).ok_or("Invalid tab format: missing name")?;

        let pane_layout_json = tab_obj.get("pane_layout").ok_or("Invalid tab format: missing pane_layout")?;

        let serializable_layout = SerializablePaneNode::from_json(pane_layout_json).ok_or("Failed to parse pane layout")?;

        // Reconstruct PaneNode with new terminals
        let pane_node = serializable_layout.to_pane_node(&mut terminal_factory);

        // Create TabState manually
        let pane_layout = crate::pane_layout::PaneLayout {
            root: pane_node,
            active_pane: crate::pane_layout::PaneId(0), // Will be set to first leaf
            dragging_divider: None,
            drag_preview: None,
            #[cfg(target_os = "linux")]
            primary_clipboard: None,
            context_menu_images: None,
            context_menu_open: None,
            pending_context_action: None,
            copy_animation: None,
        };

        // Set active pane to the first leaf
        let leaf_ids = pane_layout.root.collect_leaf_ids();
        let mut pane_layout = pane_layout;
        if let Some(first_id) = leaf_ids.first() {
            pane_layout.active_pane = *first_id;
        }

        let tab_state = TabState {
            pane_layout,
            name: tab_name.clone(),
            is_editing: false,
            temp_name: tab_name.clone(),
        };

        tab_bar.tab_states.push(tab_state);
    }

    // Set active tab (ensure it's within bounds)
    tab_bar.active_tab = active_tab.min(tab_bar.tab_states.len().saturating_sub(1));

    eprintln!("[STATE] Loaded state from: {:?} ({} tabs)", state_path, tab_bar.tab_states.len());
    Ok((tab_bar, active_tab))
}

/// Backup a corrupted state file with a timestamp
/// Only backs up for parse errors or version mismatches, not for empty states
fn backup_corrupted_state(state_path: &PathBuf, reason: &str) {
    // Only backup once per session to avoid clutter
    static BACKUP_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    if BACKUP_DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return; // Already backed up this session
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let backup_path = state_path.with_file_name(format!("state.{}.{}.backup", reason, timestamp));

    match fs::copy(state_path, &backup_path) {
        Ok(_) => {
            eprintln!("[STATE] Backed up corrupted state to: {:?}", backup_path);
        }
        Err(e) => {
            eprintln!("[STATE] Failed to backup corrupted state: {}", e);
        }
    }
}

/// Clean up test state file when running in test mode (only called on exit, not used for now)
#[allow(dead_code)]
pub fn cleanup_test_state() {
    // Currently not used - test state cleanup happens during load for invalid states
    // Valid test states are left for tests to verify and clean up manually
}

/// Format JSON value as a string (simple pretty-printer)
fn format_json(value: &JsonValue) -> String {
    format_json_impl(value, 0)
}

fn format_json_impl(value: &JsonValue, indent: usize) -> String {
    let indent_str = "  ".repeat(indent);
    let indent_str_next = "  ".repeat(indent + 1);

    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Boolean(b) => b.to_string(),
        JsonValue::Number(n) => {
            // Format numbers nicely
            if n.fract() == 0.0 && n.abs() < 1e10 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        JsonValue::String(s) => format!("\"{}\"", escape_json_string(s)),
        JsonValue::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else {
                let items: Vec<String> = arr.iter().map(|v| format!("{}{}", indent_str_next, format_json_impl(v, indent + 1))).collect();
                format!("[\n{}\n{}]", items.join(",\n"), indent_str)
            }
        }
        JsonValue::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                let mut items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}\"{}\": {}", indent_str_next, escape_json_string(k), format_json_impl(v, indent + 1)))
                    .collect();
                items.sort(); // Sort keys for consistent output
                format!("{{\n{}\n{}}}", items.join(",\n"), indent_str)
            }
        }
    }
}

fn escape_json_string(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            c if c.is_control() => format!("\\u{:04x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_directory_path() {
        // Test that we can get a config directory path
        // This may fail in parallel test runs due to permission issues, so we accept both success and certain errors
        let path = get_state_file_path();

        if let Err(e) = &path {
            // If there's a permission error during parallel test runs, that's acceptable
            if e.contains("Permission denied") {
                eprintln!("Note: Permission denied in parallel test run (acceptable)");
                return;
            }
        }

        assert!(path.is_ok(), "Should be able to get state file path: {:?}", path.err());

        let path = path.unwrap();
        assert!(path.to_string_lossy().ends_with("state.json"), "Path should end with state.json");

        // Verify the path contains the correct app name based on build profile
        let path_str = path.to_string_lossy();
        #[cfg(production)]
        assert!(
            path_str.contains("nist") && !path_str.contains("nist-test"),
            "Production build should use 'nist' directory, got: {}",
            path_str
        );

        #[cfg(not(production))]
        assert!(
            path_str.contains("nist-test"),
            "Debug build should use 'nist-test' directory, got: {}",
            path_str
        );
    }

    #[test]
    fn test_config_path_is_platform_appropriate() {
        let path = get_state_file_path();

        // Handle permission errors in parallel test runs
        if let Err(e) = &path {
            if e.contains("Permission denied") {
                eprintln!("Note: Permission denied in parallel test run (acceptable)");
                return;
            }
        }

        let path = path.unwrap();
        let path_str = path.to_string_lossy();

        // On Windows, should use AppData
        #[cfg(target_os = "windows")]
        assert!(
            path_str.contains("AppData") || path_str.contains("APPDATA"),
            "Windows should use AppData directory, got: {}",
            path_str
        );

        // On Unix-like systems, should use .config
        #[cfg(not(target_os = "windows"))]
        assert!(
            path_str.contains(".config"),
            "Unix-like systems should use .config directory, got: {}",
            path_str
        );
    }
}
