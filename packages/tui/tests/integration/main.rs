//! Verifies reusable components, gallery fixtures and real terminal restoration.

mod chat;
#[cfg(feature = "gallery")]
mod chat_terminal;
mod commands;
#[cfg(feature = "gallery")]
mod commands_terminal;
mod completion;
#[cfg(feature = "gallery")]
mod completion_terminal;
mod components;
mod composer;
mod composer_host;
#[cfg(feature = "gallery")]
mod composer_terminal;
mod composer_workflows;
mod dialog;
mod dialog_design;
#[cfg(feature = "gallery")]
mod dialog_terminal;
mod editor;
#[cfg(feature = "gallery")]
mod gallery;
mod inspection;
#[cfg(feature = "gallery")]
mod inspection_gallery;
#[cfg(feature = "gallery")]
mod inspection_terminal;
#[cfg(feature = "gallery")]
mod permission_gallery;
mod permission_review;
mod picker;
#[cfg(feature = "gallery")]
mod picker_terminal;
mod presentation;
mod question_form;
#[cfg(feature = "gallery")]
mod review_gallery;
#[cfg(feature = "gallery")]
mod review_terminal;
mod run_activity;
mod session_browser;
#[cfg(feature = "gallery")]
mod session_gallery;
#[cfg(feature = "gallery")]
mod session_terminal;
mod tool_call_card;
#[cfg(feature = "gallery")]
mod tool_card_gallery;
mod tree_browser;
#[cfg(feature = "gallery")]
mod tree_gallery;
#[cfg(feature = "gallery")]
mod tree_terminal;
