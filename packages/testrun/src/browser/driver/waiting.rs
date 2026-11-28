//! Read-only process evidence while the same owned child remains held for retry.
#[cfg(unix)]
use super::{Diagnostics, Error};
use crate::browser::processes::Snapshot;
#[cfg(unix)]
use crate::browser::processes::Tracker;
#[cfg(unix)]
use std::sync::Mutex;
use std::{collections::VecDeque, time::Duration};
#[cfg(unix)]
use tokio::time::Instant;

const MAX_SAMPLES: usize = 32;

#[derive(Clone, Debug, Default)]
pub(super) struct Samples {
    values: VecDeque<serde_json::Value>,
    omitted: u64,
}
impl Samples {
    pub(super) fn record(&mut self, elapsed: u64, observed: std::io::Result<Snapshot>) {
        let complete = observed.is_ok();
        let observed = observed.ok();
        if self.values.len() == MAX_SAMPLES {
            self.values.pop_front();
            self.omitted = self.omitted.saturating_add(1);
        }
        self.values.push_back(serde_json::json!({"elapsedMs":elapsed,"driverAlive":observed.and_then(|snapshot|snapshot.driver_alive),"descendantsAlive":observed.map(|snapshot|snapshot.descendants_alive),"descendantsZombies":observed.map(|snapshot|snapshot.descendants_zombies),"descendantsAbsent":observed.map(|snapshot|snapshot.descendants_absent),"descendantsReused":observed.map(|snapshot|snapshot.descendants_reused),"inspectionSucceeded":complete}));
    }
    pub(super) fn snapshot(&self) -> serde_json::Value {
        let source = if cfg!(windows) {
            "windows-job-accounting"
        } else {
            "birth-fenced-process-witnesses"
        };
        serde_json::json!({"clock":"driver-session","source":source,"observationsOnly":true,"samples":self.values,"maxSamples":MAX_SAMPLES,"omittedSamples":self.omitted,"intervalMs":if cfg!(windows) { None } else { Some(1000) }})
    }
}
/// Records one sample and returns it for the caller's release decision.
#[cfg(unix)]
fn capture(root: u32, witnesses: &Tracker, diagnostics: &Mutex<Diagnostics>) -> Option<Snapshot> {
    let observed = witnesses.snapshot(Some(root));
    let snapshot = observed.as_ref().ok().copied();
    let mut diagnostics = diagnostics.lock().expect("Private driver evidence");
    let elapsed = diagnostics.lifecycle.elapsed_ms();
    diagnostics.process_wait.record(elapsed, observed);
    snapshot
}
/// Once every witnessed browser process is gone, the closed-pipe driver owns
/// nothing left to finalize; past this grace it is ended (owner decision).
pub(super) const RELEASED_DRIVER_GRACE: Duration = Duration::from_secs(2);

/// The witness table includes the driver itself; only its descendants decide.
/// Zombies count as gone: their processes have exited.
#[cfg(unix)]
fn descendants_released(observed: &Snapshot) -> bool {
    observed.driver_alive == Some(true)
        && observed.descendants_alive == 0
        && observed.descendants_absent + observed.descendants_zombies + observed.descendants_reused
            > 0
}

/// Waits for the driver after its pipe closed. The pinned Node driver can miss
/// its browser child's exit and idle until its own EOF fallback; after
/// [`RELEASED_DRIVER_GRACE`] with all witnessed descendants released, its owned
/// process group is signalled. A failed pre-close inspection (`inspected`
/// false) never allows it, and a fresh observation immediately before the
/// signal must still find no live descendant, including one started after the
/// last observation.
#[cfg(unix)]
pub(super) async fn exit(
    owned: &mut super::OwnedChild,
    witnesses: &mut Tracker,
    inspected: bool,
    diagnostics: &Mutex<Diagnostics>,
    deadline: Instant,
) -> Result<std::process::ExitStatus, Error> {
    let root = owned.group;
    capture(root, witnesses, diagnostics);
    let mut released_since: Option<Instant> = None;
    loop {
        if Instant::now() >= deadline {
            return Err(Error::CleanupPending);
        }
        // Child::wait is cancel safe: this branch never drops or replaces the
        // child when sampling or its deadline wins.
        let next = deadline.min(Instant::now() + Duration::from_secs(1));
        let settled = tokio::select! {
            result=owned.child.wait()=>Some(result),
            ()=tokio::time::sleep_until(next)=>None,
        };
        let observed = capture(root, witnesses, diagnostics);
        if let Some(status) = settled {
            return Ok(status?);
        }
        if inspected && observed.as_ref().is_some_and(descendants_released) {
            let since = *released_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= RELEASED_DRIVER_GRACE
                && witnesses.observe(root).is_ok()
                && witnesses
                    .snapshot(Some(root))
                    .is_ok_and(|fresh| descendants_released(&fresh))
            {
                owned.signal()?;
            }
        } else {
            released_since = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn driver_lifecycle_process_samples_bound_loss_and_preserve_unknown_inspections() {
        let mut samples = Samples::default();
        for elapsed in 0..MAX_SAMPLES + 1 {
            samples.record(
                elapsed as u64,
                Ok(Snapshot {
                    driver_alive: Some(true),
                    descendants_alive: 0,
                    descendants_zombies: 1,
                    descendants_absent: 2,
                    descendants_reused: 0,
                }),
            );
        }
        samples.record(
            100,
            Err(std::io::Error::other("Acme-private-inspection-error")),
        );
        let report = samples.snapshot();
        assert_eq!(report["samples"].as_array().unwrap().len(), MAX_SAMPLES);
        assert_eq!(report["omittedSamples"], 2);
        assert_eq!(report["samples"][0]["descendantsZombies"], 1);
        assert_eq!(report["samples"][0]["descendantsAbsent"], 2);
        let unknown = report["samples"].as_array().unwrap().last().unwrap();
        assert_eq!(unknown["inspectionSucceeded"], false);
        assert!(unknown["driverAlive"].is_null());
        assert!(unknown["descendantsAlive"].is_null());
        assert!(unknown["descendantsZombies"].is_null());
        assert!(unknown["descendantsAbsent"].is_null());
        assert!(!report.to_string().contains("Acme"));
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn driver_lifecycle_process_wait_keeps_child_for_retry_and_observes_natural_exit() {
        let mut command = tokio::process::Command::new("/bin/cat");
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut owned = super::super::OwnedChild::spawn(&mut command).unwrap();
        let input = owned.child.stdin.take().unwrap();
        let root = owned.child.id().unwrap();
        let mut witnesses = Tracker::default();
        witnesses.observe(root).unwrap();
        let diagnostics = Mutex::new(Diagnostics::default());
        assert!(matches!(
            exit(
                &mut owned,
                &mut witnesses,
                true,
                &diagnostics,
                Instant::now()
            )
            .await,
            Err(Error::CleanupPending)
        ));
        assert_eq!(owned.child.id(), Some(root));
        assert_eq!(
            diagnostics.lock().unwrap().process_wait.snapshot()["samples"][0]["driverAlive"],
            true
        );
        let mut waiting = Box::pin(exit(
            &mut owned,
            &mut witnesses,
            true,
            &diagnostics,
            Instant::now() + Duration::from_secs(5),
        ));
        assert!(futures_util::poll!(waiting.as_mut()).is_pending());
        drop(waiting);
        // The owned stdin keeps the helper alive until this explicit EOF.
        drop(input);
        assert!(
            exit(
                &mut owned,
                &mut witnesses,
                true,
                &diagnostics,
                Instant::now() + Duration::from_secs(5)
            )
            .await
            .unwrap()
            .success()
        );
        assert!(
            !owned.forced,
            "A driver without witnessed descendants is never ended"
        );
        assert!(owned.child.id().is_none());
        let report = diagnostics.lock().unwrap().process_wait.snapshot();
        let final_sample = report["samples"].as_array().unwrap().last().unwrap();
        assert_eq!(final_sample["driverAlive"], false);
        assert_eq!(final_sample["descendantsAlive"], 0);
        assert_eq!(final_sample["inspectionSucceeded"], true);
        assert!(witnesses.released().unwrap());
    }
    /// Stand-in for the pinned driver missing its browser child's exit: a
    /// short-lived descendant, then `rest`, then an idle leader holding its
    /// open stdin. Returns once the first descendant has been witnessed.
    #[cfg(target_os = "macos")]
    async fn lingering(
        rest: &str,
    ) -> (
        super::super::OwnedChild,
        tokio::process::ChildStdin,
        Tracker,
    ) {
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("/bin/sleep 0.5; {rest} exec /bin/cat"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut owned = super::super::OwnedChild::spawn(&mut command).unwrap();
        let input = owned.child.stdin.take().unwrap();
        let root = owned.child.id().unwrap();
        let mut witnesses = Tracker::default();
        let observed = Instant::now() + Duration::from_secs(2);
        while witnesses.witnessed() < 2 {
            assert!(
                Instant::now() < observed,
                "The descendant must be witnessed"
            );
            witnesses.observe(root).unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (owned, input, witnesses)
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn driver_lifecycle_released_descendants_end_a_lingering_driver_after_grace() {
        let (mut owned, _input, mut witnesses) = lingering("").await;
        let diagnostics = Mutex::new(Diagnostics::default());
        let started = Instant::now();
        let status = exit(
            &mut owned,
            &mut witnesses,
            true,
            &diagnostics,
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert!(!status.success());
        assert!(owned.forced, "The idle driver is ended, not waited out");
        assert!(started.elapsed() >= RELEASED_DRIVER_GRACE);
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(owned.child.id().is_none());
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn driver_lifecycle_unobserved_live_child_or_failed_inspection_keeps_the_driver() {
        // A descendant started after the last observation is found by the
        // fresh observation before the signal and keeps the driver alive.
        let (mut owned, _input, mut witnesses) = lingering("/bin/sleep 30 &").await;
        let diagnostics = Mutex::new(Diagnostics::default());
        let deadline = Instant::now() + RELEASED_DRIVER_GRACE + Duration::from_secs(3);
        assert!(matches!(
            exit(&mut owned, &mut witnesses, true, &diagnostics, deadline).await,
            Err(Error::CleanupPending)
        ));
        assert!(
            !owned.forced,
            "A live unobserved child forbids ending the driver"
        );
        assert!(
            !witnesses.released().unwrap(),
            "The late child is now witnessed"
        );
        owned.signal().unwrap();
        owned.child.wait().await.unwrap();
        // A failed pre-close inspection never allows ending the driver.
        let (mut owned, _input, mut witnesses) = lingering("").await;
        let deadline = Instant::now() + RELEASED_DRIVER_GRACE + Duration::from_secs(3);
        assert!(matches!(
            exit(&mut owned, &mut witnesses, false, &diagnostics, deadline).await,
            Err(Error::CleanupPending)
        ));
        assert!(!owned.forced, "Unknown release never ends the driver");
        owned.signal().unwrap();
        owned.child.wait().await.unwrap();
    }
}
