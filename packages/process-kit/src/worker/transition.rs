//! Ordered cutover; callback failure never silently returns fenced authority.
use super::{Outcome, Phase, Request, Slot, Supervisor, Transition, Worker, identity, lock};
use crate::{Error, Result, core::Quiescence, version::Version};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

pub(super) fn callback<T>(call: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).unwrap_or(Err(Error::Worker))
}

impl<W: Worker> Supervisor<W> {
    /// Replace at a between-call boundary. `launch` returns an inert, owned
    /// candidate; on error it must leave no untracked process. `commit` publishes
    /// product authority under its writer locks and may durably record the attempt.
    /// Both callbacks execute synchronously outside state/admission locks and must
    /// honor their deadline. A late returned candidate is still retained/cleaned.
    ///
    /// # Errors
    /// Refuses invalid/stale inputs and concurrent attempts without launch.
    /// Preparation errors resume only after candidate cleanup. Fence/commit/
    /// activation uncertainty retains both handles in `Reconcile` until shutdown
    /// or product reconciliation; this method never retries effects automatically.
    pub fn replace(
        &self,
        request: &Request,
        launch: impl FnOnce(Instant) -> Result<W>,
        commit: impl FnOnce(&Transition<'_>) -> Result<()>,
    ) -> Result<Outcome> {
        self.validate(request)?;
        if let Some(outcome) = Self::find(&*lock(&self.state)?, request)? {
            return Ok(outcome);
        }
        let _operation = self.operation.try_lock().map_err(|_| Error::HandoverBusy)?;
        if let Some(outcome) = Self::find(&*lock(&self.state)?, request)? {
            return Ok(outcome);
        }
        let old = self.begin(request)?;
        let mut pause = old.gate.quiesce()?;
        let result = callback(|| self.prepare(request, &old, &mut pause, launch, commit));
        if let Err(error) = result {
            if self.closing.load(Ordering::SeqCst) {
                if let Err(cleanup) = self.cleanup() {
                    return Err(Error::WorkerCleanup {
                        cause: Box::new(error),
                        cleanup: Box::new(cleanup),
                    });
                }
                return Err(error);
            }
            if old.gate.snapshot().handover_fenced {
                self.phase(Phase::Reconcile)?;
                return Err(error);
            }
            let candidate = lock(&self.state)?.candidate.clone();
            if let Some(slot) = candidate {
                if let Err(cleanup) =
                    callback(|| slot.worker.stop(Instant::now() + self.options.cleanup))
                {
                    self.phase(Phase::Reconcile)?;
                    return Err(Error::WorkerCleanup {
                        cause: Box::new(error),
                        cleanup: Box::new(cleanup),
                    });
                }
                lock(&self.state)?.candidate = None;
            }
            // Keep the public dispatch gate paused until the RAII gate resumes.
            pause.abort()?;
            self.phase(Phase::Deferred)?;
            return Err(error);
        }
        self.outcome(request)
    }

    fn validate(&self, request: &Request) -> Result<()> {
        if request.frontend != self.frontend.manager_id
            || request.sequence == 0
            || !crate::protocol::valid_id(&request.selection.key)
            || request.selection.requires.len() > 128
            || request
                .selection
                .requires
                .iter()
                .any(|cap| !crate::protocol::valid_id(cap))
        {
            return Err(Error::HandoverState);
        }
        Version::parse(&request.selection.build)?;
        Ok(())
    }
    fn begin(&self, request: &Request) -> Result<Slot<W>> {
        let mut state = lock(&self.state)?;
        if self.closing.load(Ordering::SeqCst)
            || !matches!(
                state.phase,
                Phase::Serving | Phase::Replaced | Phase::Deferred
            )
        {
            return Err(Error::HandoverBusy);
        }
        if request.sequence <= state.high_water {
            return Err(Error::HandoverState);
        }
        let old = state.active.as_ref().ok_or(Error::Draining)?.clone();
        if !Version::parse(&request.selection.build)?
            .is_newer_than(&Version::parse(&old.identity().build)?)
        {
            return Err(Error::Incompatible);
        }
        if let Some(previous) = state.current.take() {
            let bytes = crate::frame::encode_json(&previous, super::OUTCOME_LIMIT)?.len();
            state.history_bytes += bytes;
            state
                .history
                .insert(previous.request.sequence, (previous, bytes));
        }
        while state.history.len() >= self.options.history
            || state.history_bytes + super::OUTCOME_LIMIT > self.options.history_bytes
        {
            if let Some((_, (_, bytes))) = state.history.pop_first() {
                state.history_bytes -= bytes;
            }
        }
        state.high_water = request.sequence;
        state.current = Some(Outcome {
            request: request.clone(),
            phase: Phase::Preparing,
            successor: None,
            fenced: false,
            commit: super::Commit::NotStarted,
        });
        state.phase = Phase::Preparing;
        Ok(old)
    }
    fn prepare(
        &self,
        request: &Request,
        old: &Slot<W>,
        pause: &mut Quiescence,
        launch: impl FnOnce(Instant) -> Result<W>,
        commit: impl FnOnce(&Transition<'_>) -> Result<()>,
    ) -> Result<()> {
        let deadline = Instant::now() + self.options.preparation;
        self.checkpoint(deadline)?;
        let worker = Arc::new(callback(|| launch(deadline))?);
        let gate = crate::core::Admission::new();
        gate.set_waker(Some(std::task::Waker::from(Arc::clone(&self.notification))));
        gate.ready();
        lock(&self.state)?.candidate = Some(Slot {
            worker: Arc::clone(&worker),
            identity: None,
            gate,
            lease: Arc::clone(&self.lease),
        });
        self.checkpoint(deadline)?;
        let observed = worker.identity();
        identity(observed)?;
        let candidate_identity = observed.clone();
        if candidate_identity.manager_id == old.identity().manager_id
            || candidate_identity.manager_id == self.frontend.manager_id
            || candidate_identity.build != request.selection.build
        {
            return Err(Error::Incompatible);
        }
        candidate_identity.require(request.selection.protocol, &request.selection.requires)?;
        lock(&self.state)?
            .candidate
            .as_mut()
            .ok_or(Error::HandoverState)?
            .identity = Some(candidate_identity.clone());
        lock(&self.state)?
            .current
            .as_mut()
            .ok_or(Error::HandoverState)?
            .successor = Some(candidate_identity.clone());
        let transition = Transition {
            request,
            predecessor: old.identity(),
            successor: &candidate_identity,
            deadline,
        };
        callback(|| worker.prepare(&transition))?;
        self.wait_empty(pause, deadline)?;
        self.checkpoint(deadline)?;
        pause.fence()?;
        self.phase(Phase::Fenced)?;
        let transition = Transition {
            deadline: Instant::now() + self.options.activation,
            ..transition
        };
        callback(|| old.worker.fence(&transition))?;
        self.checkpoint(transition.deadline)?;
        callback(|| worker.prepare(&transition))?;
        self.checkpoint(transition.deadline)?;
        lock(&self.state)?
            .current
            .as_mut()
            .ok_or(Error::HandoverState)?
            .commit = super::Commit::Unknown;
        callback(|| commit(&transition))?;
        lock(&self.state)?
            .current
            .as_mut()
            .ok_or(Error::HandoverState)?
            .commit = super::Commit::Confirmed;
        self.phase(Phase::Committed)?;
        self.checkpoint(transition.deadline)?;
        callback(|| old.worker.retire(&transition))?;
        pause.commit()?;
        self.phase(Phase::Activating)?;
        self.checkpoint(transition.deadline)?;
        callback(|| worker.activate(&transition))?;
        self.checkpoint(transition.deadline)?;
        let mut state = lock(&self.state)?;
        if self.closing.load(Ordering::SeqCst) {
            return Err(Error::Draining);
        }
        state.active = state.candidate.take();
        state.phase = Phase::Replaced;
        state.current.as_mut().ok_or(Error::HandoverState)?.phase = Phase::Replaced;
        Ok(())
    }
    fn wait_empty(&self, pause: &Quiescence, deadline: Instant) -> Result<()> {
        let mut epoch = lock(&self.notification.epoch)?;
        while !pause.ready() {
            self.checkpoint(deadline)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            epoch = self
                .notification
                .changed
                .wait_timeout(epoch, remaining)
                .map_err(|_| Error::Worker)?
                .0;
        }
        self.checkpoint(deadline)
    }
}
