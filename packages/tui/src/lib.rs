//! Reusable terminal views with explicit state, actions and terminal ownership.
//!
//! Importing this library does not enter raw mode or start an event loop.
//! Products own operations and approvals; components only return UI actions.
//!
//! ```
//! use canonic_tui::picker::Picker;
//! let picker = Picker::new("Choose", vec!["Allow once".into(), "Cancel".into()], Some(1));
//! assert_eq!(picker.selected(), Some(1));
//! ```

pub mod chat;
pub mod commands;
pub mod completion;
pub mod composer;
pub mod dialog;
pub mod editor;
pub mod focus;
#[cfg(feature = "gallery")]
pub mod gallery;
pub mod inspection;
pub mod permission_review;
pub mod picker;
pub mod question_form;
pub mod review_queue;
mod review_ticket;
pub mod run_activity;
pub mod runtime;
pub mod session_browser;
pub mod status_panels;
mod text;
pub mod theme;
pub mod tool_call_card;
pub mod tree_browser;
pub mod ui;

// Keep event and rendering types consistent for local consumers.
pub use crossterm::event;
pub use ratatui;
