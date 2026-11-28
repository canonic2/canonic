//! The manager's device pool: every target connected for any client, with
//! when it was last used, the operations and media running on it, a viewer
//! holding its input and a release that failed and waits for its retry. One
//! pool per manager serves every client; nothing in it belongs to a client.
//! Pure state; the service applies its decisions to device actors and records.
use super::super::clock::Awake;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

/// How long a connected target stays connected without a use request from
/// any client, and how long a draining manager keeps an idle MCP connection.
pub(crate) const IDLE_RELEASE: Duration = Duration::from_secs(600);

/// The longest wait between automatic retries of a failed release.
const RETRY_CEILING: Duration = Duration::from_secs(30);

/// Which kind of client sent a request: status and history report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// A CLI command.
    Cli,
    /// An MCP server.
    Mcp,
}

/// Media running in the pool, counted against the manager's limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Media {
    Preview,
    Recording,
}
impl Media {
    /// The manager's bound for this kind: enough for the per-target allowance
    /// (two previews, one recording) on eight targets at once; each device
    /// actor enforces the per-target limit.
    fn limit(self) -> usize {
        match self {
            Self::Preview => 16,
            Self::Recording => 8,
        }
    }
}

pub(crate) struct Operation {
    pub(crate) kind: String,
    pub(crate) started: std::time::Instant,
    pub(crate) host: Option<String>,
    pub(crate) client: Kind,
    /// The target it runs on, when it has one.
    pub(crate) target: Option<String>,
}

/// A release that failed and is retried automatically.
#[derive(Clone)]
pub(crate) struct Release {
    pub(crate) attempts: u32,
    pub(crate) failed: Awake,
    /// The last attempt's error.
    pub(crate) error: Value,
    /// An attempt is running now.
    pub(crate) running: bool,
}
impl Release {
    /// How long after the last failure the next attempt runs: doubling from
    /// two seconds up to [`RETRY_CEILING`].
    fn delay(&self) -> Duration {
        let shift = self.attempts.saturating_sub(1).min(4);
        (Duration::from_secs(2) * (1 << shift)).min(RETRY_CEILING)
    }
    fn due(&self) -> bool {
        !self.running && self.failed.elapsed() >= self.delay()
    }
    fn summary(&self) -> Value {
        json!({
            "attempts":self.attempts,"running":self.running,"error":self.error,
            "nextAttemptMs":self.delay().saturating_sub(self.failed.elapsed()).as_millis(),
        })
    }
}

/// One target in the pool.
pub(crate) struct Held {
    pub(crate) last_use: Awake,
    /// Connected through `connect` (or a browser connect): it stays until a
    /// `disconnect` or idle release. A target only claimed for an app
    /// operation or device preparation is released when that call ends.
    pub(crate) connected: bool,
    /// App operations and preparations claiming it for their call.
    pub(crate) temporary: usize,
    /// A viewer holds its input.
    pub(crate) viewer: bool,
    pub(crate) release: Option<Release>,
}

pub(crate) struct Pool {
    pub(crate) id: String,
    pub(crate) generation: u64,
    pub(crate) directory: PathBuf,
    pub(crate) targets: BTreeMap<String, Held>,
    /// Targets a pooled device became after a recorded transition, such as a
    /// started AVD's emulator serial, mapped to the pooled target.
    pub(crate) aliases: BTreeMap<String, String>,
    /// The target of each preview and recording, so status and stop find it.
    pub(crate) media_targets: BTreeMap<String, String>,
    /// Previews and recordings by ID, including reservations being started.
    pub(crate) media: BTreeMap<String, Media>,
    /// Admitted use requests still running.
    pub(crate) calls: usize,
    pub(crate) operations: BTreeMap<String, Operation>,
    pub(crate) created: u64,
}

impl Pool {
    /// Whether `target` is in the pool, directly or as an alias.
    #[cfg(test)]
    pub(crate) fn holds(&self, target: &str) -> bool {
        self.targets.contains_key(target) || self.aliases.contains_key(target)
    }
    /// The pooled target `target` names: itself or the target it is an alias of.
    pub(crate) fn primary(&self, target: &str) -> Option<String> {
        if self.targets.contains_key(target) {
            Some(target.to_owned())
        } else {
            self.aliases.get(target).cloned()
        }
    }
    /// Connected device targets, without browsers or temporary claims.
    pub(crate) fn devices(&self) -> Vec<String> {
        self.connected()
            .filter(|target| !target.starts_with("browser:"))
            .collect()
    }
    /// Connected browser targets.
    pub(crate) fn browsers(&self) -> Vec<String> {
        self.connected()
            .filter(|target| target.starts_with("browser:"))
            .collect()
    }
    fn connected(&self) -> impl Iterator<Item = String> + '_ {
        self.targets
            .iter()
            .filter(|(_, held)| held.connected)
            .map(|(target, _)| target.clone())
    }
    /// Whether anything still runs on `target`: an operation, media or a viewer.
    fn busy(&self, target: &str) -> bool {
        self.operations
            .values()
            .any(|operation| operation.target.as_deref() == Some(target))
            || self
                .media_targets
                .iter()
                .any(|(media, owner)| owner == target && self.media.contains_key(media))
            || self.targets.get(target).is_some_and(|held| held.viewer)
    }
    pub(crate) fn summary(&self) -> Value {
        let targets: Vec<Value> = self
            .targets
            .iter()
            .map(|(target, held)| {
                json!({
                    "targetId":target,"connected":held.connected,"idleMs":held.last_use.elapsed().as_millis(),
                    "viewer":held.viewer,"release":held.release.as_ref().map(Release::summary),
                })
            })
            .collect();
        json!({
            "poolId":self.id,"generation":self.generation,"targets":targets,
            "operations":self.operations.iter().map(|(id, op)| json!({"operationId":id,"kind":op.kind,"ageMs":op.started.elapsed().as_millis(),"client":op.client,"hostLabel":op.host,"targetId":op.target})).collect::<Vec<_>>(),
            "created":self.created,
        })
    }
}

/// Inputs that create the pool when the manager first needs it.
pub(crate) struct Opening {
    pub(crate) directory: PathBuf,
    pub(crate) generation: u64,
}

#[derive(Default)]
pub(crate) struct Registry {
    pool: Option<Pool>,
    pub(crate) draining: bool,
    /// Admission is closed while the manager decides to exit.
    pub(crate) closing: bool,
}

fn refused(code: &str, message: &str) -> Error {
    Error::new(code, message).details(json!({"admitted":false,"dispatched":false}))
}

impl Registry {
    /// Nothing is pooled and nothing runs: the manager may exit.
    pub(crate) fn is_empty(&self) -> bool {
        self.pool.as_ref().is_none_or(|pool| {
            pool.targets.is_empty()
                && pool.operations.is_empty()
                && pool.calls == 0
                && pool.media.is_empty()
        })
    }
    pub(crate) fn pool(&self) -> Option<&Pool> {
        self.pool.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn pool_mut(&mut self) -> Option<&mut Pool> {
        self.pool.as_mut()
    }
    /// The pool, opened by `open` when the manager has none yet.
    pub(crate) fn open(&mut self, open: impl FnOnce() -> Result<Opening>) -> Result<String> {
        if let Some(pool) = &self.pool {
            return Ok(pool.id.clone());
        }
        if self.closing {
            return Err(self.exiting());
        }
        let opening = open()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.pool = Some(Pool {
            id: id.clone(),
            generation: opening.generation,
            directory: opening.directory,
            targets: BTreeMap::new(),
            aliases: BTreeMap::new(),
            media_targets: BTreeMap::new(),
            media: BTreeMap::new(),
            calls: 0,
            operations: BTreeMap::new(),
            created: super::super::now(),
        });
        Ok(id)
    }
    fn exiting(&self) -> Error {
        let mut error = refused("OWNER_EXITING", "The manager is exiting");
        error.retryable = true;
        error
    }
    pub(crate) fn draining_error(&self) -> Error {
        let mut error = refused(
            "OWNER_DRAINING",
            "The manager is draining for an upgrade and accepts no new connections; it retires after releasing its devices",
        );
        error.retryable = true;
        error.details(json!({"admitted":false,"dispatched":false,"pool":self.pool.as_ref().map(Pool::summary)}))
    }
    fn pool_mut_or_refused(&mut self) -> Result<&mut Pool> {
        self.pool
            .as_mut()
            .ok_or_else(|| refused("OWNER_EXITING", "The manager has no device pool"))
    }
    /// Admit one use request: it keeps the manager running until
    /// [`Registry::finish`], and renews `target`'s idle timeout.
    pub(crate) fn admit(&mut self, target: Option<&str>) -> Result<()> {
        let pool = self.pool_mut_or_refused()?;
        pool.calls += 1;
        if let Some(held) = target
            .and_then(|target| pool.primary(target))
            .and_then(|target| pool.targets.get_mut(&target))
        {
            held.last_use = Awake::now();
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self) {
        if let Some(pool) = &mut self.pool {
            pool.calls = pool.calls.saturating_sub(1);
        }
    }
    /// Connect `target` for every client. A draining manager connects no new
    /// target; a target already pooled stays as it is.
    pub(crate) fn claim(&mut self, target: &str) -> Result<()> {
        let draining = self.draining.then(|| self.draining_error());
        let pool = self.pool_mut_or_refused()?;
        if let Some(primary) = pool.primary(target) {
            let held = pool.targets.get_mut(&primary).expect("pooled target");
            held.connected = true;
            held.last_use = Awake::now();
            return Ok(());
        }
        if let Some(error) = draining {
            return Err(error);
        }
        pool.targets.insert(
            target.to_owned(),
            Held {
                last_use: Awake::now(),
                connected: true,
                temporary: 0,
                viewer: false,
                release: None,
            },
        );
        Ok(())
    }
    /// Claim `target` for one app operation or device preparation. Returns
    /// the pooled target it resolved to.
    pub(crate) fn claim_temporary(&mut self, target: &str) -> Result<String> {
        let pool = self.pool_mut_or_refused()?;
        let primary = pool.primary(target).unwrap_or_else(|| target.to_owned());
        let held = pool.targets.entry(primary.clone()).or_insert_with(|| Held {
            last_use: Awake::now(),
            connected: false,
            temporary: 0,
            viewer: false,
            release: None,
        });
        held.temporary += 1;
        held.last_use = Awake::now();
        Ok(primary)
    }
    /// One temporary claim ended. A target nobody connected in the meantime
    /// leaves the pool.
    pub(crate) fn settle_temporary(&mut self, target: &str) {
        let Some(pool) = &mut self.pool else {
            return;
        };
        let Some(held) = pool.targets.get_mut(target) else {
            return;
        };
        held.temporary = held.temporary.saturating_sub(1);
        if held.temporary == 0 && !held.connected && held.release.is_none() {
            pool.targets.remove(target);
        }
    }
    /// Record the target pooled device `from` became, such as a started
    /// AVD's emulator serial: both name the same pooled device.
    pub(crate) fn transition(&mut self, from: &str, to: &str) -> Result<()> {
        let pool = self.pool_mut_or_refused()?;
        let held = pool.primary(from).unwrap_or_else(|| from.to_owned());
        if held != to {
            pool.aliases.insert(to.to_owned(), held);
        }
        Ok(())
    }
    /// `target` and its aliases left the pool after a verified release.
    pub(crate) fn release_target(&mut self, target: &str) {
        if let Some(pool) = &mut self.pool {
            pool.targets.remove(target);
            pool.aliases.retain(|_, held| held != target);
        }
    }
    /// A viewer took or gave back `target`'s input.
    pub(crate) fn viewer(&mut self, target: &str, holding: bool) {
        if let Some(pool) = &mut self.pool
            && let Some(primary) = pool.primary(target)
            && let Some(held) = pool.targets.get_mut(&primary)
        {
            held.viewer = holding;
            held.last_use = Awake::now();
        }
    }
    /// A release attempt of `target` is starting; false when one already runs.
    pub(crate) fn release_started(&mut self, target: &str) -> bool {
        let Some(held) = self
            .pool
            .as_mut()
            .and_then(|pool| pool.targets.get_mut(target))
        else {
            return true;
        };
        match &mut held.release {
            Some(release) if release.running => false,
            Some(release) => {
                release.running = true;
                true
            }
            None => true,
        }
    }
    /// `target`'s release failed with `error`: it stays pooled and is retried.
    pub(crate) fn release_failed(&mut self, target: &str, error: Value) {
        if let Some(held) = self
            .pool
            .as_mut()
            .and_then(|pool| pool.targets.get_mut(target))
        {
            let attempts = held.release.as_ref().map_or(0, |release| release.attempts) + 1;
            held.release = Some(Release {
                attempts,
                failed: Awake::now(),
                error,
                running: false,
            });
        }
    }
    /// A later connect released `target` itself: its pending release is done.
    pub(crate) fn release_settled(&mut self, target: &str) {
        if let Some(held) = self
            .pool
            .as_mut()
            .and_then(|pool| pool.targets.get_mut(target))
        {
            held.release = None;
        }
    }
    /// The pending release of `target`, when one failed.
    pub(crate) fn pending_release(&self, target: &str) -> Option<Release> {
        let pool = self.pool.as_ref()?;
        pool.targets.get(&pool.primary(target)?)?.release.clone()
    }
    /// Targets whose failed release is due for its next attempt.
    pub(crate) fn due_releases(&self) -> Vec<String> {
        self.pool.as_ref().map_or_else(Vec::new, |pool| {
            pool.targets
                .iter()
                .filter(|(_, held)| held.release.as_ref().is_some_and(Release::due))
                .map(|(target, _)| target.clone())
                .collect()
        })
    }
    /// Connected devices idle past [`IDLE_RELEASE`] with nothing running on
    /// them: the manager releases them. Browsers close on disconnect or drain.
    #[cfg(test)]
    pub(crate) fn idle(&self) -> Vec<String> {
        self.idle_after(IDLE_RELEASE)
    }
    pub(crate) fn idle_after(&self, limit: Duration) -> Vec<String> {
        self.pool.as_ref().map_or_else(Vec::new, |pool| {
            pool.targets
                .iter()
                .filter(|(target, held)| {
                    held.connected
                        && (self.draining
                            || !target.starts_with(crate::browser::runtime::TARGET_PREFIX))
                        && held.temporary == 0
                        && held.release.is_none()
                        && held.last_use.elapsed() >= limit
                        && !pool.busy(target)
                })
                .map(|(target, _)| target.clone())
                .collect()
        })
    }
    /// Reserve a preview or recording that is starting, within the
    /// manager's limit.
    pub(crate) fn reserve_media(&mut self, kind: Media, reservation: &str) -> Result<()> {
        let pool = self.pool_mut_or_refused()?;
        let held = pool.media.values().filter(|held| **held == kind).count();
        if held >= kind.limit() {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                match kind {
                    Media::Preview => "The manager already runs its most previews; stop one first",
                    Media::Recording => {
                        "The manager already runs its most recordings; stop one first"
                    }
                },
            )
            .details(json!({"admitted":false,"dispatched":false,"limit":kind.limit()})));
        }
        pool.media.insert(reservation.to_owned(), kind);
        Ok(())
    }
    /// Replace a reservation with the started media's ID, or drop it, and
    /// remember which target the media belongs to.
    pub(crate) fn settle_media(
        &mut self,
        reservation: &str,
        started: Option<&str>,
        target: Option<&str>,
    ) {
        if let Some(pool) = &mut self.pool
            && let Some(kind) = pool.media.remove(reservation)
            && let Some(started) = started
        {
            pool.media.insert(started.to_owned(), kind);
            if let Some(target) = target {
                pool.media_targets
                    .insert(started.to_owned(), target.to_owned());
            }
        }
    }
    /// The media stopped or finished.
    pub(crate) fn release_media(&mut self, media: &str) {
        if let Some(pool) = &mut self.pool {
            pool.media.remove(media);
        }
    }
    pub(crate) fn begin_operation(
        &mut self,
        operation: &str,
        kind: &str,
        host: Option<&str>,
        client: Kind,
        target: Option<&str>,
    ) {
        if let Some(pool) = &mut self.pool {
            pool.operations.insert(
                operation.to_owned(),
                Operation {
                    kind: kind.to_owned(),
                    started: std::time::Instant::now(),
                    host: host.map(str::to_owned),
                    client,
                    target: target.map(str::to_owned),
                },
            );
        }
    }
    pub(crate) fn end_operation(&mut self, operation: &str) {
        if let Some(pool) = &mut self.pool {
            pool.operations.remove(operation);
        }
    }
    /// The manager is exiting with nothing pooled: forget the pool.
    pub(crate) fn close(&mut self) -> Option<Pool> {
        self.pool.take()
    }
}

#[cfg(test)]
mod tests;
