//! Capacity belongs to pending creations as well as published handle references.
use super::*;

pub(super) struct Allocation {
    operation: Option<(Uuid, u64)>,
    session: String,
    native_world: String,
    pub(super) settled: bool,
}

impl Evaluation {
    pub(super) fn handle_capacity(&self) -> Result<(), Error> {
        if self.handles.len() + self.allocations.len() >= MAX_HANDLES {
            return Err("HANDLE_CAPACITY".into());
        }
        Ok(())
    }
}

impl Worker {
    pub(super) fn reserve_handle_allocation(
        &mut self,
        operation: Uuid,
        world: Reference,
        session: &str,
    ) -> Result<Uuid, Error> {
        self.evaluation.handle_capacity()?;
        let route = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        if route.session_id != session {
            return Err("INVALID_SCOPE".into());
        }
        let key = Uuid::new_v4();
        let owner = self.execution_operation(operation);
        self.evaluation.allocations.insert(
            key,
            Allocation {
                operation: owner,
                session: session.to_owned(),
                native_world: route.execution_unique_id.ok_or("INVALID_SCOPE")?,
                settled: true,
            },
        );
        Ok(key)
    }

    pub(in crate::browser::cdp::session) fn execution_operation(
        &self,
        id: Uuid,
    ) -> Option<(Uuid, u64)> {
        self.shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .map(|operation| (id, operation.sequence))
    }

    pub(in crate::browser::cdp::session) fn owns_uncertain_execution(&self, id: Uuid) -> bool {
        let Some(operation) = self.execution_operation(id) else {
            return false;
        };
        if !self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.status().open)
        {
            return false;
        }
        self.evaluation.allocations.values().any(|allocation| {
            allocation.operation == Some(operation)
                && !allocation.settled
                && self.index.list(Kind::World).into_iter().any(|(world, _)| {
                    self.index
                        .route(world, Some(Kind::World))
                        .is_ok_and(|route| {
                            route.session_id == allocation.session
                                && route.execution_unique_id.as_deref()
                                    == Some(allocation.native_world.as_str())
                        })
                })
        }) || self
            .evaluation
            .groups
            .owns_uncertain_execution(operation, &self.index)
    }

    pub(super) fn finish_handle_allocation(
        &mut self,
        key: Option<Uuid>,
        scope: Reference,
        result: Result<Value, Error>,
    ) -> Result<Value, Error> {
        let Some(key) = key else {
            return result;
        };
        let pending = self
            .evaluation
            .allocations
            .get(&key)
            .is_some_and(|allocation| !allocation.settled);
        if !pending {
            self.evaluation.allocations.remove(&key);
        }
        result.map_err(|mut error| {
            if pending {
                let cause = error.partial.take();
                let mut partial = json!({"scope":scope,"handleAllocationPending":true,"potentiallyMutating":true,"nextAction":"browser_disconnect","cause":cause});
                for field in ["scopeClosed","callerEffectsUnknown","callerDispatched","dispatchCompleted","argumentCleanupPending"] {
                    if let Some(value) = cause.as_ref().and_then(|cause| cause.get(field)) { partial[field] = value.clone(); }
                }
                error.partial = Some(partial);
            }
            error
        })
    }

    pub(in crate::browser::cdp::session) fn handle_allocation_snapshot(&self) -> Value {
        json!({"limit":MAX_HANDLES,"retained":self.evaluation.handles.len(),"pending":self.evaluation.allocations.len(),"uncertain":self.evaluation.allocations.values().filter(|allocation| !allocation.settled).count()})
    }

    pub(in crate::browser::cdp::session) fn handle_allocation_end(&mut self, event: &Event) {
        // Document/reference rotation and broad context-clear notifications do
        // not prove an unanswered native call cannot allocate later. Only the
        // exact native realm destruction or inspector-session detachment ends it.
        match event.method.as_str() {
            "Runtime.executionContextDestroyed" => {
                if let (Some(session), Some(world)) = (
                    event.session_id.as_deref(),
                    event.params["executionContextUniqueId"].as_str(),
                ) {
                    self.evaluation.allocations.retain(|_, allocation| {
                        allocation.session != session || allocation.native_world != world
                    });
                }
            }
            "Target.detachedFromTarget" => {
                if let Some(session) = event.params["sessionId"].as_str() {
                    self.evaluation
                        .allocations
                        .retain(|_, allocation| allocation.session != session);
                }
            }
            _ => {}
        }
    }
}
