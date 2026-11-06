//! Safe platform clocks for reusable grants; discontinuous clocks fail closed.
use super::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stamp {
    utc: u64,
    ticks: u64,
}

impl Stamp {
    pub(super) fn capture() -> Result<Self, Error> {
        Ok(Self {
            utc: super::super::clock::now_ms()?,
            ticks: ticks()?,
        })
    }
    pub(super) fn valid(&self, lifetime: u64) -> Result<bool, Error> {
        let now = Self::capture()?;
        let wall = now.utc.checked_sub(self.utc).ok_or(Error::Invalid(
            "authorization_unavailable: clock moved backwards",
        ))?;
        let elapsed = now.ticks.checked_sub(self.ticks).ok_or(Error::Invalid(
            "authorization_unavailable: system clock restarted",
        ))?;
        if wall.abs_diff(elapsed) > 1000 {
            return Err(Error::Invalid(
                "authorization_unavailable: clock discontinuity; grant use is suspended",
            ));
        }
        Ok(wall < lifetime && elapsed < lifetime)
    }
}

#[cfg(unix)]
fn ticks() -> Result<u64, Error> {
    #[cfg(target_os = "linux")]
    let id = rustix::time::ClockId::Boottime;
    #[cfg(target_os = "macos")]
    let id = rustix::time::ClockId::Monotonic;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let id = rustix::time::ClockId::Monotonic;
    let time = rustix::time::clock_gettime(id);
    u64::try_from(time.tv_sec)
        .ok()
        .and_then(|s| s.checked_mul(1000))
        .and_then(|s| s.checked_add(u64::try_from(time.tv_nsec).ok()? / 1_000_000))
        .ok_or(Error::Invalid(
            "authorization_unavailable: monotonic clock unavailable",
        ))
}
#[cfg(windows)]
fn ticks() -> Result<u64, Error> {
    Ok(winsafe::GetTickCount64())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_grants_clock_discontinuity_suspends_without_terminalizing() {
        let (_root, context, reference) = super::super::tests::fixture();
        let proposed = super::super::propose(&context, &serde_json::json!({
            "session_ref":reference,"idempotency_key":"clock",
            "scope":[{"conditions":{"domain":"argv","executable":"acme",
                "arguments":{"kind":"exact","values":["merge"]},"cwd":{"exact":{"root":"project"}}}}]
        })).unwrap();
        super::super::apply(&context, &proposed["apply_arguments"], true).unwrap();
        let mut state = super::super::store(&context)
            .unwrap()
            .read_checkpoint()
            .unwrap();
        let id = proposed["proposal_id"].as_str().unwrap();
        let clock = state.session_grants.records[id]
            .grant
            .as_ref()
            .unwrap()
            .clock
            .clone();
        state
            .session_grants
            .records
            .get_mut(id)
            .unwrap()
            .grant
            .as_mut()
            .unwrap()
            .clock
            .utc += 10_000;
        let now = super::super::super::clock::now_ms().unwrap();
        assert_eq!(
            state.session_grants.records[id].effective(&state, now),
            "suspended"
        );
        super::super::compact(&mut state, now);
        assert!(state.session_grants.records[id].terminal.is_none());
        assert!(
            state
                .session_grants
                .sessions
                .values()
                .any(|ids| ids.iter().any(|v| v == id))
        );
        state
            .session_grants
            .records
            .get_mut(id)
            .unwrap()
            .grant
            .as_mut()
            .unwrap()
            .clock = clock;
        assert_eq!(
            state.session_grants.records[id].effective(&state, now),
            "active"
        );
    }

    #[test]
    fn session_grants_clock_discontinuities_never_extend_permission() {
        let now = Stamp::capture().unwrap();
        assert!(now.valid(60_000).unwrap());
        assert!(!now.valid(0).unwrap());
        let future = Stamp {
            utc: now.utc + 10_000,
            ticks: now.ticks,
        };
        assert!(future.valid(60_000).is_err());
        let restarted = Stamp {
            utc: now.utc,
            ticks: now.ticks + 10_000,
        };
        assert!(restarted.valid(60_000).is_err());
        let jumped = Stamp {
            utc: now.utc.saturating_sub(10_000),
            ticks: now.ticks,
        };
        assert!(jumped.valid(60_000).is_err());
    }
}
