//! Input handling module for keyboard and mouse events
//!
//! This module organizes all input-related logic:
//! - `hotkeys`: Hotkey matching and action definitions
//! - `keyboard`: Keyboard event handling
//! - `mouse`: Mouse event handling
//! - `events`: SDL2 event dispatching

pub mod events;
pub mod hotkeys;
pub mod keyboard;
pub mod mouse;

// Re-export types and functions used by main.rs
