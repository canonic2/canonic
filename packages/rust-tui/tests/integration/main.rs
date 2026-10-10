//! Verifies reusable components, gallery fixtures and real terminal restoration.

mod components;
mod dialog;
#[cfg(feature = "gallery")]
mod dialog_terminal;
mod editor;
#[cfg(feature = "gallery")]
mod gallery;
mod picker;
#[cfg(feature = "gallery")]
mod picker_terminal;
mod presentation;
