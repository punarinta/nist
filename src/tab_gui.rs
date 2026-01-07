use crate::pane_layout::{ContextMenuImages, PaneLayout};
use crate::terminal::Terminal;
use std::sync::{Arc, Mutex};

/// Manages the state of a single tab
pub struct TabState {
    pub pane_layout: PaneLayout,
    pub name: String,
    pub is_editing: bool,
    pub temp_name: String,
}

impl TabState {
    pub fn new(terminal: Arc<Mutex<Terminal>>, name: String) -> Self {
        let pane_layout = PaneLayout::new(terminal);
        Self {
            pane_layout,
            name: name.clone(),
            is_editing: false,
            temp_name: name,
        }
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn start_editing(&mut self) {
        self.is_editing = true;
        self.temp_name = self.get_name();
    }

    pub fn finish_editing(&mut self, save: bool) {
        if save && !self.temp_name.trim().is_empty() {
            self.set_name(self.temp_name.clone());
        } else {
            self.temp_name = self.get_name();
        }
        self.is_editing = false;
    }
}

/// Manages the GUI state for all tabs
pub struct TabBarGui {
    pub tab_states: Vec<TabState>,
    pub active_tab: usize,
    pub context_menu_images: Option<ContextMenuImages>,
}

impl TabBarGui {
    pub fn new() -> Self {
        Self {
            tab_states: Vec::new(),
            active_tab: 0,
            context_menu_images: None,
        }
    }

    pub fn add_tab(&mut self, terminal: Arc<Mutex<Terminal>>, name: String) {
        let mut tab_state = TabState::new(terminal, name);
        // Set context menu images if available
        if let Some(ref images) = self.context_menu_images {
            tab_state.pane_layout.context_menu_images = Some(images.clone());
        }
        self.tab_states.push(tab_state);
        self.active_tab = self.tab_states.len() - 1;
    }

    pub fn set_context_menu_images(&mut self, images: ContextMenuImages) {
        self.context_menu_images = Some(images.clone());
        // Update all existing tabs
        for tab_state in &mut self.tab_states {
            tab_state.pane_layout.context_menu_images = Some(images.clone());
        }
    }

    pub fn remove_tab(&mut self, index: usize) -> bool {
        if index >= self.tab_states.len() {
            return false;
        }

        self.tab_states.remove(index);

        if self.tab_states.is_empty() {
            return true; // Signal to quit
        }

        if self.active_tab >= self.tab_states.len() {
            self.active_tab = self.tab_states.len() - 1;
        }

        false
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tab_states.len() {
            self.active_tab = index;
        }
    }

    pub fn cycle_to_next_tab(&mut self) {
        if self.tab_states.is_empty() {
            return;
        }
        self.active_tab = (self.active_tab + 1) % self.tab_states.len();
    }

    pub fn cycle_to_previous_tab(&mut self) {
        if self.tab_states.is_empty() {
            return;
        }
        if self.active_tab == 0 {
            self.active_tab = self.tab_states.len() - 1;
        } else {
            self.active_tab -= 1;
        }
    }

    pub fn reorder_tab(&mut self, from_index: usize, to_index: usize) {
        if from_index >= self.tab_states.len() || to_index >= self.tab_states.len() {
            return;
        }
        if from_index == to_index {
            return;
        }

        // Remove the tab from its current position
        let tab = self.tab_states.remove(from_index);

        // Insert it at the new position
        self.tab_states.insert(to_index, tab);

        // Update active_tab index if needed
        if self.active_tab == from_index {
            // The active tab was moved
            self.active_tab = to_index;
        } else if from_index < self.active_tab && to_index >= self.active_tab {
            // A tab before the active tab was moved to after it
            self.active_tab -= 1;
        } else if from_index > self.active_tab && to_index <= self.active_tab {
            // A tab after the active tab was moved to before it
            self.active_tab += 1;
        }
    }

    pub fn get_active_terminal(&self) -> Option<Arc<Mutex<Terminal>>> {
        self.tab_states.get(self.active_tab).and_then(|ts| ts.pane_layout.get_active_terminal())
    }

    pub fn get_active_pane_layout(&mut self) -> Option<&mut PaneLayout> {
        self.tab_states.get_mut(self.active_tab).map(|ts| &mut ts.pane_layout)
    }

    /// Get all terminals from all tabs (used for test server and management)
    pub fn get_all_terminals(&self) -> Vec<Arc<Mutex<Terminal>>> {
        self.tab_states.iter().flat_map(|ts| ts.pane_layout.get_all_terminals()).collect()
    }

    /// Get all terminals from the active tab only
    /// This is used for dirty flag checking to avoid checking inactive tabs,
    /// since inactive tabs are not rendered and don't need to trigger redraws
    pub fn get_active_tab_terminals(&self) -> Vec<Arc<Mutex<Terminal>>> {
        self.tab_states
            .get(self.active_tab)
            .map(|ts| ts.pane_layout.get_all_terminals())
            .unwrap_or_default()
    }

    pub fn get_tab_names(&self) -> Vec<String> {
        self.tab_states.iter().map(|ts| ts.get_name()).collect()
    }
}
