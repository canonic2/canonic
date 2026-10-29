//! Bounds retained/encoded shell evidence before accumulating repeated explanations.
use crate::policy::Error;
use serde::Serialize;
use std::io::{self, Write};

pub(super) const MAX_EVIDENCE_BYTES: usize = 4_194_304;

pub(super) fn measure<T: Serialize>(
    value: &T,
    maximum: usize,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<usize, Error> {
    let mut meter = Meter {
        bytes: 0,
        maximum,
        checkpoint,
        failure: None,
    };
    let result = serde_json::to_writer(&mut meter, value);
    if let Some(error) = meter.failure {
        return Err(error);
    }
    result.map_err(|_| Error::Contract("cannot encode shell evidence"))?;
    Ok(meter.bytes)
}

struct Meter<'a> {
    bytes: usize,
    maximum: usize,
    checkpoint: &'a dyn Fn() -> Result<(), Error>,
    failure: Option<Error>,
}

impl Write for Meter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Err(error) = (self.checkpoint)() {
            self.failure = Some(error);
            return Err(io::Error::other("shell evidence interrupted"));
        }
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            self.failure = Some(Error::Limit);
            return Err(io::Error::other("shell evidence limit"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
