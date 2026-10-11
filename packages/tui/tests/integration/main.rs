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
mod picker;
#[cfg(feature = "gallery")]
mod picker_terminal;
mod presentation;
