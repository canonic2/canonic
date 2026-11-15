//! The storage contract for issued grants, and the in-memory store.
//!
//! Records hold digests of every secret value, never the values themselves,
//! so a copy of the store cannot be replayed. Each [`GrantStore`] operation is
//! atomic; durable backends belong to the consumer.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

/// The default number of each record kind [`MemoryStore`] holds.
pub const DEFAULT_MEMORY_CAPACITY: usize = 4096;
/// The most records of each kind [`MemoryStore`] may hold.
pub const MAX_MEMORY_CAPACITY: usize = 1_048_576;

/// A SHA-256 digest of a secret value with its type prefix.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// An identity assertion that was redeemed, kept to refuse its replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionRecord {
    /// The digest of the assertion's issuer and `jti`.
    pub digest: Digest,
    /// When the assertion could no longer be accepted.
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
///
/// The library answers the client as soon as a method returns, so a durable
/// backend returns `Ok` only once the change survives a restart. Losing an
/// acknowledged [`take_pending`](Self::take_pending),
/// [`take_code`](Self::take_code), [`rotate`](Self::rotate) or
/// [`revoke_family`](Self::revoke_family) brings back a spent code, a replaced
/// refresh token or a revoked family.
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

    /// Inserts a `client_credentials` grant: a family of generation 0, which
    /// has no refresh token, with its one access token. When the family's
    /// client and subject already hold `limit` generation-0 grants, the oldest
    /// is removed first, with its access token.
    ///
    /// # Errors
    /// Returns [`StoreError`] when it cannot be stored.
    fn create_client_grant(
        &self,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
    ) -> Result<(), StoreError>;

    /// Whether an assertion with this digest was redeemed and its record is
    /// unexpired at `now`. The library asks before the consumer's admission
    /// hook runs; [`create_assertion_grant`](Self::create_assertion_grant)
    /// remains the atomic check.
    ///
    /// # Errors
    /// Returns [`StoreError::Unavailable`] when the backend fails.
    fn assertion_redeemed(&self, digest: &Digest, now: SystemTime) -> Result<bool, StoreError>;

    /// Inserts an identity-assertion grant like
    /// [`create_client_grant`](Self::create_client_grant), atomically with
    /// recording `assertion`. Returns `false`, inserting nothing, when a
    /// record with the same digest is unexpired at `now`: the assertion was
    /// replayed.
    ///
    /// # Errors
    /// Returns [`StoreError`] when it cannot be stored.
    fn create_assertion_grant(
        &self,
        assertion: AssertionRecord,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
        now: SystemTime,
    ) -> Result<bool, StoreError>;

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

/// A [`MemoryStore`] capacity outside 1..=[`MAX_MEMORY_CAPACITY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("memory store capacity {0} is outside 1..={MAX_MEMORY_CAPACITY}")]
pub struct CapacityError(pub usize);

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
    /// The code that creates each family, for revocation.
    family_codes: HashMap<String, Digest>,
    families: HashMap<String, FamilyRecord>,
    access: HashMap<Digest, AccessRecord>,
    /// Each client and subject's generation-0 families, oldest first.
    client_grants: HashMap<(String, String), VecDeque<String>>,
    /// Redeemed identity assertions and when each record may go.
    assertions: HashMap<Digest, SystemTime>,
    /// Every record by when it expires, kept exact by every insertion and
    /// removal, so a purge visits only the records that are due.
    due: BTreeSet<(SystemTime, Record)>,
    /// Records purges visited, for the work guard in tests.
    #[cfg(test)]
    visited: usize,
}

/// A record named in the expiry index. Access tokens sort before families, so
/// a token expiring with its family is purged, and counted, on its own first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Record {
    Pending(String),
    Code(Digest),
    Access(Digest),
    Family(String),
    Assertion(Digest),
}

/// When a family stops being live: its absolute or refresh expiry.
fn family_expiry(family: &FamilyRecord) -> SystemTime {
    family.expires_at.min(family.refresh_expires_at)
}

impl MemoryStore {
    /// A store holding up to [`DEFAULT_MEMORY_CAPACITY`] of each record kind.
    #[must_use]
    pub fn new() -> Self {
        Self {
            capacity: DEFAULT_MEMORY_CAPACITY,
            state: Mutex::default(),
        }
    }

    /// A store holding up to `capacity` of each record kind.
    ///
    /// # Errors
    /// Returns [`CapacityError`] when `capacity` is outside
    /// 1..=[`MAX_MEMORY_CAPACITY`].
    pub fn with_capacity(capacity: usize) -> Result<Self, CapacityError> {
        if !(1..=MAX_MEMORY_CAPACITY).contains(&capacity) {
            return Err(CapacityError(capacity));
        }
        Ok(Self {
            capacity,
            state: Mutex::default(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// References in the eviction queue, live or completed.
    #[cfg(test)]
    pub(super) fn queued_references(&self) -> usize {
        self.lock().order.len()
    }

    /// Records purges have visited, and entries in the expiry index.
    #[cfg(test)]
    pub(super) fn purge_work(&self) -> (usize, usize) {
        let state = self.lock();
        (state.visited, state.due.len())
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    /// Removes up to `limit` due records in expiry order, visiting no others,
    /// and returns how many. A family that ends before its access token takes
    /// the token with it; that token is not expired, so it is not counted.
    fn purge(&mut self, now: SystemTime, limit: usize) -> usize {
        let mut removed = 0;
        while removed < limit && self.due.first().is_some_and(|(at, _)| *at <= now) {
            let Some((_, record)) = self.due.pop_first() else {
                break;
            };
            removed += 1;
            #[cfg(test)]
            {
                self.visited += 1;
            }
            match record {
                Record::Pending(reference) => {
                    self.drop_pending(&reference);
                }
                Record::Code(digest) => {
                    self.drop_code(&digest);
                }
                Record::Family(id) => {
                    self.drop_family(&id);
                }
                Record::Access(digest) => {
                    self.drop_access(&digest);
                }
                Record::Assertion(digest) => {
                    self.assertions.remove(&digest);
                }
            }
        }
        removed
    }

    // Each `add_*` removes the entry of a record it replaces before indexing
    // the new one, so a replacement keeps exactly its own expiry entry.

    fn add_pending(&mut self, record: PendingRecord) {
        self.drop_pending(&record.reference);
        self.due
            .insert((record.expires_at, Record::Pending(record.reference.clone())));
        self.pending.insert(record.reference.clone(), record);
    }

    fn drop_pending(&mut self, reference: &str) -> Option<PendingRecord> {
        let record = self.pending.remove(reference)?;
        self.due
            .remove(&(record.expires_at, Record::Pending(reference.to_owned())));
        Some(record)
    }

    fn add_code(&mut self, record: CodeRecord) {
        self.drop_code(&record.digest);
        self.due
            .insert((record.expires_at, Record::Code(record.digest)));
        self.family_codes
            .insert(record.family_id.clone(), record.digest);
        self.codes.insert(record.digest, record);
    }

    fn drop_code(&mut self, digest: &Digest) -> Option<CodeRecord> {
        let record = self.codes.remove(digest)?;
        self.due.remove(&(record.expires_at, Record::Code(*digest)));
        if self.family_codes.get(&record.family_id) == Some(digest) {
            self.family_codes.remove(&record.family_id);
        }
        Some(record)
    }

    fn add_access(&mut self, record: AccessRecord) {
        self.drop_access(&record.digest);
        self.due
            .insert((record.expires_at, Record::Access(record.digest)));
        self.access.insert(record.digest, record);
    }

    fn drop_access(&mut self, digest: &Digest) {
        if let Some(record) = self.access.remove(digest) {
            self.due
                .remove(&(record.expires_at, Record::Access(*digest)));
        }
    }

    fn add_family(&mut self, family: FamilyRecord) {
        // Only the index entry: the replaced family's token and grant list
        // entry belong to the caller.
        if let Some(old) = self.families.get(&family.id) {
            self.due
                .remove(&(family_expiry(old), Record::Family(family.id.clone())));
        }
        self.due
            .insert((family_expiry(&family), Record::Family(family.id.clone())));
        self.families.insert(family.id.clone(), family);
    }

    /// Removes a family with its access token and grant list entry.
    fn drop_family(&mut self, id: &str) -> Option<FamilyRecord> {
        let family = self.families.remove(id)?;
        self.due
            .remove(&(family_expiry(&family), Record::Family(id.to_owned())));
        self.drop_access(&family.access);
        let key = (family.client_id.clone(), family.subject.clone());
        if let Some(grants) = self.client_grants.get_mut(&key) {
            grants.retain(|grant| grant != id);
            if grants.is_empty() {
                self.client_grants.remove(&key);
            }
        }
        Some(family)
    }

    fn add_assertion(&mut self, digest: Digest, expires_at: SystemTime) {
        if let Some(old) = self.assertions.remove(&digest) {
            self.due.remove(&(old, Record::Assertion(digest)));
        }
        self.due.insert((expires_at, Record::Assertion(digest)));
        self.assertions.insert(digest, expires_at);
    }

    /// Inserts a generation-0 family, first removing its client and subject's
    /// oldest such family once they hold `limit`.
    fn single_grant(
        &mut self,
        capacity: usize,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
    ) -> Result<(), StoreError> {
        let key = (family.client_id.clone(), family.subject.clone());
        while let Some(oldest) = self
            .client_grants
            .get(&key)
            .filter(|grants| grants.len() >= limit.max(1))
            .and_then(|grants| grants.front().cloned())
        {
            if self.drop_family(&oldest).is_none()
                && let Some(grants) = self.client_grants.get_mut(&key)
            {
                grants.retain(|grant| *grant != oldest);
            }
        }
        if self.families.len() >= capacity {
            self.purge(SystemTime::now(), usize::MAX);
            if self.families.len() >= capacity {
                return Err(StoreError::Full);
            }
        }
        self.client_grants
            .entry(key)
            .or_default()
            .push_back(family.id.clone());
        self.add_access(access);
        self.add_family(family);
        Ok(())
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
            state.drop_pending(&oldest);
        }
        // Completed authorizations leave their references behind; drop them
        // once they could outnumber the live ones, which keeps the queue
        // within twice the capacity at constant amortized cost.
        if state.order.len() >= 2 * self.capacity {
            let State { order, pending, .. } = &mut *state;
            order.retain(|reference| pending.contains_key(reference));
        }
        state.order.push_back(record.reference.clone());
        state.add_pending(record);
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
            .drop_pending(reference)
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
        state.add_code(record);
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
        state.add_access(access);
        state.add_family(family);
        Ok(true)
    }

    fn get_family(&self, id: &str, now: SystemTime) -> Result<Option<FamilyRecord>, StoreError> {
        Ok(self.lock().live_family(id, now).cloned())
    }

    fn create_client_grant(
        &self,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
    ) -> Result<(), StoreError> {
        self.lock()
            .single_grant(self.capacity, family, access, limit)
    }

    fn assertion_redeemed(&self, digest: &Digest, now: SystemTime) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .assertions
            .get(digest)
            .is_some_and(|expires_at| *expires_at > now))
    }

    fn create_assertion_grant(
        &self,
        assertion: AssertionRecord,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
        now: SystemTime,
    ) -> Result<bool, StoreError> {
        let mut state = self.lock();
        if state
            .assertions
            .get(&assertion.digest)
            .is_some_and(|expires_at| *expires_at > now)
        {
            return Ok(false);
        }
        if state.assertions.len() >= self.capacity {
            state.purge(now, usize::MAX);
            if state.assertions.len() >= self.capacity {
                return Err(StoreError::Full);
            }
        }
        state.single_grant(self.capacity, family, access, limit)?;
        state.add_assertion(assertion.digest, assertion.expires_at);
        Ok(true)
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
        let (old_access, old_expiry) = (family.access, family_expiry(family));
        state.drop_access(&old_access);
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
        let id = Record::Family(family.id.clone());
        state.due.remove(&(old_expiry, id.clone()));
        state.due.insert((family_expiry(&family), id));
        state.add_access(rotation.access);
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
        // A family holds one access token and is produced by at most one code.
        state.drop_family(id);
        if let Some(code) = state.family_codes.get(id).copied() {
            state.drop_code(&code);
        }
        Ok(())
    }

    fn purge_expired(&self, now: SystemTime, limit: usize) -> Result<usize, StoreError> {
        Ok(self.lock().purge(now, limit))
    }
}
