//! Reads bounded newline-terminated frames on a dedicated thread.

use std::{
    io::{self, BufRead, Read},
    sync::mpsc::SyncSender,
    thread::Thread,
};

/// One reader event for the dispatcher.
pub(super) enum Input {
    /// A complete frame, including its line break.
    Frame(Vec<u8>),
    /// The input ended or failed.
    End(io::Result<()>),
    /// A frame exceeded the limit or the input ended mid-frame.
    Invalid,
}

/// Reads frames of at most `limit` bytes plus a line break until the input
/// ends, fails or sends an invalid frame, or the dispatcher stops listening.
pub(super) fn read(
    mut reader: impl BufRead,
    events: &SyncSender<Input>,
    dispatcher: &Thread,
    limit: usize,
) {
    let emit = |event| {
        let sent = events.send(event).is_ok();
        dispatcher.unpark();
        sent
    };
    let bound = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    loop {
        let mut frame = Vec::new();
        let count = match reader.by_ref().take(bound).read_until(b'\n', &mut frame) {
            Ok(count) => count,
            Err(error) => {
                emit(Input::End(Err(error)));
                return;
            }
        };
        if count == 0 {
            emit(Input::End(Ok(())));
            return;
        }
        if frame.last() != Some(&b'\n') || frame.len() - 1 > limit {
            emit(Input::Invalid);
            return;
        }
        if !emit(Input::Frame(frame)) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, sync::mpsc, thread};

    fn events(input: &[u8], limit: usize) -> Vec<Result<Vec<u8>, &'static str>> {
        let (sender, receiver) = mpsc::sync_channel(16);
        read(
            Cursor::new(input.to_vec()),
            &sender,
            &thread::current(),
            limit,
        );
        drop(sender);
        receiver
            .into_iter()
            .map(|event| match event {
                Input::Frame(frame) => Ok(frame),
                Input::End(Ok(())) => Err("end"),
                Input::End(Err(_)) => Err("error"),
                Input::Invalid => Err("invalid"),
            })
            .collect()
    }

    #[test]
    fn frames_end_at_line_breaks_and_respect_the_limit() {
        assert_eq!(
            events(b"{}\n[1]\n", 4),
            vec![Ok(b"{}\n".to_vec()), Ok(b"[1]\n".to_vec()), Err("end")]
        );
        assert_eq!(
            events(b"1234\n", 4),
            vec![Ok(b"1234\n".to_vec()), Err("end")]
        );
        assert_eq!(events(b"12345\n", 4), vec![Err("invalid")]);
        assert_eq!(
            events(b"{}\n{}", 4),
            vec![Ok(b"{}\n".to_vec()), Err("invalid")]
        );
        assert_eq!(events(b"", 4), vec![Err("end")]);
    }
}
