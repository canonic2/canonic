//! Device actors and their supervisor. Each actor runs on its own thread with
//! a current-thread runtime, so blocking and native work never occupies the
//! manager's async workers. The supervisor frame, not the actor's task,
//! owns the device lease and the QA server holding native handles and
//! streams: a panic that unwinds the actor leaves them in place, and the
//! device is quarantined until recovery runs cleanup on the same thread.
use super::{Connection, owner, owner::Owner};
use crate::{
    error::{Error, Result},
    lease::ControlLease,
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Running,
    /// The actor panicked; its resources stay with the supervisor and no new
    /// work is admitted until a recovery retry finishes cleanup.
    Quarantined {
        cause: String,
    },
    /// Cleanup finished; the thread is waiting for started blocking jobs
    /// before it releases the device lease.
    Finishing,
}

enum Signal {
    Retry,
    Terminate,
}

struct Actor {
    target: String,
    generation: u64,
    state: State,
    signals: mpsc::Sender<Signal>,
    started: u64,
}

/// Selects the product controller or a Unix test controller with the same
/// records and endpoint.
#[derive(Clone, Copy, Default)]
pub(crate) enum Launcher {
    #[default]
    Product,
    #[cfg(all(test, unix))]
    Fixture,
}

pub(crate) struct Supervisor {
    actors: Mutex<HashMap<String, Actor>>,
    generations: Mutex<HashMap<String, u64>>,
    terminate: tokio::sync::watch::Sender<bool>,
    changed: tokio::sync::Notify,
    launcher: Launcher,
}

impl Supervisor {
    pub(crate) fn new(launcher: Launcher) -> Self {
        Self {
            actors: Mutex::default(),
            generations: Mutex::default(),
            terminate: tokio::sync::watch::channel(false).0,
            changed: tokio::sync::Notify::new(),
            launcher,
        }
    }
    fn actors(&self) -> std::sync::MutexGuard<'_, HashMap<String, Actor>> {
        self.actors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// Start the actor for `connection`, keyed by its target and a generation
    /// that increases with every actor of that target.
    pub(crate) fn start(self: &Arc<Self>, connection: Connection) -> Result<()> {
        let generation = {
            let mut generations = self
                .generations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let generation = generations.entry(connection.target.clone()).or_default();
            *generation += 1;
            *generation
        };
        let (signals, receiver) = mpsc::channel();
        self.actors().insert(
            connection.epoch.clone(),
            Actor {
                target: connection.target.clone(),
                generation,
                state: State::Running,
                signals,
                started: super::now(),
            },
        );
        let supervisor = self.clone();
        let epoch = connection.epoch.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("testrun-device-{generation}"))
            .spawn(move || supervisor.supervise(connection, &receiver));
        if spawned.is_err() {
            self.finished(&epoch);
            return Err(Error::new("HOST_IO", "Cannot start the device actor"));
        }
        Ok(())
    }
    /// The actor thread: lease, runtime and server live in this frame.
    fn supervise(self: Arc<Self>, connection: Connection, signals: &mpsc::Receiver<Signal>) {
        let epoch = connection.epoch.clone();
        let Some(lease) = acquire(&connection) else {
            self.finished(&epoch);
            return;
        };
        let _set =
            crate::simulator::device_set::DeviceSet::bind(connection.simulator_device_set.clone());
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => {
                let _ = owner::startup_failure(
                    &connection,
                    &Error::new("HOST_IO", "Cannot start the device actor runtime"),
                );
                drop(lease);
                self.finished(&epoch);
                return;
            }
        };
        let owner = Owner {
            connection,
            terminate: self.terminate.subscribe(),
        };
        let mut server = crate::qa::Server::new();
        let outcome = runtime
            .block_on(std::panic::AssertUnwindSafe(self.serve(&owner, &mut server)).catch_unwind());
        if let Err(panic) = outcome {
            self.quarantine(
                &owner,
                &runtime,
                &mut server,
                &panic_cause(&*panic),
                signals,
            );
        }
        self.set_state(&epoch, State::Finishing);
        {
            // Native handles release inside their runtime's context.
            let _context = runtime.enter();
            drop(server);
        }
        // Waits for blocking jobs already started: exclusion lasts until they finish.
        drop(runtime);
        drop(lease);
        self.finished(&epoch);
    }
    async fn serve(&self, owner: &Owner, server: &mut crate::qa::Server) {
        match self.launcher {
            Launcher::Product => {
                let _ = owner::run(owner, server).await;
            }
            #[cfg(all(test, unix))]
            Launcher::Fixture => {
                let _ = server;
                super::tests::fixture_actor(owner).await;
            }
        }
    }
    /// Keep the panicked actor's resources and wait for recovery; never
    /// restart it. A retry runs resumable cleanup under this same owner.
    fn quarantine(
        &self,
        owner: &Owner,
        runtime: &tokio::runtime::Runtime,
        server: &mut crate::qa::Server,
        cause: &str,
        signals: &mpsc::Receiver<Signal>,
    ) {
        let epoch = owner.epoch().to_owned();
        let error = Error::new(
            "ACTOR_QUARANTINED",
            "The device actor stopped unexpectedly; its resources are kept until recovery retries cleanup",
        )
        .details(json!({"cause":cause}));
        // Records first: once the quarantine is visible, admission refuses.
        let _ = owner.cleanup_progress(server.cleanup_status(), Some(&error), None);
        let _ = owner.state("failed", json!({"error":error}));
        self.set_state(
            &epoch,
            State::Quarantined {
                cause: cause.to_owned(),
            },
        );
        loop {
            let exit = match signals.recv() {
                Ok(Signal::Retry) => false,
                Ok(Signal::Terminate) | Err(_) => true,
            };
            let result = runtime.block_on(async {
                if exit {
                    server.shutdown().await
                } else {
                    server.shutdown_resumable().await
                }
            });
            let _ = owner.cleanup_progress(server.cleanup_status(), result.as_ref().err(), None);
            match result {
                Ok(()) => {
                    let _ = owner.state("closed", json!({}));
                    return;
                }
                Err(error) => {
                    let failure =
                        Error::new("CLEANUP_FAILED", "Quarantined device cleanup is unfinished")
                            .details(json!({"cause":error,"quarantine":cause}));
                    let _ = owner.state("failed", json!({"error":failure}));
                    if exit {
                        return;
                    }
                }
            }
        }
    }
    fn set_state(&self, epoch: &str, state: State) {
        if let Some(actor) = self.actors().get_mut(epoch) {
            actor.state = state;
        }
        self.changed.notify_waiters();
    }
    fn finished(&self, epoch: &str) {
        self.actors().remove(epoch);
        self.changed.notify_waiters();
    }
    /// Live actors, including quarantined and finishing ones: each is an
    /// obligation that keeps the manager running.
    pub(crate) fn alive(&self) -> usize {
        self.actors().len()
    }
    pub(crate) fn running(&self, epoch: &str) -> bool {
        self.actors().contains_key(epoch)
    }
    /// A quarantined actor on `target`, which admits no new work.
    pub(crate) fn quarantined(&self, target: &str) -> Option<String> {
        self.actors().values().find_map(|actor| match &actor.state {
            State::Quarantined { cause } if actor.target == target => Some(cause.clone()),
            _ => None,
        })
    }
    /// Ask a quarantined actor to retry cleanup. False when it is not quarantined.
    pub(crate) fn retry(&self, epoch: &str) -> bool {
        self.actors().get(epoch).is_some_and(|actor| {
            matches!(actor.state, State::Quarantined { .. })
                && actor.signals.send(Signal::Retry).is_ok()
        })
    }
    pub(crate) fn inventory(&self) -> Vec<Value> {
        let mut actors: Vec<Value> = self
            .actors()
            .iter()
            .map(|(epoch, actor)| {
                let (state, cause) = match &actor.state {
                    State::Running => ("running", None),
                    State::Quarantined { cause } => ("quarantined", Some(cause.clone())),
                    State::Finishing => ("finishing", None),
                };
                json!({"targetId":actor.target,"generation":actor.generation,"epoch":epoch,"state":state,"cause":cause,"started":actor.started})
            })
            .collect();
        actors.sort_by(|a, b| a["targetId"].as_str().cmp(&b["targetId"].as_str()));
        actors
    }
    /// The manager is exiting: running actors finish with exit cleanup
    /// and quarantined ones make one exit attempt.
    pub(crate) fn terminate(&self) {
        self.terminate.send_replace(true);
        for actor in self.actors().values() {
            let _ = actor.signals.send(Signal::Terminate);
        }
    }
    /// Wait until no actor remains.
    pub(crate) async fn finished_all(&self) {
        self.wait_until(|supervisor| supervisor.alive() == 0).await;
    }
    /// Wait until `condition` holds, rechecking whenever an actor changes.
    pub(crate) async fn wait_until(&self, condition: impl Fn(&Self) -> bool) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if condition(self) {
                return;
            }
            changed.await;
        }
    }
}

fn panic_cause(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|cause| (*cause).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "The device actor panicked".into())
}

/// Acquire the device lease for a new actor, recording a refused start.
fn acquire(connection: &Connection) -> Option<ControlLease> {
    if let Err(error) = connection.require_build() {
        let _ = owner::startup_failure(connection, &error);
        return None;
    }
    let mut lease = match ControlLease::acquire(&connection.authority()) {
        Ok(lease) => lease,
        Err(_) => {
            let _ = owner::startup_failure(
                connection,
                &Error::new(
                    "DEVICE_IN_USE",
                    "Another Testrun manager is using this device",
                )
                .details(json!({"blockingState":"external_owner"})),
            );
            return None;
        }
    };
    let published = lease.publish(json!({"epoch":connection.epoch,"task":connection.task,"targetId":connection.target,"endpoint":connection.socket(),"journalDirectory":connection.directory.join("s"),"version":super::WIRE}));
    if let Err(error) = published {
        let _ = owner::startup_failure(connection, &error);
        return None;
    }
    Some(lease)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::managed::{Caller, Kind, storage, tests};

    fn connection(root: &std::path::Path) -> Connection {
        let caller = Caller::new(None, root.join("state"), Kind::Cli);
        let directory = caller.directory().unwrap();
        let connection = caller
            .desired(
                &directory,
                &json!({"targetId":format!("android:Acme-{}", uuid::Uuid::new_v4().simple())}),
            )
            .unwrap();
        storage::directory(&connection.directory).unwrap();
        connection
    }

    async fn finished(supervisor: &Supervisor) {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            supervisor.finished_all(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_incompatible_build_is_refused_without_device_authority_or_cleanup_debt() {
        let root = tests::root();
        let mut connection = connection(root.path());
        connection.build_id = Some("Acme-other-build".into());
        let supervisor = Arc::new(Supervisor::new(Launcher::Fixture));
        supervisor.start(connection.clone()).unwrap();
        finished(&supervisor).await;
        assert!(!connection.held().unwrap());
        let state: Value = storage::read(&connection.directory.join("state.json"))
            .unwrap()
            .unwrap();
        assert_eq!(state["error"]["code"], "VERSION_MISMATCH");
        let cleanup: Value = storage::read(&connection.directory.join("cleanup.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            cleanup["complete"], true,
            "A refused start allocated nothing"
        );
    }

    #[tokio::test]
    async fn a_device_held_elsewhere_is_reported_and_its_holder_and_records_are_kept() {
        let root = tests::root();
        let connection = connection(root.path());
        let mut foreign = ControlLease::acquire(&connection.authority()).unwrap();
        foreign
            .publish(json!({"epoch":"Acme-foreign","task":"Acme-foreign-task"}))
            .unwrap();
        let retained = ControlLease::inspect(&connection.authority()).unwrap();
        let supervisor = Arc::new(Supervisor::new(Launcher::Fixture));
        supervisor.start(connection.clone()).unwrap();
        finished(&supervisor).await;
        let state: Value = storage::read(&connection.directory.join("state.json"))
            .unwrap()
            .unwrap();
        assert_eq!(state["error"]["code"], "DEVICE_IN_USE");
        assert_eq!(state["error"]["details"]["blockingState"], "external_owner");
        assert_eq!(
            ControlLease::inspect(&connection.authority()).unwrap(),
            retained
        );
        // A duplicate start of a live epoch preserves its state and progress.
        foreign
            .publish(json!({"epoch":connection.epoch,"task":connection.task}))
            .unwrap();
        let cleanup = json!({"complete":false,"errors":[{"code":"Acme-release-fault"}]});
        storage::write(&connection.directory.join("cleanup.json"), &cleanup).unwrap();
        let ready = json!({"state":"ready"});
        storage::write(&connection.directory.join("state.json"), &ready).unwrap();
        supervisor.start(connection.clone()).unwrap();
        finished(&supervisor).await;
        assert_eq!(
            storage::read::<Value>(&connection.directory.join("cleanup.json")).unwrap(),
            Some(cleanup)
        );
        assert_eq!(
            storage::read::<Value>(&connection.directory.join("state.json")).unwrap(),
            Some(ready)
        );
    }
}
