//! Private temporary object groups retain uncertain calls until attachment loss.
use super::*;
pub(super) type Key = (String, String);
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    DiagnosticRead,
    DeveloperTransfer,
    HeapInspection,
    HeapCleanup,
    ArgumentAdoption,
    FrameRelationship,
}
struct Owner {
    operation: Option<(Uuid, u64)>,
    context: Reference,
    purpose: Purpose,
    settled: bool,
    in_use: bool,
    releasing: bool,
    target: Option<String>,
}
#[derive(Default)]
pub(super) struct Groups {
    items: BTreeMap<Key, Owner>,
}
impl Groups {
    pub(super) fn call_scope(
        &self,
        operation: Option<(Uuid, u64)>,
        key: &Key,
    ) -> Option<Reference> {
        let operation = operation?;
        let owner = self.items.get(key)?;
        (owner.operation == Some(operation)
            && !owner.settled
            && owner.in_use
            && !owner.releasing
            && matches!(
                owner.purpose,
                Purpose::ArgumentAdoption | Purpose::FrameRelationship
            ))
        .then_some(owner.context)
    }

    pub(super) fn owns_uncertain_execution(&self, operation: (Uuid, u64), index: &Index) -> bool {
        self.items.iter().any(|((session, _), owner)| {
            owner.operation == Some(operation)
                && !owner.settled
                && matches!(
                    owner.purpose,
                    Purpose::ArgumentAdoption | Purpose::FrameRelationship
                )
                && index
                    .diagnostic_scope(session)
                    .is_some_and(|(context, _, _)| context == owner.context)
        })
    }
    pub(super) fn contains(&self, key: &Key) -> bool {
        self.items.contains_key(key)
    }
    pub(super) fn transfer_settled(&mut self, key: &Key) -> Result<(), Error> {
        if !self
            .items
            .get(key)
            .is_some_and(|owner| owner.settled && owner.in_use && !owner.releasing)
        {
            return Err("EXECUTION_CLEANUP_PENDING".into());
        }
        self.items.remove(key);
        Ok(())
    }
    pub(super) fn pending_heap(&self) -> usize {
        self.items
            .values()
            .filter(|owner| {
                matches!(
                    owner.purpose,
                    Purpose::HeapInspection | Purpose::HeapCleanup
                )
            })
            .count()
    }
    pub(super) fn capacity(&self, purpose: Purpose) -> Result<(), Error> {
        if self
            .items
            .values()
            .filter(|owner| owner.purpose == purpose)
            .count()
            >= 32
        {
            return Err(match purpose {
                Purpose::DiagnosticRead => "DIAGNOSTIC_ARGUMENT_READ_CAPACITY",
                Purpose::DeveloperTransfer => "DEVELOPER_RESULT_TRANSFER_CAPACITY",
                Purpose::HeapInspection => "HEAP_INSPECTION_CAPACITY",
                Purpose::HeapCleanup => "HEAP_CLEANUP_CAPACITY",
                Purpose::ArgumentAdoption => "EVALUATION_ADOPTION_CAPACITY",
                Purpose::FrameRelationship => "FRAME_RELATIONSHIP_CAPACITY",
            }
            .into());
        }
        Ok(())
    }
    pub(super) fn begin(
        &mut self,
        context: Reference,
        session: String,
        purpose: Purpose,
    ) -> Result<Key, Error> {
        self.capacity(purpose)?;
        let prefix = match purpose {
            Purpose::DiagnosticRead => "diagnostic-argument-read",
            Purpose::DeveloperTransfer => "developer-result-transfer",
            Purpose::HeapInspection => "heap-inspection",
            Purpose::HeapCleanup => "heap-cleanup",
            Purpose::ArgumentAdoption => "argument-adoption",
            Purpose::FrameRelationship => "frame-relationship",
        };
        let key = (session, format!("testrun:{prefix}:{}", Uuid::new_v4()));
        self.items.insert(
            key.clone(),
            Owner {
                operation: None,
                context,
                purpose,
                settled: matches!(
                    purpose,
                    Purpose::ArgumentAdoption | Purpose::FrameRelationship
                ),
                // These transactions span separately settled calls. Event-driven
                // housekeeping cannot release a group borrowed by that request.
                in_use: matches!(
                    purpose,
                    Purpose::ArgumentAdoption | Purpose::FrameRelationship
                ),
                releasing: false,
                target: None,
            },
        );
        Ok(key)
    }
    pub(super) fn begin_private(
        &mut self,
        developer: Reference,
        target: String,
        session: String,
        purpose: Purpose,
    ) -> Result<Key, Error> {
        let key = self.begin(developer, session, purpose)?;
        self.items.get_mut(&key).expect("Allocated group").target = Some(target);
        Ok(key)
    }
    pub(super) fn end_session(&mut self, session: &str) {
        self.items.retain(|(owner, _), _| owner != session);
    }
    pub(super) fn observe_private_event(&mut self, event: &Event) {
        if event.method == "Target.detachedFromTarget"
            && let Some(session) = event.params["sessionId"].as_str()
        {
            self.end_session(session);
        }
        if event.method == "Target.targetDestroyed"
            && let Some(target) = event.params["targetId"].as_str()
        {
            self.items
                .retain(|_, owner| owner.target.as_deref() != Some(target));
        }
    }
    pub(super) fn pending(&self, context: Reference, purpose: Purpose) -> usize {
        self.items
            .values()
            .filter(|owner| owner.context == context && owner.purpose == purpose)
            .count()
    }
}

impl Worker {
    pub(super) fn fence_execution_groups(&mut self) {
        self.evaluation.groups.items.retain(|(session, _), owner| {
            owner.target.is_some() || self.index.session_attached(session)
        });
    }
    pub(super) async fn execution_group_call(
        &mut self,
        id: Uuid,
        key: &Key,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.execution_group_call_settled(id, key, method, params, deadline)
            .await
            .0
    }
    pub(super) async fn execution_group_call_settled(
        &mut self,
        id: Uuid,
        key: &Key,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> (Result<Value, Error>, bool) {
        let operation = self.execution_operation(id);
        let Some(owner) = self.evaluation.groups.items.get_mut(key) else {
            return (Err("STALE_REFERENCE".into()), true);
        };
        if owner.releasing {
            return (Err("EXECUTION_CLEANUP_PENDING".into()), true);
        }
        owner.settled = false;
        owner.operation = operation;
        let (mut result, settled) = self
            .call_with_settlement(id, Some(&key.0), method, params, deadline, Some(key))
            .await;
        if let Some(owner) = self.evaluation.groups.items.get_mut(key) {
            owner.settled = settled;
        }
        if !settled && let Err(error) = result {
            if error.code == "STALE_REFERENCE"
                && error
                    .partial
                    .as_ref()
                    .is_some_and(|partial| partial["scopeClosed"] == true)
            {
                // Context disposal closes the caller scope while its original
                // native reply remains unconfirmed. Preserve both facts.
                return (Err(error), settled);
            }
            let mut unknown = Error::from("OUTCOME_UNKNOWN");
            unknown.partial = Some(
                json!({"causeCode":error.code,"cause":error.partial,"potentiallyMutating":true}),
            );
            result = Err(unknown);
        }
        // Group retirement can happen during the call. Its absence is not a
        // witness that the original native dispatch has settled.
        (result, settled)
    }
    pub(super) async fn release_execution_groups(&mut self, deadline: Instant) {
        self.fence_execution_groups();
        let ready = self
            .evaluation
            .groups
            .items
            .iter()
            .filter(|(_, owner)| owner.settled && !owner.releasing && !owner.in_use)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in ready {
            self.release_execution_group(&key, deadline).await;
        }
    }
    pub(super) async fn release_execution_group(&mut self, key: &Key, deadline: Instant) {
        // The request explicitly relinquishes its borrow even if the last call
        // is uncertain. Settlement still independently gates native release.
        if let Some(owner) = self.evaluation.groups.items.get_mut(key) {
            owner.in_use = false;
        }
        if self
            .evaluation
            .groups
            .items
            .get(key)
            .is_some_and(|owner| owner.settled && !owner.releasing)
        {
            if let Some(owner) = self.evaluation.groups.items.get_mut(key) {
                owner.releasing = true;
            }
            let result = self
                .cleanup_call(
                    Some(&key.0),
                    "Runtime.releaseObjectGroup",
                    json!({"objectGroup":key.1}),
                    deadline,
                )
                .await;
            if result.is_ok() {
                self.evaluation.groups.items.remove(key);
            } else if let Some(owner) = self.evaluation.groups.items.get_mut(key) {
                owner.releasing = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_element_group_scope_requires_exact_active_operation_and_transaction_purpose() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let operation = (Uuid::new_v4(), 7);
        let mut groups = Groups::default();
        for purpose in [Purpose::ArgumentAdoption, Purpose::FrameRelationship] {
            let key = groups
                .begin(context, "Acme-session".into(), purpose)
                .unwrap();
            let owner = groups.items.get_mut(&key).unwrap();
            owner.operation = Some(operation);
            owner.settled = false;
            assert_eq!(groups.call_scope(Some(operation), &key), Some(context));
            assert_eq!(groups.call_scope(Some((operation.0, 8)), &key), None);
            assert_eq!(groups.call_scope(Some((Uuid::new_v4(), 7)), &key), None);
            assert_eq!(groups.call_scope(None, &key), None);
            groups.items.get_mut(&key).unwrap().operation = None;
            assert_eq!(
                groups.call_scope(None, &key),
                None,
                "Missing operation identities cannot supply scope authority"
            );
            groups.items.get_mut(&key).unwrap().operation = Some(operation);
            assert_eq!(
                groups.call_scope(Some(operation), &("Acme-foreign".into(), key.1.clone())),
                None
            );
            for state in ["settled", "unborrowed", "releasing"] {
                let owner = groups.items.get_mut(&key).unwrap();
                owner.settled = state == "settled";
                owner.in_use = state != "unborrowed";
                owner.releasing = state == "releasing";
                assert_eq!(groups.call_scope(Some(operation), &key), None);
            }
        }
        let key = groups
            .begin(context, "Acme-session".into(), Purpose::HeapInspection)
            .unwrap();
        let owner = groups.items.get_mut(&key).unwrap();
        owner.operation = Some(operation);
        owner.in_use = true;
        assert_eq!(
            groups.call_scope(Some(operation), &key),
            None,
            "Unrelated private owners cannot acquire caller scope-close semantics"
        );
    }
    #[test]
    fn private_heap_groups_require_attachment_loss_and_reserve_cleanup_capacity() {
        let developer = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let mut groups = Groups::default();
        for _ in 0..32 {
            groups
                .begin_private(
                    developer,
                    "Acme frontend".into(),
                    "Acme inspector".into(),
                    Purpose::HeapInspection,
                )
                .unwrap();
        }
        assert!(groups.capacity(Purpose::HeapInspection).is_err());
        let cleanup = groups
            .begin_private(
                developer,
                "Acme frontend".into(),
                "Acme inspector".into(),
                Purpose::HeapCleanup,
            )
            .unwrap();
        groups.observe_private_event(&Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":"Acme page"}),
        });
        assert!(groups.contains(&cleanup));
        groups.observe_private_event(&Event {
            method: "Target.detachedFromTarget".into(),
            session_id: None,
            params: json!({"sessionId":"Acme inspector"}),
        });
        assert_eq!(groups.pending(developer, Purpose::HeapInspection), 0);
        assert!(!groups.contains(&cleanup));
    }
    #[test]
    fn temporary_execution_groups_bound_each_purpose_without_reusing_namespaces() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let mut groups = Groups::default();
        let mut names = BTreeSet::new();
        for purpose in [Purpose::DiagnosticRead, Purpose::DeveloperTransfer] {
            for _ in 0..32 {
                let key = groups
                    .begin(context, "Acme session".into(), purpose)
                    .unwrap();
                assert!(names.insert(key.1));
            }
            assert!(groups.capacity(purpose).is_err());
            assert_eq!(groups.pending(context, purpose), 32);
        }
        assert_eq!(groups.items.len(), 64);
        assert!(
            groups
                .items
                .values()
                .all(|owner| !owner.settled && !owner.releasing)
        );
    }
}
