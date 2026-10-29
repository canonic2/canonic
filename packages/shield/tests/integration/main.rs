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
#[cfg(unix)]
mod review;
#[cfg(unix)]
mod runtime;
mod workflows;
#[cfg(unix)]
mod working_files;
