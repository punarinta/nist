use crate::terminal::Terminal;
use crate::ui::animations::CopyAnimation;
use sdl3::rect::Rect;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use arboard::Clipboard;

/// Unique identifier for a pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub usize);

pub static NEXT_PANE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl PaneId {
    fn new() -> Self {
        PaneId(NEXT_PANE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

/// Direction of split
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal, // Left | Right
    Vertical,   // Top / Bottom
}

/// A node in the pane tree - either a leaf (terminal) or a split container
#[derive(Clone)]
pub enum PaneNode {
    Leaf {
        id: PaneId,
        terminal: Arc<Mutex<Terminal>>,
    },
    Split {
        id: PaneId,
        direction: SplitDirection,
        /// Ratio of first child (0.0 to 1.0), second child gets (1.0 - ratio)
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// Create a new leaf node with a terminal
    pub fn new_leaf(terminal: Arc<Mutex<Terminal>>) -> Self {
        PaneNode::Leaf { id: PaneId::new(), terminal }
    }

    /// Get the pane ID
    pub fn id(&self) -> PaneId {
        match self {
            PaneNode::Leaf { id, .. } => *id,
            PaneNode::Split { id, .. } => *id,
        }
    }

    /// Split this pane in the given direction, returning the ID of the newly created pane
    pub fn split(&mut self, pane_id: PaneId, direction: SplitDirection, new_terminal: Arc<Mutex<Terminal>>) -> Option<PaneId> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == pane_id {
                    // Replace this leaf with a split node
                    let old_terminal = terminal.clone();
                    // Preserve the original pane's ID instead of creating a new one
                    let old_leaf = PaneNode::Leaf {
                        id: *id,
                        terminal: old_terminal,
                    };
                    let new_leaf = PaneNode::new_leaf(new_terminal);
                    let new_pane_id = new_leaf.id(); // Capture the new pane's ID before boxing

                    *self = PaneNode::Split {
                        id: PaneId::new(),
                        direction,
                        ratio: 0.5,
                        first: Box::new(old_leaf),
                        second: Box::new(new_leaf),
                    };
                    Some(new_pane_id)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                // Recursively search in children
                first
                    .split(pane_id, direction, new_terminal.clone())
                    .or_else(|| second.split(pane_id, direction, new_terminal))
            }
        }
    }

    /// Close a pane by ID, returning true if the entire tree should be removed
    pub fn close_pane(&mut self, pane_id: PaneId) -> CloseResult {
        match self {
            PaneNode::Leaf { id, .. } => {
                if *id == pane_id {
                    CloseResult::RemoveThis
                } else {
                    CloseResult::NotFound
                }
            }
            PaneNode::Split { first, second, .. } => {
                let first_result = first.close_pane(pane_id);
                match first_result {
                    CloseResult::RemoveThis => {
                        // Replace self with second child
                        *self = (**second).clone();
                        return CloseResult::Replaced;
                    }
                    CloseResult::Replaced => return CloseResult::Replaced,
                    CloseResult::NotFound => {}
                }

                let second_result = second.close_pane(pane_id);
                match second_result {
                    CloseResult::RemoveThis => {
                        // Replace self with first child
                        *self = (**first).clone();
                        return CloseResult::Replaced;
                    }
                    CloseResult::Replaced => return CloseResult::Replaced,
                    CloseResult::NotFound => {}
                }

                CloseResult::NotFound
            }
        }
    }

    /// Find a terminal by pane ID
    pub fn find_terminal(&self, pane_id: PaneId) -> Option<Arc<Mutex<Terminal>>> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == pane_id {
                    Some(terminal.clone())
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => first.find_terminal(pane_id).or_else(|| second.find_terminal(pane_id)),
        }
    }

    /// Collect all terminals in the tree
    pub fn collect_terminals(&self) -> Vec<Arc<Mutex<Terminal>>> {
        match self {
            PaneNode::Leaf { terminal, .. } => vec![terminal.clone()],
            PaneNode::Split { first, second, .. } => {
                let mut terminals = first.collect_terminals();
                terminals.extend(second.collect_terminals());
                terminals
            }
        }
    }

    /// Collect all leaf pane IDs
    pub fn collect_leaf_ids(&self) -> Vec<PaneId> {
        match self {
            PaneNode::Leaf { id, .. } => vec![*id],
            PaneNode::Split { first, second, .. } => {
                let mut ids = first.collect_leaf_ids();
                ids.extend(second.collect_leaf_ids());
                ids
            }
        }
    }

    /// Count the number of leaf panes
    pub fn count_leaf_panes(&self) -> usize {
        match self {
            PaneNode::Leaf { .. } => 1,
            PaneNode::Split { first, second, .. } => first.count_leaf_panes() + second.count_leaf_panes(),
        }
    }

    /// Collect terminals with their pane IDs
    pub fn collect_terminals_with_ids(&self) -> Vec<(PaneId, Arc<Mutex<Terminal>>)> {
        match self {
            PaneNode::Leaf { id, terminal } => vec![(*id, terminal.clone())],
            PaneNode::Split { first, second, .. } => {
                let mut pairs = first.collect_terminals_with_ids();
                pairs.extend(second.collect_terminals_with_ids());
                pairs
            }
        }
    }

    /// Update split ratio for a specific split node
    pub fn update_ratio(&mut self, pane_id: PaneId, new_ratio: f32) -> bool {
        match self {
            PaneNode::Leaf { .. } => false,
            PaneNode::Split { id, ratio, first, second, .. } => {
                if *id == pane_id {
                    *ratio = new_ratio.clamp(0.1, 0.9);
                    true
                } else {
                    first.update_ratio(pane_id, new_ratio) || second.update_ratio(pane_id, new_ratio)
                }
            }
        }
    }
}

pub enum CloseResult {
    NotFound,
    RemoveThis,
    Replaced,
}

/// Context menu image data (embedded at compile time)
#[derive(Clone)]
pub struct ContextMenuImages {
    pub vertical_split: &'static [u8],
    pub horizontal_split: &'static [u8],
    pub expand_into_tab: &'static [u8],
    pub kill_shell: &'static [u8],
}

impl ContextMenuImages {
    pub fn load() -> Self {
        Self {
            vertical_split: include_bytes!("../static/gfx/vertical-split.png"),
            horizontal_split: include_bytes!("../static/gfx/horizontal-split.png"),
            expand_into_tab: include_bytes!("../static/gfx/expand-into-tab.png"),
            kill_shell: include_bytes!("../static/gfx/kill-shell.png"),
        }
    }
}

/// Manages the pane layout for a single tab
pub struct PaneLayout {
    pub root: PaneNode,
    pub active_pane: PaneId,
    /// Track which split divider is being dragged (split node ID)
    pub dragging_divider: Option<PaneId>,
    /// Track preview ratio during dragging (split_id, preview_ratio)
    pub drag_preview: Option<(PaneId, f32)>,
    /// Keep clipboard context alive to maintain PRIMARY selection on Linux
    #[cfg(target_os = "linux")]
    pub primary_clipboard: Option<Clipboard>,
    /// Context menu images
    pub context_menu_images: Option<ContextMenuImages>,
    /// Context menu state: (pane_id, x, y)
    pub context_menu_open: Option<(PaneId, i32, i32)>,
    /// Context menu instance
    pub context_menu: Option<crate::ui::context_menu::ContextMenu<String>>,
    /// Pending context menu action: (pane_id, action_type)
    pub pending_context_action: Option<(PaneId, String)>,
    /// Copy animation (expanding and fading rectangle after Ctrl+Shift+C)
    pub copy_animation: Option<CopyAnimation>,
    /// Panes selected for group input (Ctrl+click to toggle)
    pub selected_panes: HashSet<PaneId>,
}

impl PaneLayout {
    /// Create a new layout with a single pane
    pub fn new(terminal: Arc<Mutex<Terminal>>) -> Self {
        let root = PaneNode::new_leaf(terminal);
        let active_pane = root.id();
        Self {
            root,
            active_pane,
            dragging_divider: None,
            drag_preview: None,
            #[cfg(target_os = "linux")]
            primary_clipboard: None,
            context_menu_images: None,
            context_menu_open: None,
            context_menu: None,
            pending_context_action: None,
            copy_animation: None,
            selected_panes: HashSet::new(),
        }
    }

    /// Get the currently active pane ID
    pub fn active_pane(&self) -> PaneId {
        self.active_pane
    }

    /// Set the active pane
    pub fn set_active_pane(&mut self, pane_id: PaneId) {
        // Verify the pane exists
        if self.root.find_terminal(pane_id).is_some() {
            self.active_pane = pane_id;
        }
    }

    /// Get the terminal for the active pane
    pub fn get_active_terminal(&self) -> Option<Arc<Mutex<Terminal>>> {
        self.root.find_terminal(self.active_pane)
    }

    /// Split the active pane in the given direction
    pub fn split_active_pane(&mut self, direction: SplitDirection, new_terminal: Arc<Mutex<Terminal>>) {
        let active_pane = self.active_pane;
        if let Some(new_pane_id) = self.root.split(active_pane, direction, new_terminal.clone()) {
            // Set the newly created pane as active
            self.active_pane = new_pane_id;
        }
    }

    /// Close a pane by ID
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        let result = self.root.close_pane(pane_id);
        match result {
            CloseResult::RemoveThis => {
                // This was the only pane, signal to close tab
                true
            }
            CloseResult::Replaced => {
                // Make sure active pane still exists
                if self.root.find_terminal(self.active_pane).is_none() {
                    // Set first available pane as active
                    if let Some(first_id) = self.root.collect_leaf_ids().first() {
                        self.active_pane = *first_id;
                    }
                }
                false
            }
            CloseResult::NotFound => false,
        }
    }

    /// Extract a pane and expand it into a new tab
    pub fn extract_pane(&mut self, pane_id: PaneId) -> Option<Arc<Mutex<Terminal>>> {
        let terminal = self.root.find_terminal(pane_id)?;
        self.close_pane(pane_id);
        Some(terminal)
    }

    /// Get all terminals in the layout
    pub fn get_all_terminals(&self) -> Vec<Arc<Mutex<Terminal>>> {
        self.root.collect_terminals()
    }

    /// Get all terminals with their pane IDs
    pub fn get_terminals_with_pane_ids(&self) -> Vec<(PaneId, Arc<Mutex<Terminal>>)> {
        self.root.collect_terminals_with_ids()
    }

    /// Cycle to the next pane in the layout
    pub fn cycle_to_next_pane(&mut self) {
        let pane_ids = self.root.collect_leaf_ids();
        if pane_ids.len() <= 1 {
            return; // Nothing to cycle
        }

        if let Some(current_idx) = pane_ids.iter().position(|&id| id == self.active_pane) {
            let next_idx = (current_idx + 1) % pane_ids.len();
            self.active_pane = pane_ids[next_idx];
        } else {
            // Current pane not found, set to first pane
            if let Some(&first_id) = pane_ids.first() {
                self.active_pane = first_id;
            }
        }
    }

    /// Cycle to the previous pane in the layout
    pub fn cycle_to_previous_pane(&mut self) {
        let pane_ids = self.root.collect_leaf_ids();
        if pane_ids.len() <= 1 {
            return; // Nothing to cycle
        }

        if let Some(current_idx) = pane_ids.iter().position(|&id| id == self.active_pane) {
            let prev_idx = if current_idx == 0 { pane_ids.len() - 1 } else { current_idx - 1 };
            self.active_pane = pane_ids[prev_idx];
        } else {
            // Current pane not found, set to first pane
            if let Some(&first_id) = pane_ids.first() {
                self.active_pane = first_id;
            }
        }
    }

    /// Check if this is the first pane in the layout
    pub fn is_first_pane(&self) -> bool {
        let pane_ids = self.root.collect_leaf_ids();
        pane_ids.first() == Some(&self.active_pane)
    }

    /// Check if this is the last pane in the layout
    pub fn is_last_pane(&self) -> bool {
        let pane_ids = self.root.collect_leaf_ids();
        pane_ids.last() == Some(&self.active_pane)
    }

    /// Get pane layout rectangles for rendering (SDL-compatible)
    /// Returns: Vec<(PaneId, Rect, Arc<Mutex<Terminal>>, is_active)>
    pub fn get_pane_rects(&self, x: i32, y: i32, width: u32, height: u32) -> Vec<(PaneId, Rect, Arc<Mutex<Terminal>>, bool, bool)> {
        let mut panes = Vec::new();
        self.collect_pane_rects(&self.root, x, y, width, height, &mut panes);
        panes
    }

    fn collect_pane_rects(&self, node: &PaneNode, x: i32, y: i32, width: u32, height: u32, panes: &mut Vec<(PaneId, Rect, Arc<Mutex<Terminal>>, bool, bool)>) {
        match node {
            PaneNode::Leaf { id, terminal } => {
                let is_active = *id == self.active_pane;
                let is_selected = self.selected_panes.contains(id);
                let rect = Rect::new(x, y, width, height);
                panes.push((*id, rect, terminal.clone(), is_active, is_selected));
            }
            PaneNode::Split {
                id,
                direction,
                ratio,
                first,
                second,
            } => {
                let split_id = *id;
                let divider_size = 2;

                // Use preview ratio if this divider is being dragged
                let effective_ratio = if let Some((preview_id, preview_ratio)) = self.drag_preview {
                    if preview_id == split_id {
                        preview_ratio
                    } else {
                        *ratio
                    }
                } else {
                    *ratio
                };

                match direction {
                    SplitDirection::Horizontal => {
                        let first_width = ((width as i32 - divider_size) as f32 * effective_ratio) as u32;
                        let second_width = width - first_width - divider_size as u32;

                        self.collect_pane_rects(first, x, y, first_width, height, panes);
                        self.collect_pane_rects(second, x + first_width as i32 + divider_size, y, second_width, height, panes);
                    }
                    SplitDirection::Vertical => {
                        let first_height = ((height as i32 - divider_size) as f32 * effective_ratio) as u32;
                        let second_height = height - first_height - divider_size as u32;

                        self.collect_pane_rects(first, x, y, width, first_height, panes);
                        self.collect_pane_rects(second, x, y + first_height as i32 + divider_size, width, second_height, panes);
                    }
                }
            }
        }
    }

    /// Get divider rectangles for rendering (SDL-compatible)
    /// Returns: Vec<(PaneId, Rect, SplitDirection)>
    pub fn get_divider_rects(&self, x: i32, y: i32, width: u32, height: u32) -> Vec<(PaneId, Rect, SplitDirection)> {
        let mut dividers = Vec::new();
        self.collect_divider_rects(&self.root, x, y, width, height, &mut dividers);
        dividers
    }

    fn collect_divider_rects(&self, node: &PaneNode, x: i32, y: i32, width: u32, height: u32, dividers: &mut Vec<(PaneId, Rect, SplitDirection)>) {
        match node {
            PaneNode::Leaf { .. } => {}
            PaneNode::Split {
                id,
                direction,
                ratio,
                first,
                second,
            } => {
                let split_id = *id;
                let divider_size = 2;

                // Use preview ratio if this divider is being dragged
                let effective_ratio = if let Some((preview_id, preview_ratio)) = self.drag_preview {
                    if preview_id == split_id {
                        preview_ratio
                    } else {
                        *ratio
                    }
                } else {
                    *ratio
                };

                match direction {
                    SplitDirection::Horizontal => {
                        let first_width = ((width as i32 - divider_size) as f32 * effective_ratio) as u32;
                        let second_width = width - first_width - divider_size as u32;

                        let divider_x = x + first_width as i32;
                        let divider_rect = Rect::new(divider_x, y, divider_size as u32, height);
                        dividers.push((split_id, divider_rect, *direction));

                        self.collect_divider_rects(first, x, y, first_width, height, dividers);
                        self.collect_divider_rects(second, x + first_width as i32 + divider_size, y, second_width, height, dividers);
                    }
                    SplitDirection::Vertical => {
                        let first_height = ((height as i32 - divider_size) as f32 * effective_ratio) as u32;
                        let second_height = height - first_height - divider_size as u32;

                        let divider_y = y + first_height as i32;
                        let divider_rect = Rect::new(x, divider_y, width, divider_size as u32);
                        dividers.push((split_id, divider_rect, *direction));

                        self.collect_divider_rects(first, x, y, width, first_height, dividers);
                        self.collect_divider_rects(second, x, y + first_height as i32 + divider_size, width, second_height, dividers);
                    }
                }
            }
        }
    }

    /// Handle mouse click on pane area (returns the clicked pane ID if any)
    pub fn handle_click(&mut self, mouse_x: i32, mouse_y: i32, area_x: i32, area_y: i32, area_width: u32, area_height: u32) -> Option<PaneId> {
        let panes = self.get_pane_rects(area_x, area_y, area_width, area_height);
        for (pane_id, pane_rect, _, _, _) in panes {
            if pane_rect.contains_point((mouse_x, mouse_y)) {
                self.set_active_pane(pane_id);
                return Some(pane_id);
            }
        }
        None
    }

    /// Start dragging a divider (returns true if a divider was grabbed)
    pub fn start_drag_divider(&mut self, mouse_x: i32, mouse_y: i32, area_x: i32, area_y: i32, area_width: u32, area_height: u32) -> bool {
        let dividers = self.get_divider_rects(area_x, area_y, area_width, area_height);
        for (split_id, rect, _direction) in dividers {
            // Expand hit area for easier dragging
            let hit_rect = Rect::new(rect.x() - 3, rect.y() - 3, rect.width() + 6, rect.height() + 6);
            if hit_rect.contains_point((mouse_x, mouse_y)) {
                self.dragging_divider = Some(split_id);
                // Get current ratio
                if let Some(ratio) = self.get_split_ratio(split_id) {
                    self.drag_preview = Some((split_id, ratio));
                }
                return true;
            }
        }
        false
    }

    /// Update divider drag
    pub fn update_drag_divider(&mut self, delta_x: i32, delta_y: i32, area_x: i32, area_y: i32, area_width: u32, area_height: u32) {
        if let Some(split_id) = self.dragging_divider {
            let dividers = self.get_divider_rects(area_x, area_y, area_width, area_height);
            for (div_id, _rect, direction) in dividers {
                if div_id == split_id {
                    let delta = match direction {
                        SplitDirection::Horizontal => delta_x,
                        SplitDirection::Vertical => delta_y,
                    };

                    let parent_size = match direction {
                        SplitDirection::Horizontal => area_width as f32,
                        SplitDirection::Vertical => area_height as f32,
                    };

                    let ratio_delta = delta as f32 / parent_size;

                    if let Some((preview_id, preview_ratio)) = &mut self.drag_preview {
                        if *preview_id == split_id {
                            *preview_ratio = (*preview_ratio + ratio_delta).clamp(0.1, 0.9);
                        }
                    }
                    break;
                }
            }
        }
    }

    /// Stop dragging divider and apply changes
    pub fn stop_drag_divider(&mut self) {
        if let Some((split_id, preview_ratio)) = self.drag_preview {
            self.root.update_ratio(split_id, preview_ratio);
        }
        self.dragging_divider = None;
        self.drag_preview = None;
    }

    /// Get the current ratio of a split node
    fn get_split_ratio(&self, split_id: PaneId) -> Option<f32> {
        Self::find_split_ratio(&self.root, split_id)
    }

    fn find_split_ratio(node: &PaneNode, split_id: PaneId) -> Option<f32> {
        match node {
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { id, ratio, first, second, .. } => {
                if *id == split_id {
                    Some(*ratio)
                } else {
                    Self::find_split_ratio(first, split_id).or_else(|| Self::find_split_ratio(second, split_id))
                }
            }
        }
    }

    /// Open context menu at the specified position for a pane
    pub fn open_context_menu(&mut self, pane_id: PaneId, x: i32, y: i32) {
        use crate::ui::context_menu::{ContextMenu, ContextMenuItem};

        self.context_menu_open = Some((pane_id, x, y));

        // Create the context menu with items
        if let Some(ref menu_images) = self.context_menu_images {
            let pane_count = self.root.count_leaf_panes();
            let items = vec![
                ContextMenuItem::new(menu_images.vertical_split, "Split vertically", "split_vertical".to_string()),
                ContextMenuItem::new(menu_images.horizontal_split, "Split horizontally", "split_horizontal".to_string()),
                ContextMenuItem::with_enabled(menu_images.expand_into_tab, "Turn into a tab", "to_tab".to_string(), pane_count > 1),
                ContextMenuItem::new(menu_images.kill_shell, "Kill terminal", "kill_shell".to_string()),
            ];
            self.context_menu = Some(ContextMenu::new(items, (x, y)));
        }

        eprintln!("[PANE_LAYOUT] Context menu opened for pane {:?} at ({}, {})", pane_id, x, y);
    }

    /// Handle a click on the context menu. Returns true if the click was handled.
    /// Sets pending_context_action if a menu item was clicked.
    pub fn handle_context_menu_click(&mut self, mouse_x: i32, mouse_y: i32) -> bool {
        if let Some((menu_pane_id, _, _)) = self.context_menu_open {
            if let Some(ref menu) = self.context_menu {
                if let Some(action) = menu.handle_click(mouse_x, mouse_y) {
                    self.pending_context_action = Some((menu_pane_id, action));
                }
            }

            // Close menu on any click
            self.context_menu_open = None;
            self.context_menu = None;
            return true;
        }

        false
    }

    /// Update the context menu hover state based on mouse position
    pub fn update_context_menu_hover(&mut self, mouse_x: i32, mouse_y: i32) {
        if let Some(ref mut menu) = self.context_menu {
            menu.update_hover(mouse_x, mouse_y);
        }
    }

    /// Toggle pane selection for group input (Ctrl+click)
    pub fn toggle_pane_selection(&mut self, pane_id: PaneId) {
        if self.selected_panes.contains(&pane_id) {
            // Always allow deselection
            self.selected_panes.remove(&pane_id);
        } else {
            // Only allow selection if there's more than one pane
            if self.root.count_leaf_panes() > 1 {
                self.selected_panes.insert(pane_id);
            }
        }
    }

    /// Get terminals for group input: either selected panes or just the active pane
    pub fn get_group_input_terminals(&self) -> Vec<Arc<Mutex<Terminal>>> {
        if self.selected_panes.is_empty() {
            // No panes selected - send to active pane only
            if let Some(terminal) = self.root.find_terminal(self.active_pane) {
                vec![terminal]
            } else {
                vec![]
            }
        } else {
            // Send to all selected panes
            self.selected_panes.iter().filter_map(|&pane_id| self.root.find_terminal(pane_id)).collect()
        }
    }
}
