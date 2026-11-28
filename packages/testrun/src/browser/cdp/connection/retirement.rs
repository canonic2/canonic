//! Exact late-reply ownership after acknowledged native scope disposal.
use super::*;

pub(super) struct Retired {
    pub(super) method: String,
    pub(super) session: Option<String>,
    pub(super) waiting: Arc<AtomicBool>,
    pub(super) reply_seen: bool,
}

/// Reserve before native disposal; dropping an uncommitted reservation only
/// reopens admission. It never settles or abandons a live native call.
pub(crate) struct Retirement {
    shared: Arc<Shared>,
    sessions: BTreeSet<String>,
    reserved: usize,
    completed: bool,
}

impl State {
    pub(super) fn prune_retired(&mut self) {
        self.retired
            .retain(|_, reply| !reply.reply_seen || reply.waiting.load(Ordering::SeqCst));
    }
}

impl Connection {
    pub(crate) fn reserve_retirement(
        &self,
        sessions: impl IntoIterator<Item = String>,
    ) -> Result<Retirement, Error> {
        let sessions: BTreeSet<_> = sessions.into_iter().collect();
        let mut state = self.0.shared.state.lock().expect("CDP pending owner");
        state.prune_retired();
        if let Some(reason) = state.loss {
            return Err(Error::Lost {
                reason,
                dispatch: Dispatch::NotDispatched,
            });
        }
        if sessions
            .iter()
            .any(|session| state.retiring.contains(session))
        {
            return Err(Error::Busy);
        }
        let selected = |session: &Option<String>| {
            session
                .as_ref()
                .is_some_and(|session| sessions.contains(session))
        };
        let reserved = state
            .pending
            .values()
            .filter(|pending| selected(&pending.session))
            .count()
            + state
                .retained
                .iter()
                .filter(|reply| selected(&reply.reply.session_id))
                .count();
        if state.retired.len() + state.retirement_reserved + reserved
            > crate::browser::cdp::MAX_RETIRED_CALLS
        {
            return Err(Error::RetirementCapacity);
        }
        state.retirement_reserved += reserved;
        state.retiring.extend(sessions.iter().cloned());
        Ok(Retirement {
            shared: self.0.shared.clone(),
            sessions,
            reserved,
            completed: false,
        })
    }
}

impl Retirement {
    fn release(&mut self, state: &mut State) {
        state.retirement_reserved -= self.reserved;
        for session in &self.sessions {
            state.retiring.remove(session);
        }
        self.completed = true;
    }

    /// Only the owner of an acknowledged native close may commit this transfer.
    /// Active permits return; unreplied identities remain strict, bounded owners.
    pub(crate) fn commit_after_close(mut self) {
        let shared = self.shared.clone();
        let mut owner = shared.state.lock().expect("CDP pending owner");
        if owner.loss.is_some() {
            self.release(&mut owner);
            return;
        }
        let selected = |session: &Option<String>| {
            session
                .as_ref()
                .is_some_and(|session| self.sessions.contains(session))
        };
        let pending = owner
            .pending
            .iter()
            .filter_map(|(id, pending)| selected(&pending.session).then_some(*id))
            .collect::<Vec<_>>();
        for id in pending {
            let Pending {
                session,
                method,
                state,
                reply,
                permit,
                waiting,
            } = owner.pending.remove(&id).expect("Selected pending owner");
            let dispatch =
                match state.compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                {
                    Ok(_) | Err(CANCELLED) => Dispatch::NotDispatched,
                    Err(DISPATCHED) => Dispatch::PossiblyDispatched,
                    Err(_) => unreachable!("Native dispatch state"),
                };
            if dispatch == Dispatch::PossiblyDispatched {
                assert!(
                    owner
                        .retired
                        .insert(
                            id,
                            Retired {
                                session,
                                method,
                                waiting,
                                reply_seen: false
                            }
                        )
                        .is_none(),
                    "Native reply keeps one original owner"
                );
            }
            let _ = reply.send(Err(Error::ScopeClosed { dispatch }));
            drop(permit);
        }
        let retained = std::mem::take(&mut owner.retained);
        for reply in retained {
            if selected(&reply.reply.session_id) {
                owner.retained_bytes -= reply.bytes;
                assert!(
                    owner
                        .retired
                        .insert(
                            reply.reply.id,
                            Retired {
                                session: reply.reply.session_id,
                                method: reply.reply.method,
                                waiting: reply.waiting,
                                reply_seen: true
                            }
                        )
                        .is_none(),
                    "Native reply keeps one original owner"
                );
                // Native disposal proves its allocated resources no longer live.
                // Keep no caller payload and return this active permit now.
            } else {
                owner.retained.push_back(reply);
            }
        }
        owner.prune_retired();
        self.release(&mut owner);
    }
}

impl Drop for Retirement {
    fn drop(&mut self) {
        if !self.completed {
            let shared = self.shared.clone();
            let mut state = shared.state.lock().expect("CDP pending owner");
            self.release(&mut state);
        }
    }
}
