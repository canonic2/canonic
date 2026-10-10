//! Verifies reusable components, gallery fixtures and real terminal restoration.

mod commands;
#[cfg(feature = "gallery")]
mod commands_terminal;
mod components;
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
