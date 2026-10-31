//! Runs Shield's CLI behavior and output checks as one integration binary.

mod diagnostics;
mod execution;
#[cfg(unix)]
mod http_environment;
mod mcp;
mod operator;
#[cfg(unix)]
mod operator_terminal;
mod output;
mod policy;
#[cfg(any(unix, windows))]
mod review;
#[cfg(any(unix, windows))]
mod runtime;
#[cfg(windows)]
mod windows;
mod workflows;
#[cfg(unix)]
mod working_files;
