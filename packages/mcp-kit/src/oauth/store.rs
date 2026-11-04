//! The storage contract for issued grants, and the in-memory store.
//!
//! Records hold digests of every secret value, never the values themselves,
//! so a copy of the store cannot be replayed. Each [`GrantStore`] operation is
//! atomic; durable backends belong to the consumer.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

/// The default number of each record kind [`MemoryStore`] holds.
pub const DEFAULT_MEMORY_CAPACITY: usize = 4096;
/// The most records of each kind [`MemoryStore`] may hold.
pub const MAX_MEMORY_CAPACITY: usize = 1_048_576;

/// A SHA-256 digest of a secret value with its type prefix.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// The digest as lowercase hexadecimal, for a durable store's key.
    #[must_use]
    pub fn to_hex(&self) -> String {
        use std::fmt::Write;
        self.0
            .iter()
            .fold(String::with_capacity(64), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            })
    }

    /// Compares in constant time.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0)
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
    }
}

impl std::fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Digest({})", self.to_hex())
    }
}

/// A validated authorization request awaiting sign-in and consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRecord {
    /// The non-secret reference that names it.
    pub reference: String,
    /// The digest of the browser binding held in the cookie.
    pub binding: Digest,
    /// The client.
    pub client_id: String,
    /// The client's display name, when it has one.
    pub client_name: Option<String>,
    /// The validated redirect URI.
    pub redirect_uri: String,
    /// The S256 code challenge.
    pub challenge: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
    /// The canonical resource.
    pub resource: String,
    /// The client's `state`, returned unchanged.
    pub state: Option<String>,
    /// When it expires.
    pub expires_at: SystemTime,
}

/// An authorization code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRecord {
    /// The code's digest.
    pub digest: Digest,
    /// The grant family the code creates, revoked if the code is redeemed twice.
    pub family_id: String,
    /// The client.
    pub client_id: String,
    /// The redirect URI the code was issued to.
    pub redirect_uri: String,
    /// The S256 code challenge.
    pub challenge: String,
    /// The approved scopes.
    pub scopes: Vec<String>,
    /// The canonical resource.
    pub resource: String,
    /// The resource owner.
    pub subject: String,
    /// When it expires.
    pub expires_at: SystemTime,
    /// Whether it was redeemed.
    pub consumed: bool,
}

/// One sign-in's lineage of refresh tokens and its live access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyRecord {
    /// The random family ID, part of each refresh token.
    pub id: String,
    /// The resource owner.
    pub subject: String,
    /// The client.
    pub client_id: String,
    /// The approved scopes.
    pub scopes: Vec<String>,
    /// The canonical resource; every token of the family keeps it.
    pub resource: String,
    /// When the family was created.
    pub created_at: SystemTime,
    /// When the family ends, whatever its tokens say.
    pub expires_at: SystemTime,
    /// The digest of the family secret every refresh token of the family
    /// carries; only a token with it can report reuse.
    pub proof: Digest,
    /// The current refresh generation.
    pub generation: u64,
    /// The current refresh token's digest.
    pub refresh: Digest,
    /// When the current refresh token expires.
    pub refresh_expires_at: SystemTime,
    /// The previous generation's refresh digest and when it was replaced.
    pub previous: Option<(Digest, SystemTime)>,
    /// The family's one live access token.
    pub access: Digest,
}

/// An access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRecord {
    /// The token's digest.
    pub digest: Digest,
    /// The family it belongs to.
    pub family_id: String,
    /// The scopes it carries, at most the family's.
    pub scopes: Vec<String>,
    /// When it expires.
    pub expires_at: SystemTime,
}

/// The result of redeeming a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeTake {
    /// The code, now marked consumed.
    Taken(CodeRecord),
    /// The code was already consumed; this is its family.
    Reused(String),
    /// No unexpired code has this digest.
    Unknown,
}

/// A refresh token rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotation {
    /// The family named by the presented token.
    pub family_id: String,
    /// The generation named by the presented token.
    pub generation: u64,
    /// The family generation the new tokens were prepared from. A rotation
    /// whose family has moved on since is stale and changes nothing.
    pub expected: u64,
    /// The presented token's digest.
    pub presented: Digest,
    /// The digest of the family secret the presented token carries.
    pub proof: Digest,
    /// The next refresh token's digest.
    pub refresh: Digest,
    /// When the next refresh token expires.
    pub refresh_expires_at: SystemTime,
    /// The new access token, replacing the family's previous one.
    pub access: AccessRecord,
    /// How long the previous generation may still rotate.
    pub grace: Duration,
}

/// The result of a rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rotated {
    /// The family moved to the next generation.
    Rotated(Box<FamilyRecord>),
    /// An older generation of a live family was presented.
    Reused,
    /// A concurrent rotation moved the family past the expected generation;
    /// nothing changed.
    Stale,
    /// No live family matches.
    Unknown,
}

/// Why a store operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Every record of this kind is live and the store is full.
    #[error("the grant store is full")]
    Full,
    /// The backend cannot serve the operation now.
    #[error("the grant store is unavailable: {0}")]
    Unavailable(String),
}

/// Atomic storage for pending authorizations, codes, families and access
/// tokens. Every method is one atomic operation.
pub trait GrantStore: Send + Sync {
    /// Inserts a pending authorization.
    ///
    /// # Errors
    /// Returns [`StoreError`] when it cannot be stored.
    fn put_pending(&self, record: PendingRecord) -> Result<(), StoreError>;

    /// Returns an unexpired pending authorization without consuming it.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn get_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError>;

    /// Removes and returns an unexpired pending authorization.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn take_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError>;

    /// Inserts an authorization code.
    ///
    /// # Errors
    /// Returns [`StoreError`] when it cannot be stored.
    fn put_code(&self, record: CodeRecord) -> Result<(), StoreError>;

    /// Marks a code consumed and returns it, or reports reuse.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn take_code(&self, digest: &Digest, now: SystemTime) -> Result<CodeTake, StoreError>;

    /// Inserts a family with its first access token, atomically with a check
    /// that the consumed code `code` that produced it is still stored for the
    /// same family. Returns `false`, inserting nothing, when the code is gone:
    /// a replay revoked the family before it existed.
    ///
    /// # Errors
    /// Returns [`StoreError`] when it cannot be stored.
    fn create_family(
        &self,
        code: &Digest,
        family: FamilyRecord,
        access: AccessRecord,
    ) -> Result<bool, StoreError>;

    /// Returns an unexpired family.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn get_family(&self, id: &str, now: SystemTime) -> Result<Option<FamilyRecord>, StoreError>;

    /// Rotates a family's refresh token: unless the family has moved past
    /// [`Rotation::expected`], which is stale, the current generation, or the
    /// previous one within its grace, advances; an older one carrying the
    /// family's proof is reuse.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn rotate(&self, rotation: Rotation, now: SystemTime) -> Result<Rotated, StoreError>;

    /// Returns an unexpired access token and its unexpired family.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn lookup_access(
        &self,
        digest: &Digest,
        now: SystemTime,
    ) -> Result<Option<(AccessRecord, FamilyRecord)>, StoreError>;

    /// Removes a family, its access token and its codes.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn revoke_family(&self, id: &str) -> Result<(), StoreError>;

    /// Removes up to `limit` expired records; returns how many.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn purge_expired(&self, now: SystemTime, limit: usize) -> Result<usize, StoreError>;
}

/// An in-memory [`GrantStore`] for tests and single-process servers. Its
/// records are lost when the process ends, so clients sign in again.
pub struct MemoryStore {
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    pending: HashMap<String, PendingRecord>,
    /// Pending references in insertion order, for evicting the oldest.
    order: VecDeque<String>,
    codes: HashMap<Digest, CodeRecord>,
    families: HashMap<String, FamilyRecord>,
    access: HashMap<Digest, AccessRecord>,
}

impl MemoryStore {
    /// A store holding up to [`DEFAULT_MEMORY_CAPACITY`] of each record kind.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MEMORY_CAPACITY)
    }

    /// A store holding up to `capacity` of each record kind, clamped to
    /// 1..=[`MAX_MEMORY_CAPACITY`].
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, MAX_MEMORY_CAPACITY),
            state: Mutex::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// References in the eviction queue, live or completed.
    #[cfg(test)]
    pub(super) fn queued_references(&self) -> usize {
        self.lock().order.len()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    fn purge(&mut self, now: SystemTime, limit: usize) -> usize {
        let mut left = limit;
        let mut take = |expired: bool| {
            let remove = expired && left > 0;
            left -= usize::from(remove);
            !remove
        };
        self.pending
            .retain(|_, record| take(record.expires_at <= now));
        self.codes
            .retain(|_, record| take(record.expires_at <= now));
        let mut ended = HashSet::new();
        self.families.retain(|id, family| {
            let keep = take(family.expires_at <= now || family.refresh_expires_at <= now);
            if !keep {
                ended.insert(id.clone());
            }
            keep
        });
        self.access
            .retain(|_, record| take(record.expires_at <= now));
        // One pass over the access tokens, however many families ended.
        if !ended.is_empty() {
            self.access
                .retain(|_, record| !ended.contains(&record.family_id));
        }
        let pending = &self.pending;
        self.order
            .retain(|reference| pending.contains_key(reference));
        limit - left
    }

    fn live_family(&self, id: &str, now: SystemTime) -> Option<&FamilyRecord> {
        self.families
            .get(id)
            .filter(|family| family.expires_at > now && family.refresh_expires_at > now)
    }
}

impl GrantStore for MemoryStore {
    fn put_pending(&self, record: PendingRecord) -> Result<(), StoreError> {
        let mut state = self.lock();
        // Anyone can start one, so the oldest gives way rather than refusing.
        while state.pending.len() >= self.capacity {
            let Some(oldest) = state.order.pop_front() else {
                break;
            };
            state.pending.remove(&oldest);
        }
        // Completed authorizations leave their references behind; drop them
        // once they could outnumber the live ones, which keeps the queue
        // within twice the capacity at constant amortized cost.
        if state.order.len() >= 2 * self.capacity {
            let State { order, pending, .. } = &mut *state;
            order.retain(|reference| pending.contains_key(reference));
        }
        state.order.push_back(record.reference.clone());
        state.pending.insert(record.reference.clone(), record);
        Ok(())
    }

    fn get_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError> {
        Ok(self
            .lock()
            .pending
            .get(reference)
            .filter(|record| record.expires_at > now)
            .cloned())
    }

    fn take_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError> {
        Ok(self
            .lock()
            .pending
            .remove(reference)
            .filter(|record| record.expires_at > now))
    }

    fn put_code(&self, record: CodeRecord) -> Result<(), StoreError> {
        let mut state = self.lock();
        if state.codes.len() >= self.capacity {
            state.purge(SystemTime::now(), usize::MAX);
            if state.codes.len() >= self.capacity {
                return Err(StoreError::Full);
            }
        }
        state.codes.insert(record.digest, record);
        Ok(())
    }

    fn take_code(&self, digest: &Digest, now: SystemTime) -> Result<CodeTake, StoreError> {
        let mut state = self.lock();
        let Some(record) = state
            .codes
            .get_mut(digest)
            .filter(|record| record.expires_at > now)
        else {
            return Ok(CodeTake::Unknown);
        };
        if record.consumed {
            return Ok(CodeTake::Reused(record.family_id.clone()));
        }
        record.consumed = true;
        Ok(CodeTake::Taken(record.clone()))
    }

    fn create_family(
        &self,
        code: &Digest,
        family: FamilyRecord,
        access: AccessRecord,
    ) -> Result<bool, StoreError> {
        let mut state = self.lock();
        if state.families.len() >= self.capacity {
            state.purge(SystemTime::now(), usize::MAX);
            if state.families.len() >= self.capacity {
                return Err(StoreError::Full);
            }
        }
        // `revoke_family` removes the family's codes, so a replay that came
        // first leaves nothing here to create from.
        let produced = state
            .codes
            .get(code)
            .is_some_and(|record| record.consumed && record.family_id == family.id);
        if !produced {
            return Ok(false);
        }
        state.access.insert(access.digest, access);
        state.families.insert(family.id.clone(), family);
        Ok(true)
    }

    fn get_family(&self, id: &str, now: SystemTime) -> Result<Option<FamilyRecord>, StoreError> {
        Ok(self.lock().live_family(id, now).cloned())
    }

    fn rotate(&self, rotation: Rotation, now: SystemTime) -> Result<Rotated, StoreError> {
        let mut state = self.lock();
        let Some(family) = state.live_family(&rotation.family_id, now) else {
            return Ok(Rotated::Unknown);
        };
        // Compare and swap: tokens prepared from an older snapshot never commit.
        if family.generation != rotation.expected {
            return Ok(Rotated::Stale);
        }
        // The presented generation comes from the client; never overflow on it.
        let Some(next) = family.generation.checked_add(1) else {
            return Ok(Rotated::Unknown);
        };
        let current =
            rotation.generation == family.generation && rotation.presented.matches(&family.refresh);
        let retried = rotation.generation.checked_add(1) == Some(family.generation)
            && family.previous.is_some_and(|(digest, replaced)| {
                digest.matches(&rotation.presented)
                    && now.duration_since(replaced).unwrap_or_default() < rotation.grace
            });
        if !current && !retried {
            // Only a token that once belonged to the family proves reuse; a
            // forged one naming the family and an old generation changes nothing.
            let reused =
                rotation.generation < family.generation && rotation.proof.matches(&family.proof);
            return Ok(if reused {
                Rotated::Reused
            } else {
                Rotated::Unknown
            });
        }
        let old_access = family.access;
        state.access.remove(&old_access);
        let family = state
            .families
            .get_mut(&rotation.family_id)
            .expect("the family was found above");
        family.previous = Some((family.refresh, now));
        family.generation = next;
        family.refresh = rotation.refresh;
        family.refresh_expires_at = rotation.refresh_expires_at;
        family.access = rotation.access.digest;
        let family = family.clone();
        state.access.insert(rotation.access.digest, rotation.access);
        Ok(Rotated::Rotated(Box::new(family)))
    }

    fn lookup_access(
        &self,
        digest: &Digest,
        now: SystemTime,
    ) -> Result<Option<(AccessRecord, FamilyRecord)>, StoreError> {
        let state = self.lock();
        let Some(access) = state
            .access
            .get(digest)
            .filter(|record| record.expires_at > now)
        else {
            return Ok(None);
        };
        Ok(state
            .live_family(&access.family_id, now)
            .map(|family| (access.clone(), family.clone())))
    }

    fn revoke_family(&self, id: &str) -> Result<(), StoreError> {
        let mut state = self.lock();
        state.families.remove(id);
        state.access.retain(|_, record| record.family_id != id);
        state.codes.retain(|_, record| record.family_id != id);
        Ok(())
    }

    fn purge_expired(&self, now: SystemTime, limit: usize) -> Result<usize, StoreError> {
        Ok(self.lock().purge(now, limit))
    }
}
