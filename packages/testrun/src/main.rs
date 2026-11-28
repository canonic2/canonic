//! Own the executable's Tokio runtime and return the library's CLI status.
use std::process::ExitCode;
fn main() -> ExitCode {
    if let Some(status) = testrun::diagnostic_parser_entry() {
        return status;
    }
    let input = match testrun::cli::parse() {
        Ok(input) => input,
        Err(status) => return status,
    };
    let stdio = input.is_mcp_server();
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    #[cfg(windows)]
    builder.thread_stack_size(16 * 1024 * 1024);
    let runtime = builder
        .enable_all()
        .build()
        .expect("Testrun runtime starts");
    let result = runtime.block_on(testrun::cli::run_to_exit(input));
    // Only the MCP client's stdin has a non-cancellable blocking read. Device
    // actors and other commands retain their normal runtime shutdown semantics.
    if stdio {
        runtime.shutdown_background();
    }
    result
}
