//! CLI composition with separate argument, application and output roles.
mod application;
mod arguments;
mod browser;
mod builds;
mod diagnostics;
mod entry;
mod init;
mod presentation;
mod projects;
mod setup;

pub use application::{execute, run};
pub(crate) use arguments::find_query;
pub use arguments::{
    Action, AppCommand, AudioCommand, BrowserCommand, ClipboardCommand, CommandLine,
    ControlCommand, DeveloperModeCommand, DeveloperSupportCommand, ManagerCommand,
    ManagerOperationCommand, MediaCommand, Mobile, NetworkCommand, PageArgs, PreviewCommand,
    RecordingCommand, TargetCommand, UiCommand, VolumeCommand,
};
pub use entry::{parse, run_to_exit};

/// Resolves a `--config` argument against this invocation's directory; the
/// manager and MCP receive only absolute paths.
fn config_path(
    config: Option<std::path::PathBuf>,
) -> crate::error::Result<Option<std::path::PathBuf>> {
    config
        .map(|path| {
            std::path::absolute(&path)
                .map_err(|_| crate::error::Error::invalid("Cannot resolve the --config path"))
        })
        .transpose()
}
