//! Owned one-shot general-runtime transport for the reviewed SDK read profile.
mod output;
pub(crate) mod wire;

use crate::{
    enforcement::read_consumer::{self, ReadCall},
    review::{self, sdk_read::RegisteredReadConsumer},
};
use std::{
    io::{Read, Write},
    path::Path,
    process::ExitCode,
    time::{Duration, Instant},
};

pub(crate) enum Outcome {
    Read(Box<read_consumer::ReadOutcome>),
    Observe(Box<read_consumer::ObservationOutcome>),
}

struct PreparedResponse {
    held: Option<Outcome>,
    context: Option<(String, String)>,
    encoded: Vec<u8>,
    exit: u8,
}

struct Deadline(Instant);
impl Deadline {
    fn checkpoint(&self) -> Result<(), crate::policy::Error> {
        if Instant::now() >= self.0 {
            Err(crate::policy::Error::Interrupted)
        } else {
            Ok(())
        }
    }
}

fn dispatch(
    root: &Path,
    envelope: wire::Envelope,
    checkpoint: review::Checkpoint<'_>,
) -> Result<Outcome, review::Error> {
    checkpoint()?;
    let consumer = RegisteredReadConsumer::admit(root)?;
    checkpoint()?;
    match envelope.operation {
        wire::Operation::Evaluate(call) => read_consumer::evaluate(
            consumer,
            ReadCall {
                input: call.input,
                context: call.context,
                provenance: call.provenance,
            },
            call.retry_ref.as_deref(),
            checkpoint,
        )
        .map(|value| Outcome::Read(Box::new(value))),
        wire::Operation::Consume(call) => read_consumer::consume(
            consumer,
            ReadCall {
                input: call.input,
                context: call.context,
                provenance: call.provenance,
            },
            &call.request_id,
            &call.retry_ref,
            checkpoint,
        )
        .map(|value| Outcome::Read(Box::new(value))),
        wire::Operation::Observe(call) => read_consumer::observe(
            consumer,
            &ReadCall {
                input: call.input,
                context: call.context,
                provenance: call.provenance,
            },
            &call.delivery_id,
            call.request_id.as_deref(),
            call.observation,
            checkpoint,
        )
        .map(|value| Outcome::Observe(Box::new(value))),
        wire::Operation::Unsupported => {
            Err(review::Error::Invalid("unsupported runtime operation"))
        }
    }
}

/// The owned outcome remains alive until the final response has left stdout.
pub(crate) fn run(root: &Path, timeout_ms: u64) -> ExitCode {
    let deadline = Deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let mut bytes = Vec::new();
    let read = std::io::stdin()
        .lock()
        .take((wire::MAX_INPUT + 1) as u64)
        .read_to_end(&mut bytes);
    let input = read
        .map_err(|_| wire::Error::InvalidInput)
        .and_then(|_| wire::Envelope::parse(&bytes));
    let PreparedResponse {
        held,
        context,
        encoded,
        mut exit,
    } = prepare(root, input, &bytes, &deadline);
    let mut stdout = std::io::stdout().lock();
    let written = match held.as_ref() {
        // Final decision validation is short. A stalled pipe reader must never
        // retain the project authority writer while an output frame drains.
        Some(Outcome::Read(read)) if exit == 0 => {
            match read.finalize(&|| deadline.checkpoint(), || ()) {
                Ok(()) => write(&mut stdout, &encoded),
                Err(error) => {
                    exit = 2;
                    let (correlation, operation) =
                        context.as_ref().expect("validated response context");
                    write(
                        &mut stdout,
                        &output::failure(
                            Some(correlation),
                            Some(operation),
                            Some(read.identity().project_id()),
                            if deadline.checkpoint().is_err() {
                                "timeout"
                            } else {
                                output::code(&error)
                            },
                        ),
                    )
                }
            }
        }
        _ => write(&mut stdout, &encoded),
    };
    drop(held);
    if written.is_err() {
        ExitCode::from(2)
    } else {
        ExitCode::from(exit)
    }
}

fn prepare(
    root: &Path,
    input: Result<wire::Envelope, wire::Error>,
    bytes: &[u8],
    deadline: &Deadline,
) -> PreparedResponse {
    let mut held = None;
    let mut context = None;
    let (encoded, exit) = match input {
        Err(error) => (
            output::failure(
                None,
                None,
                None,
                match error {
                    wire::Error::InvalidInput => "invalid_input",
                    wire::Error::UnsupportedVersion => "unsupported_version",
                    wire::Error::UnsupportedOperation => "unsupported_operation",
                },
            ),
            2,
        ),
        Ok(envelope) => {
            let correlation = envelope.correlation_id.clone();
            let operation = envelope.name.clone();
            context = Some((correlation.clone(), operation.clone()));
            if matches!(envelope.operation, wire::Operation::Unsupported) {
                (
                    output::failure(
                        Some(&correlation),
                        Some(&operation),
                        None,
                        "unsupported_operation",
                    ),
                    2,
                )
            } else {
                match dispatch(root, envelope, &|| deadline.checkpoint()) {
                    Ok(outcome) => {
                        held = Some(outcome);
                        match output::encode(
                            held.as_ref().expect("owned outcome"),
                            &correlation,
                            &operation,
                            &crate::policy::fingerprint(bytes),
                            &|| deadline.checkpoint(),
                        ) {
                            Ok(encoded) => (encoded, 0),
                            Err(_) => (
                                output::failure(
                                    Some(&correlation),
                                    Some(&operation),
                                    None,
                                    if deadline.checkpoint().is_err() {
                                        "timeout"
                                    } else {
                                        "budget_exceeded"
                                    },
                                ),
                                2,
                            ),
                        }
                    }
                    Err(error) => (
                        output::failure(
                            Some(&correlation),
                            Some(&operation),
                            None,
                            if deadline.checkpoint().is_err() {
                                "timeout"
                            } else {
                                output::code(&error)
                            },
                        ),
                        2,
                    ),
                }
            }
        }
    };
    PreparedResponse {
        held,
        context,
        encoded,
        exit,
    }
}

fn write(stdout: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    stdout.write_all(bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
