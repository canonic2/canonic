//! Connection-owned snapshots outlive source scopes; leases outlive removal.
use super::*;
const MAX_INPUTS: usize = 512;
const MAX_BYTES: usize = 32 * 1048576;

struct Entry {
    context: Reference,
    snapshot: Arc<Snapshot>,
    unavailable: Option<&'static str>,
}
#[derive(Default)]
pub(crate) struct Store {
    connection: Option<Uuid>,
    items: BTreeMap<Uuid, Entry>,
    bytes: usize,
    refusals: u64,
    enabled: BTreeMap<Reference, bool>,
}
impl Store {
    pub(crate) fn start(&mut self, connection: Uuid, context: Reference) -> Result<(), Error> {
        // An explicit opt-out survives subscription restarts. New diagnostic
        // owners retain Request inputs before subscription events can arrive.
        let enabled = self.enabled.get(&context).copied().unwrap_or(true);
        self.set(connection, context, enabled)
    }
    pub(crate) fn enabled(&self, context: Reference) -> bool {
        self.enabled.get(&context).copied().unwrap_or(false)
    }
    pub(crate) fn set(
        &mut self,
        connection: Uuid,
        context: Reference,
        enabled: bool,
    ) -> Result<(), Error> {
        if context.connection_id != connection
            || self.connection.is_some_and(|owner| owner != connection)
        {
            return Err("HTTP_REQUEST_INPUT_SCOPE".into());
        }
        if !self.enabled.contains_key(&context) && self.enabled.len() >= MAX_INPUTS {
            return Err("HTTP_REQUEST_INPUT_CAPACITY".into());
        }
        self.connection = Some(connection);
        self.enabled.insert(context, enabled);
        Ok(())
    }
    pub(crate) fn close_context(&mut self, context: Reference) {
        self.enabled.remove(&context);
    }
    pub(crate) fn insert(
        &mut self,
        connection: Uuid,
        context: Reference,
        id: Uuid,
        snapshot: Snapshot,
    ) -> Result<bool, Error> {
        if context.connection_id != connection
            || self.connection.is_some_and(|owner| owner != connection)
        {
            return Err("HTTP_REQUEST_INPUT_SCOPE".into());
        }
        if let Some(known) = self.items.get(&id) {
            if known.context != context {
                return Err("HTTP_REQUEST_INPUT_SCOPE".into());
            }
            // Later response/finish observations cannot rewrite the initializer.
            return Ok(false);
        }
        let bytes = snapshot.bytes();
        if self.items.len() >= MAX_INPUTS || self.bytes.saturating_add(bytes) > MAX_BYTES {
            self.refusals = self.refusals.saturating_add(1);
            return Err("HTTP_REQUEST_INPUT_CAPACITY".into());
        }
        self.connection = Some(connection);
        self.bytes += bytes;
        self.items.insert(
            id,
            Entry {
                context,
                snapshot: Arc::new(snapshot),
                unavailable: None,
            },
        );
        Ok(true)
    }
    pub(crate) fn get(&self, connection: Uuid, id: Uuid) -> Result<Arc<Snapshot>, Error> {
        if self.connection != Some(connection) {
            return Err("HTTP_REQUEST_INPUT_UNKNOWN".into());
        }
        self.items
            .get(&id)
            .ok_or_else(|| Error::from("HTTP_REQUEST_INPUT_UNKNOWN"))
            .and_then(|entry| match entry.unavailable {
                Some(code) => Err(code.into()),
                None => Ok(entry.snapshot.clone()),
            })
    }
    pub(crate) fn update(
        &mut self,
        connection: Uuid,
        id: Uuid,
        changes: &crate::browser::routing::Overrides,
    ) -> Result<bool, Error> {
        if self.connection.is_some_and(|owner| owner != connection) {
            return Err("HTTP_REQUEST_INPUT_SCOPE".into());
        }
        let Some(entry) = self.items.get_mut(&id) else {
            return Ok(false);
        };
        if let Some(code) = entry.unavailable {
            return Err(code.into());
        }
        let snapshot = match entry.snapshot.routed(changes) {
            Ok(snapshot) => snapshot,
            Err(error) if error.code == "HTTP_REQUEST_INPUT_BOUNDS" => {
                entry.unavailable = Some("HTTP_REQUEST_INPUT_BOUNDS");
                self.refusals = self.refusals.saturating_add(1);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let bytes = self.bytes - entry.snapshot.bytes() + snapshot.bytes();
        if bytes > MAX_BYTES {
            // Keep accounting and existing leases, but never serve stale intent
            // after a source update we cannot retain. Unrelated inputs stay put.
            entry.unavailable = Some("HTTP_REQUEST_INPUT_CAPACITY");
            self.refusals = self.refusals.saturating_add(1);
            return Err("HTTP_REQUEST_INPUT_CAPACITY".into());
        }
        self.bytes = bytes;
        entry.snapshot = Arc::new(snapshot);
        Ok(true)
    }
    pub(crate) fn remove(&mut self, connection: Uuid, id: Uuid) -> Result<bool, Error> {
        if self.connection.is_some_and(|owner| owner != connection) {
            return Err("HTTP_REQUEST_INPUT_SCOPE".into());
        }
        let Some(entry) = self.items.remove(&id) else {
            return Ok(false);
        };
        self.bytes -= entry.snapshot.bytes();
        Ok(true)
    }
    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.bytes = 0;
        self.enabled.clear();
    }
    pub(crate) fn summary(&self) -> Value {
        json!({"retainedInputs":self.items.len(),"retainedBytes":self.bytes,"capacityRefusals":self.refusals,"maxInputs":MAX_INPUTS,"maxBytes":MAX_BYTES,"privacy":"private-request-initializer-data"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(connection: Uuid) -> Reference {
        Reference {
            connection_id: connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    fn source(body: &[u8]) -> Snapshot {
        Snapshot::pinned(&json!({"url":"https://example.com/Acme-private?secret=one","method":"POST","headers":[],"postData":STANDARD.encode(body)})).unwrap()
    }
    fn body(source: &Snapshot) -> Vec<u8> {
        source
            .fetch(serde_json::from_value(json!({})).unwrap())
            .unwrap()
            .body
            .unwrap()
            .bytes()
            .unwrap()
    }
    #[test]
    fn http_request_input_default_start_preserves_explicit_choices_and_connection_ownership() {
        let owner = Uuid::new_v4();
        let first = context(owner);
        let second = context(owner);
        let mut store = Store::default();
        store.set(owner, first, false).unwrap();
        store.start(owner, first).unwrap();
        assert!(!store.enabled(first));
        store.start(owner, second).unwrap();
        assert!(store.enabled(second));
        store.start(owner, first).unwrap();
        assert!(!store.enabled(first));
        store.set(owner, first, true).unwrap();
        store.start(owner, first).unwrap();
        assert!(store.enabled(first));
        assert_eq!(
            store.start(Uuid::new_v4(), first).unwrap_err().code,
            "HTTP_REQUEST_INPUT_SCOPE"
        );
        assert!(store.enabled(first));
        assert!(store.enabled(second));
    }
    #[test]
    fn http_request_input_default_context_retirement_resets_choice_without_losing_old_sources() {
        let owner = Uuid::new_v4();
        let original = context(owner);
        let next = Reference {
            generation: Uuid::new_v4(),
            ..original
        };
        let id = Uuid::new_v4();
        let mut store = Store::default();
        store.start(owner, original).unwrap();
        store
            .insert(owner, original, id, source(&[0, 255, 128]))
            .unwrap();
        store.set(owner, original, false).unwrap();
        store.close_context(original);
        store.start(owner, next).unwrap();
        assert!(store.enabled(next));
        assert!(!store.enabled(original));
        assert_eq!(body(&store.get(owner, id).unwrap()), [0, 255, 128]);
        store.clear();
        assert!(!store.enabled(next));
        assert!(store.get(owner, id).is_err());
    }
    #[test]
    fn http_request_input_default_start_capacity_failure_preserves_existing_opt_out() {
        let owner = Uuid::new_v4();
        let opted_out = context(owner);
        let mut store = Store::default();
        store.set(owner, opted_out, false).unwrap();
        for _ in 1..MAX_INPUTS {
            store.start(owner, context(owner)).unwrap();
        }
        assert_eq!(
            store.start(owner, context(owner)).unwrap_err().code,
            "HTTP_REQUEST_INPUT_CAPACITY"
        );
        store.start(owner, opted_out).unwrap();
        assert!(!store.enabled(opted_out));
        assert_eq!(store.enabled.len(), MAX_INPUTS);
    }
    #[test]
    fn http_request_input_route_updates_preserve_cumulative_intent_and_earlier_leases() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        store
            .insert(connection, context, id, source(&[1, 2, 3]))
            .unwrap();
        let initial = store.get(connection, id).unwrap();
        let first = serde_json::from_value(
            json!({"url":"https://example.com/changed","method":"PUT","bodyBase64":"AP+A"}),
        )
        .unwrap();
        assert!(store.update(connection, id, &first).unwrap());
        let middle = store.get(connection, id).unwrap();
        let next=serde_json::from_value(json!({"headers":[{"name":"X-Acme","value":"first"},{"name":"x-acme","value":"last"},{"name":"Cookie","value":"AcmeLayer=private"},{"name":"__proto__","value":"Acme literal"}]})).unwrap();
        store.update(connection, id, &next).unwrap();
        let current = store
            .get(connection, id)
            .unwrap()
            .fetch(Default::default())
            .unwrap();
        assert_eq!(current.url, "https://example.com/changed");
        assert_eq!(current.method, "PUT");
        assert_eq!(
            current
                .headers
                .iter()
                .map(|h| (h.name.as_str(), h.value.as_str()))
                .collect::<Vec<_>>(),
            [("x-acme", "first, last"), ("cookie", "AcmeLayer=private")]
        );
        assert_eq!(body(&initial), [1, 2, 3]);
        assert_eq!(body(&middle), [0, 255, 128]);
        let empty = serde_json::from_value(json!({"headers":[],"bodyBase64":""})).unwrap();
        store.update(connection, id, &empty).unwrap();
        assert!(body(&store.get(connection, id).unwrap()).is_empty());
        assert_eq!(body(&middle), [0, 255, 128]);
        assert!(store.update(Uuid::new_v4(), id, &first).is_err());
        store.remove(connection, id).unwrap();
        assert!(!store.update(connection, id, &first).unwrap());
    }
    #[test]
    fn http_request_input_route_capacity_refuses_stale_reads_without_evicting_other_inputs() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        store.insert(connection, context, id, source(&[])).unwrap();
        let old = store.get(connection, id).unwrap();
        let large = source(&vec![0; MAX_BODY]);
        let unrelated = Uuid::new_v4();
        store
            .insert(connection, context, unrelated, large.clone())
            .unwrap();
        while store.bytes + large.bytes() <= MAX_BYTES {
            store
                .insert(connection, context, Uuid::new_v4(), large.clone())
                .unwrap();
        }
        let rest = MAX_BYTES - store.bytes - source(&[]).bytes();
        assert!(rest <= MAX_BODY);
        store
            .insert(connection, context, Uuid::new_v4(), source(&vec![0; rest]))
            .unwrap();
        assert_eq!(store.bytes, MAX_BYTES);
        let changes =
            serde_json::from_value(json!({"bodyBase64":STANDARD.encode(vec![1;65536])})).unwrap();
        assert_eq!(
            store.update(connection, id, &changes).unwrap_err().code,
            "HTTP_REQUEST_INPUT_CAPACITY"
        );
        assert_eq!(
            store.get(connection, id).unwrap_err().code,
            "HTTP_REQUEST_INPUT_CAPACITY"
        );
        assert!(body(&old).is_empty());
        assert_eq!(
            body(&store.get(connection, unrelated).unwrap()).len(),
            MAX_BODY
        );
        assert_eq!(store.bytes, MAX_BYTES);
        assert_eq!(store.summary()["capacityRefusals"], 1);
        store.remove(connection, id).unwrap();
        assert!(store.bytes < MAX_BYTES);
    }
    #[test]
    fn http_request_input_route_bounds_invalidate_future_reads_without_serving_stale_data() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        store.insert(connection, context, id, source(&[1])).unwrap();
        let lease = store.get(connection, id).unwrap();
        let before = store.bytes;
        let changes = serde_json::from_value(
            json!({"url":format!("https://example.com/{}","A".repeat(4096))}),
        )
        .unwrap();
        assert_eq!(
            store.update(connection, id, &changes).unwrap_err().code,
            "HTTP_REQUEST_INPUT_BOUNDS"
        );
        assert_eq!(
            store.get(connection, id).unwrap_err().code,
            "HTTP_REQUEST_INPUT_BOUNDS"
        );
        assert_eq!(body(&lease), [1]);
        assert_eq!(store.bytes, before);
        store.remove(connection, id).unwrap();
        assert_eq!(store.bytes, 0);
    }
    #[test]
    fn http_request_input_store_context_close_disables_capture_without_invalidating_owned_inputs() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        assert!(!store.enabled(context));
        store.set(connection, context, true).unwrap();
        store
            .insert(connection, context, id, source(&[0, 255]))
            .unwrap();
        store.close_context(context);
        assert!(!store.enabled(context));
        assert_eq!(body(&store.get(connection, id).unwrap()), [0, 255]);
        store.set(connection, context, false).unwrap();
        assert!(!store.enabled(context));
        assert!(store.get(connection, id).is_ok());
        assert!(store.set(Uuid::new_v4(), context, true).is_err());
        store.clear();
        assert!(store.get(connection, id).is_err());
    }
    #[test]
    fn http_request_input_store_leases_preserve_bytes_after_explicit_removal_and_clear() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        assert!(
            store
                .insert(connection, context, id, source(&[0, 255, 128]))
                .unwrap()
        );
        let lease = store.get(connection, id).unwrap();
        assert!(store.remove(connection, id).unwrap());
        assert!(!store.remove(connection, id).unwrap());
        assert_eq!(
            store.get(connection, id).unwrap_err().code,
            "HTTP_REQUEST_INPUT_UNKNOWN"
        );
        store.clear();
        assert_eq!(body(&lease), [0, 255, 128]);
        assert_eq!(store.summary()["retainedBytes"], 0);
        assert!(!store.summary().to_string().contains("secret"));
    }
    #[test]
    fn http_request_input_store_rejects_foreign_owners_and_preserves_first_initializer() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let id = Uuid::new_v4();
        let mut store = Store::default();
        store.insert(connection, context, id, source(&[1])).unwrap();
        assert!(!store.insert(connection, context, id, source(&[2])).unwrap());
        assert_eq!(body(&store.get(connection, id).unwrap()), [1]);
        let foreign = Uuid::new_v4();
        assert!(
            store
                .insert(foreign, context, Uuid::new_v4(), source(&[3]))
                .is_err()
        );
        assert!(store.get(foreign, id).is_err());
        assert!(store.remove(foreign, id).is_err());
        let other = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        assert!(store.insert(connection, other, id, source(&[3])).is_err());
        assert_eq!(body(&store.get(connection, id).unwrap()), [1]);
    }
    #[test]
    fn http_request_input_store_capacity_refuses_without_eviction_or_overwriting() {
        let connection = Uuid::new_v4();
        let context = context(connection);
        let mut store = Store::default();
        let snapshot = source(&[]);
        let first = Uuid::new_v4();
        store
            .insert(connection, context, first, snapshot.clone())
            .unwrap();
        for _ in 1..MAX_INPUTS {
            store
                .insert(connection, context, Uuid::new_v4(), snapshot.clone())
                .unwrap();
        }
        assert_eq!(
            store
                .insert(connection, context, Uuid::new_v4(), snapshot)
                .unwrap_err()
                .code,
            "HTTP_REQUEST_INPUT_CAPACITY"
        );
        assert_eq!(store.summary()["retainedInputs"], MAX_INPUTS);
        assert!(store.get(connection, first).is_ok());
        assert_eq!(store.summary()["capacityRefusals"], 1);
        store.clear();
        let large = source(&vec![0; 1048576]);
        let mut accepted = 0;
        while store
            .insert(connection, context, Uuid::new_v4(), large.clone())
            .is_ok()
        {
            accepted += 1;
            assert!(accepted <= 32);
        }
        assert!(accepted > 0);
        assert!(store.bytes <= MAX_BYTES);
        assert_eq!(store.items.len(), accepted);
    }
}
