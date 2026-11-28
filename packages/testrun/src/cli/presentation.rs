//! Developer-facing settled output and explicit machine envelopes; no product operations.
mod document;
use crate::error::{Error, Result};
use canonic_tui_kit::output::{Block, Document};
use serde_json::{Value, json};
use std::io::{self, IsTerminal, Write};

pub fn print_error(error: &Error, machine: bool) -> io::Result<()> {
    if reported(error) {
        return Ok(());
    }
    write_error(
        &mut io::stderr().lock(),
        error,
        machine,
        width(io::stderr().is_terminal()),
    )
}

fn write_error(
    output: &mut impl Write,
    error: &Error,
    machine: bool,
    width: u16,
) -> io::Result<()> {
    if machine {
        return writeln!(output, "{}", json!({"error":error}));
    }
    let mut doc = Document(vec![Block::Heading(format!(
        "{}: {}",
        error.code, error.message
    ))]);
    if let Some(details) = &error.details {
        document::append(&mut doc, "Details", details);
    }
    output.write_all(doc.render(width).as_bytes())
}

pub(super) fn print_result(value: Value, machine: bool, title: &str) -> Result<()> {
    if !machine && io::stdout().is_terminal() {
        let text = document::result(title, &value)
            .render_with(width(true), canonic_tui_kit::runtime::profile::detect());
        io::stdout()
            .lock()
            .write_all(text.as_bytes())
            .map_err(output_error)?;
    } else {
        write_result(
            &mut io::stdout().lock(),
            &value,
            machine,
            title,
            width(io::stdout().is_terminal()),
        )
        .map_err(output_error)?;
    }
    if value["ok"] == false || value.get("isError").and_then(Value::as_bool) == Some(true) {
        Err(Error::new(
            "CLI_RESULT_FAILED",
            "The command reported a failed result",
        ))
    } else {
        if !machine && let Some(hint) = crate::pagination::human::hint(std::env::args(), &value) {
            writeln!(io::stdout().lock(), "{hint}").map_err(output_error)?;
        }
        Ok(())
    }
}

const KEYBOARD_NOTICE: &str = "Notice: typing attached a hardware keyboard, so iOS hides the on-screen keyboard until disconnect. Screens with a focused field now show no keyboard. To test with it visible, disconnect and connect again with --control-path xctest.";

/// Tells a human or agent reading the terminal, once per attachment, that
/// later screens are captured without the on-screen keyboard.
pub(super) fn print_keyboard_notice(value: &Value, machine: bool) -> Result<()> {
    if machine || !keyboard_just_attached(value) {
        return Ok(());
    }
    writeln!(io::stderr().lock(), "{KEYBOARD_NOTICE}").map_err(output_error)
}

fn keyboard_just_attached(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("hardwareKeyboardJustAttached") == Some(&Value::Bool(true))
                || map.values().any(keyboard_just_attached)
        }
        Value::Array(items) => items.iter().any(keyboard_just_attached),
        _ => false,
    }
}

fn write_result(
    output: &mut impl Write,
    value: &Value,
    machine: bool,
    title: &str,
    width: u16,
) -> io::Result<()> {
    if machine {
        writeln!(output, "{value}")
    } else {
        output.write_all(document::result(title, value).render(width).as_bytes())
    }
}

fn reported(error: &Error) -> bool {
    error.code == "CLI_RESULT_FAILED"
}
pub fn broken_pipe(error: &Error) -> bool {
    error.code == "CLI_OUTPUT_CLOSED"
}
pub(super) fn output_error(error: io::Error) -> Error {
    Error::new(
        if error.kind() == io::ErrorKind::BrokenPipe {
            "CLI_OUTPUT_CLOSED"
        } else {
            "HOST_IO"
        },
        "Cannot write CLI output",
    )
    .details(json!({"cause":error.to_string()}))
}
fn width(terminal: bool) -> u16 {
    if terminal {
        canonic_tui_kit::runtime::terminal_size().map_or(80, |(width, _)| width)
    } else {
        80
    }
}

/// Announce real work without acquiring input or claiming that interruption cancels it.
pub(super) async fn work<F>(title: &str, machine: bool, future: F) -> Result<Value>
where
    F: std::future::Future<Output = Result<Value>>,
{
    let visible = !machine && io::stderr().is_terminal();
    let started = std::time::Instant::now();
    if visible {
        writeln!(
            io::stderr().lock(),
            "Working: {}",
            canonic_tui_kit::output::sanitize(title)
        )
        .map_err(output_error)?;
    }
    let result = future.await;
    if visible {
        let outcome = match &result {
            Ok(value) if value["ok"] == false => "Failed",
            Ok(_) => "Finished",
            Err(error) if error.code == "SETUP_CANCELLED" => "Cancelled",
            Err(_) => "Failed",
        };
        let written = writeln!(
            io::stderr().lock(),
            "{outcome}: {} ({:.1}s)",
            canonic_tui_kit::output::sanitize(title),
            started.elapsed().as_secs_f64()
        );
        if result.is_ok() {
            written.map_err(output_error)?;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn machine_results_and_all_errors_preserve_complete_envelopes() {
        let value = json!({"targets":[{"targetId":"simulator:Acme"}],"unavailable":[]});
        let mut bytes = Vec::new();
        write_result(&mut bytes, &value, true, "Targets", 20).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), value);
        for code in [
            "SIGNING_PREFERENCE_INVALID",
            "HOST_IO",
            "RUNNER_BUILD_FAILED",
            "SETUP_CANCELLED",
        ] {
            bytes.clear();
            let error = Error::new(code, "Refused").details(json!({"cause":"terminal unavailable","manualAction":"Run setup explicitly","log":"/tmp/acme.log"}));
            write_error(&mut bytes, &error, true, 20).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                json!({"error":error})
            );
            bytes.clear();
            write_error(&mut bytes, &error, false, 80).unwrap();
            let human = String::from_utf8(bytes.clone()).unwrap();
            assert!(
                human.contains("terminal unavailable")
                    && human.contains("Run setup explicitly")
                    && human.contains("/tmp/acme.log")
            );
        }
    }
    #[test]
    fn keyboard_notice_follows_only_a_new_attachment_in_any_action_result() {
        let attached = json!({"actionOutcomes":[{"state":"completed"},{"result":{"hardwareKeyboardAttached":true,"hardwareKeyboardJustAttached":true}}]});
        assert!(keyboard_just_attached(&attached));
        let reused = json!({"actionOutcomes":[{"result":{"hardwareKeyboardAttached":true,"hardwareKeyboardJustAttached":false}}]});
        assert!(!keyboard_just_attached(&reused));
        assert!(!keyboard_just_attached(&json!({"state":"completed"})));
    }
    #[test]
    fn human_text_is_inert_and_write_failures_are_returned() {
        let value = json!({"name":"Acme\u{1b}]52;c;secret\u{7}","state":"partial","cleanup":{"nextAction":"retry"}});
        let mut bytes = Vec::new();
        write_result(&mut bytes, &value, false, "Status", 40).unwrap();
        let human = String::from_utf8(bytes).unwrap();
        assert!(!human.contains('\x1b') && !human.contains("secret"));
        assert!(human.contains("partial") && human.contains("retry"));
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = write_result(&mut Closed, &value, true, "Status", 80).unwrap_err();
        assert!(broken_pipe(&output_error(error)));
    }
}
