//! The Unix driver: one poll over both listeners and a coalesced wake socket.

use super::{Handler, Pool, Shared};
use crate::{Result, core::ExitPolicy, endpoint::Endpoint, wakeup::Wakeup};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::{
    os::fd::AsFd,
    time::{Duration, Instant},
};

/// Register the wake channel, declare readiness and serve until admission closes.
pub(super) fn run<H: Handler>(
    shared: &Shared<H>,
    policy: &impl ExitPolicy,
    lanes: [(&Endpoint<'_>, &Pool, bool); 2],
    start: Instant,
) -> Result<()> {
    let wakeup = Wakeup::new()?;
    shared.gate.set_waker(Some(wakeup.waker()));
    shared.gate.ready();
    drive(shared, policy, &wakeup, lanes, start)
}

fn drive<H: Handler>(
    shared: &Shared<H>,
    policy: &impl ExitPolicy,
    wakeup: &Wakeup,
    lanes: [(&Endpoint<'_>, &Pool, bool); 2],
    start: Instant,
) -> Result<()> {
    loop {
        // Any change racing the following observation leaves a readable signal.
        wakeup.clear()?;
        if shared.gate.tick(start.elapsed(), policy) {
            return Ok(());
        }
        let timeout = policy_timeout(shared.gate.next_check(start.elapsed(), policy));
        let mut descriptors = [
            PollFd::new(lanes[0].0.listener().as_fd(), PollFlags::POLLIN),
            PollFd::new(lanes[1].0.listener().as_fd(), PollFlags::POLLIN),
            PollFd::new(wakeup.as_fd(), PollFlags::POLLIN),
        ];
        match poll(&mut descriptors, timeout) {
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(error.into()),
        }
        for (endpoint, pool, control) in lanes {
            // One accept per lane preserves controls and lifetime progress.
            match endpoint.listener().accept() {
                Ok((stream, _)) => pool.enqueue(shared, stream, control),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

fn policy_timeout(delay: Option<Duration>) -> PollTimeout {
    delay.map_or(PollTimeout::NONE, |delay| {
        // Round up fractional milliseconds so a near deadline cannot busy-poll.
        let millis = i32::try_from(delay.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX);
        PollTimeout::try_from(millis).unwrap_or(PollTimeout::MAX)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_waits_round_up_and_explicit_waits_have_no_timer() {
        assert_eq!(policy_timeout(None), PollTimeout::NONE);
        assert_eq!(
            policy_timeout(Some(Duration::ZERO)),
            PollTimeout::from(0_u16)
        );
        assert_eq!(
            policy_timeout(Some(Duration::from_nanos(1))),
            PollTimeout::from(1_u16)
        );
        assert_eq!(
            policy_timeout(Some(Duration::from_secs(30))),
            PollTimeout::from(30_000_u16)
        );
        assert_eq!(policy_timeout(Some(Duration::MAX)), PollTimeout::MAX);
    }
}
