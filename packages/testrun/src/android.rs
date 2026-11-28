//! Android capability composition; transport, input, apps and hierarchy each own their role.
use std::path::PathBuf;

mod adb;
mod apps;
pub(crate) mod emulator;
mod focus;
mod hierarchy;
mod input;
pub(crate) mod lifecycle;
mod readiness;
mod recording;
pub(crate) mod streaming;
mod window;
pub use hierarchy::hierarchy;
pub(crate) use input::{key_codes, text_argument, text_timeout};
pub(crate) use recording::Recording as AndroidRecording;

pub struct Android {
    pub adb: PathBuf,
}
#[derive(Clone)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub name: String,
    pub transport: String,
    /// `usb`, `emulator` or `other` (wireless debugging), from `adb devices -l`.
    pub link: String,
}
impl Default for Android {
    fn default() -> Self {
        Self {
            adb: std::env::var_os("TESTRUN_ADB")
                .map_or_else(|| PathBuf::from("adb"), PathBuf::from),
        }
    }
}

#[cfg(test)]
mod tests;
