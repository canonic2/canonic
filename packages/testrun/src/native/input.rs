//! Worker-local retained HID dispatch; validation precedes every native mutation.
use super::bridge::Input;
use crate::{
    error::{Error, Result},
    simulator::{
        incarnation::Incarnation,
        input::protocol::{Action, Command},
        ipc,
    },
};
use serde_json::json;
use std::io::{BufRead, Read};

pub(crate) fn run(developer: &str, uuid: &str, identity: &str) -> Result<()> {
    let identity: Incarnation =
        serde_json::from_str(identity).map_err(|_| Error::invalid("Invalid simulator identity"))?;
    let input = super::startup((|| {
        identity.validate()?;
        let input = Input::open(developer, uuid)?;
        if input.incarnation(uuid)? != identity {
            return Err(Error::new(
                "STALE_TARGET",
                "Native input resolved a different incarnation",
            ));
        }
        Ok(input)
    })())?;
    let mut output = std::io::stdout().lock();
    ipc::write(&mut output, &json!({"event":"ready"}), &[])?;
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut bytes = Vec::new();
        let count = (&mut stdin)
            .take(4 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Input pipe failed"))?;
        if count == 0 {
            return Ok(());
        }
        if count > 4 * 1024 * 1024 {
            return Err(Error::invalid("Input request exceeds limit"));
        }
        let command: Command =
            serde_json::from_slice(&bytes).map_err(|_| Error::invalid("Invalid input request"))?;
        if matches!(command.input, Action::Close) {
            ipc::write(&mut output, &json!({"event":"reply","id":command.id}), &[])?;
            return Ok(());
        }
        let result = dispatch(&input, &identity, &command);
        ipc::write(
            &mut output,
            &json!({"event":"reply","id":command.id,"error":result.err()}),
            &[],
        )?;
    }
}
fn dispatch(input: &Input, identity: &Incarnation, command: &Command) -> Result<()> {
    command.input.validate()?;
    identity.validate()?;
    if !matches!(command.input, Action::Release { .. }) {
        let size = input.geometry()?;
        if command.geometry != Some([size.0, size.1]) {
            return Err(Error::new(
                "STALE_GEOMETRY",
                "Simulator display changed before input",
            ));
        }
    }
    // Reading geometry cannot authorize a later incarnation. Check again at admission.
    identity.validate()?;
    input
        .perform(&command.input)
        .and_then(|()| identity.validate())
        .map_err(|error| {
            Error::new(
                "OUTCOME_UNKNOWN",
                "Native input did not complete with current identity",
            )
            .details(json!({"cause":error.code}))
        })
}
