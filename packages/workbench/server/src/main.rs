//! CLI boundary for the Workbench native capabilities.
fn main() -> std::process::ExitCode {
    match workbench_server::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
