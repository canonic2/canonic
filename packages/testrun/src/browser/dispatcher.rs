//! Versioned wire dispatch, typed object initialization and pending-call correlation.
//!
//! One connection actor owns this state. No I/O or public task authority lives
//! here, and validated protocol coverage does not imply implemented QA workflows.

use super::{
    objects::{self, ObjectRef, ObjectTable},
    schema,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub const MAX_PENDING: usize = 128;
pub const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
const MAX_INITIALIZER_BYTES: usize = 1024 * 1024;
const MAX_INITIALIZERS_BYTES: usize = 16 * 1024 * 1024;
const MAX_ID: u64 = 9_007_199_254_740_991;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Driver dispatcher is closed")]
    Closed,
    #[error("Driver bootstrap has not completed or was already requested")]
    NotReady,
    #[error("Driver pending-call capacity is exhausted")]
    Capacity,
    #[error("Driver request ID space is exhausted")]
    Exhausted,
    #[error("Malformed driver message")]
    Message,
    #[error("Driver reply references a never-issued request")]
    UnknownReply,
    #[error("Driver method is not in the pinned protocol")]
    Method,
    #[error("Driver bootstrap object graph is incompatible")]
    Bootstrap,
    #[error(transparent)]
    Schema(#[from] schema::Error),
    #[error(transparent)]
    Object(#[from] objects::Error),
}

#[derive(Clone, Debug)]
pub struct Bootstrap {
    pub playwright: ObjectRef,
    pub browser_types: BTreeMap<String, ObjectRef>,
}

/// Upstream error payload is private diagnostic data, never a public result.
#[derive(Debug)]
pub struct RemoteError {
    pub error: Value,
    pub details: Option<Value>,
    pub log: Option<Value>,
}

#[derive(Debug)]
pub enum Event {
    Created {
        object: ObjectRef,
        kind: String,
    },
    Adopted {
        object: ObjectRef,
        parent: ObjectRef,
    },
    Disposed {
        object: ObjectRef,
        count: usize,
        reason: Option<String>,
    },
    Message {
        object: ObjectRef,
        method: String,
        params: Option<Value>,
    },
    LateReply {
        id: u64,
    },
}

#[derive(Debug)]
pub enum Incoming {
    Reply {
        id: u64,
        result: Result<Option<Value>, RemoteError>,
    },
    Event(Event),
}

struct Pending {
    kind: String,
    method: String,
}

pub struct Dispatcher {
    objects: ObjectTable,
    initializers: HashMap<ObjectRef, (Option<Value>, usize)>,
    initializer_bytes: usize,
    pending: BTreeMap<u64, Pending>,
    next_id: u64,
    initialize_requested: bool,
    bootstrap: Option<Bootstrap>,
    closed: bool,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self {
            objects: ObjectTable::default(),
            initializers: HashMap::new(),
            initializer_bytes: 0,
            pending: BTreeMap::new(),
            next_id: 1,
            initialize_requested: false,
            bootstrap: None,
            closed: false,
        }
    }
}

impl Dispatcher {
    pub fn root(&self) -> Result<ObjectRef, Error> {
        Ok(self.objects.lookup("", "Root")?)
    }

    pub fn bootstrap(&self) -> Option<&Bootstrap> {
        self.bootstrap.as_ref()
    }

    pub fn resolve(&self, guid: &str, expected: &str) -> Result<ObjectRef, Error> {
        Ok(self.objects.lookup(guid, expected)?)
    }
    pub(crate) fn children(&self, parent: &ObjectRef, kind: &str) -> Result<Vec<ObjectRef>, Error> {
        Ok(self.objects.children(parent, kind)?)
    }
    pub(crate) fn owned_by(&self, child: &ObjectRef, parent: &ObjectRef) -> Result<bool, Error> {
        Ok(self.objects.owned_by(child, parent)?)
    }

    /// Keep a frame's `loadStates` current from its validated `loadstate`
    /// events, as the pinned client does; the driver removes every state when a
    /// new document commits.
    fn apply_load_state(&mut self, frame: &ObjectRef, params: Option<&Value>) {
        let Some(Some(initializer)) = self
            .initializers
            .get_mut(frame)
            .map(|entry| entry.0.as_mut())
        else {
            return;
        };
        let Some(states) = initializer["loadStates"].as_array_mut() else {
            return;
        };
        let params = params.unwrap_or(&Value::Null);
        if let Some(added) = params["add"].as_str()
            && !states.iter().any(|state| state == added)
        {
            states.push(Value::from(added));
        }
        if let Some(removed) = params["remove"].as_str() {
            states.retain(|state| state != removed);
        }
    }
    pub fn initializer(&self, object: &ObjectRef) -> Result<Option<Value>, Error> {
        self.objects.kind(object)?;
        Ok(self
            .initializers
            .get(object)
            .ok_or(Error::Message)?
            .0
            .clone())
    }

    /// Registers the result slot before the returned bytes can enter a writer.
    /// Unknown methods/arguments and capacity failures issue no request ID.
    pub fn admit(
        &mut self,
        object: &ObjectRef,
        method: &str,
        params: Value,
    ) -> Result<(u64, Value), Error> {
        self.admit_inner(object, method, params, false)
    }

    pub fn admit_responsive(
        &mut self,
        object: &ObjectRef,
        method: &str,
        params: Value,
    ) -> Result<(u64, Value), Error> {
        let kind = self.objects.kind(object)?;
        if !matches!(
            (kind, method),
            ("Dialog", "accept" | "dismiss")
                | (
                    "Route",
                    "abort" | "continue" | "fulfill" | "redirectNavigationRequest"
                )
                | ("BindingCall", "resolve" | "reject")
                | (
                    "Page",
                    "screencastFrameAck" | "screencastStop" | "close" | "keyboardUp" | "mouseUp"
                )
                | (
                    "Browser" | "BrowserContext" | "Stream" | "WritableStream",
                    "close"
                )
                | ("JSHandle" | "ElementHandle", "dispose")
                | ("ElementHandle", "setInputFiles")
                | ("Artifact", "cancel" | "delete")
                | ("APIRequestContext", "disposeAPIResponse")
                | ("CDPSession", "detach")
                | (
                    "Debugger",
                    "requestPause" | "resume" | "next" | "enable" | "runTo"
                )
                | (
                    "Tracing",
                    "tracingStop" | "tracingStopChunk" | "tracingGroupEnd"
                )
        ) {
            return Err(Error::Method);
        }
        self.admit_inner(object, method, params, true)
    }

    fn admit_inner(
        &mut self,
        object: &ObjectRef,
        method: &str,
        params: Value,
        responsive: bool,
    ) -> Result<(u64, Value), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let kind = self.objects.kind(object)?.to_owned();
        let initializing = kind == "Root" && method == "initialize";
        if (self.bootstrap.is_none() && !initializing)
            || (initializing && self.initialize_requested)
        {
            return Err(Error::NotReady);
        }
        let key = method_key(&kind, method, "Params")?;
        schema::validate(&key, Some(&params), &self.objects)?;
        schema::encoded_len(&params, MAX_ARGUMENT_BYTES)?;
        if self.pending.len()
            >= if responsive {
                MAX_PENDING
            } else {
                MAX_PENDING - 8
            }
        {
            return Err(Error::Capacity);
        }
        if self.next_id > MAX_ID {
            return Err(Error::Exhausted);
        }
        let id = self.next_id;
        let guid = self.objects.guid(object, &kind)?;
        let message =
            json!({"id":id.to_string(),"guid":guid,"method":method,"params":params,"metadata":{}});
        self.pending.insert(
            id,
            Pending {
                kind,
                method: method.into(),
            },
        );
        self.next_id += 1;
        if initializing {
            self.initialize_requested = true;
        }
        Ok((id, message))
    }

    /// Only work proven not dispatched may retire without a driver witness.
    pub fn retire_unsent(&mut self, id: u64) {
        self.pending.remove(&id);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.objects.mark_lost();
        self.initializers.clear();
        self.initializer_bytes = 0;
        self.pending.clear();
        self.bootstrap = None;
    }

    /// The connection owner treats any error as terminal for this driver epoch.
    pub fn dispatch(&mut self, message: Value) -> Result<Incoming, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if message.get("id").is_some() {
            return self.reply(message);
        }
        fields(&message, &["guid", "method", "params"])?;
        let guid = text(&message, "guid")?;
        let method = text(&message, "method")?;
        let kind = self.objects.wire_kind(guid)?.to_owned();
        let object = self.objects.lookup(guid, &kind)?;
        let params = message.get("params");
        let event = match method {
            "__create__" => self.create(object, params.ok_or(Error::Message)?)?,
            "__adopt__" => {
                let params = params.ok_or(Error::Message)?;
                fields(params, &["guid"])?;
                let guid = text(params, "guid")?;
                let child = self.objects.lookup(guid, self.objects.wire_kind(guid)?)?;
                self.objects.adopt(&child, &object)?;
                Event::Adopted {
                    object: child,
                    parent: object,
                }
            }
            "__dispose__" => {
                let params = params.ok_or(Error::Message)?;
                fields(params, &["reason"])?;
                let reason = params
                    .get("reason")
                    .map(|value| {
                        value
                            .as_str()
                            .filter(|text| text.len() <= 1024)
                            .map(str::to_owned)
                            .ok_or(Error::Message)
                    })
                    .transpose()?;
                let count = self.objects.dispose(&object)?;
                self.initializers.retain(|reference, (_, bytes)| {
                    if self.objects.kind(reference).is_ok() {
                        true
                    } else {
                        self.initializer_bytes -= *bytes;
                        false
                    }
                });
                if let Some(bootstrap) = &self.bootstrap {
                    self.objects
                        .kind(&bootstrap.playwright)
                        .map_err(|_| Error::Bootstrap)?;
                    for object in bootstrap.browser_types.values() {
                        self.objects.kind(object).map_err(|_| Error::Bootstrap)?;
                    }
                }
                Event::Disposed {
                    object,
                    count,
                    reason,
                }
            }
            _ => {
                schema::validate(&method_key(&kind, method, "Event")?, params, &self.objects)?;
                if kind == "Frame" && method == "loadstate" {
                    self.apply_load_state(&object, params);
                }
                Event::Message {
                    object,
                    method: method.into(),
                    params: params.cloned(),
                }
            }
        };
        Ok(Incoming::Event(event))
    }

    fn create(&mut self, parent: ObjectRef, params: &Value) -> Result<Event, Error> {
        fields(params, &["guid", "type", "initializer"])?;
        let guid = text(params, "guid")?;
        let kind = text(params, "type")?;
        let initializer = params.get("initializer");
        schema::validate(&format!("{kind}Initializer"), initializer, &self.objects)?;
        let bytes = initializer
            .map(|value| schema::encoded_len(value, MAX_INITIALIZER_BYTES))
            .transpose()?
            .unwrap_or(0);
        if bytes > MAX_INITIALIZERS_BYTES - self.initializer_bytes {
            return Err(schema::Error::Bounds.into());
        }
        let object = self.objects.create(&parent, guid, kind)?;
        self.initializers
            .insert(object.clone(), (initializer.cloned(), bytes));
        self.initializer_bytes += bytes;
        Ok(Event::Created {
            object,
            kind: kind.into(),
        })
    }

    fn reply(&mut self, message: Value) -> Result<Incoming, Error> {
        fields(&message, &["id", "result", "error", "errorDetails", "log"])?;
        let id = message["id"]
            .as_u64()
            .or_else(|| {
                let text = message["id"].as_str()?;
                if text.len() > 16 || text.starts_with('0') {
                    return None;
                }
                text.parse::<u64>().ok().filter(|id| id.to_string() == text)
            })
            .filter(|id| *id > 0 && *id <= MAX_ID)
            .ok_or(Error::Message)?;
        let Some(pending) = self.pending.get(&id) else {
            return if id < self.next_id {
                Ok(Incoming::Event(Event::LateReply { id }))
            } else {
                Err(Error::UnknownReply)
            };
        };
        let result = if let Some(error) = message.get("error") {
            if message.get("result").is_some() {
                return Err(Error::Message);
            }
            schema::validate("SerializedError", Some(error), &self.objects)?;
            schema::encoded_len(error, 64 * 1024)?;
            if let Some(log) = message.get("log") {
                let lines = log.as_array().ok_or(Error::Message)?;
                if lines.iter().any(|line| !line.is_string()) {
                    return Err(Error::Message);
                }
                schema::encoded_len(log, 64 * 1024)?;
            }
            let details_schema = method_key(&pending.kind, &pending.method, "ErrorDetails").ok();
            if let Some(name) = details_schema.filter(|name| schema::contains(name)) {
                // The official client validates absent details as an empty
                // object. Keep the raw field's absence in private diagnostics.
                let empty = json!({});
                schema::validate(
                    &name,
                    Some(message.get("errorDetails").unwrap_or(&empty)),
                    &self.objects,
                )?;
            } else if message
                .get("errorDetails")
                .is_some_and(|value| value != &json!({}))
            {
                return Err(Error::Message);
            }
            Err(RemoteError {
                error: error.clone(),
                details: message.get("errorDetails").cloned(),
                log: message.get("log").cloned(),
            })
        } else {
            schema::validate(
                &method_key(&pending.kind, &pending.method, "Result")?,
                message.get("result"),
                &self.objects,
            )?;
            if pending.kind == "Root" && pending.method == "initialize" {
                self.bootstrap =
                    Some(self.verify_bootstrap(message.get("result").ok_or(Error::Bootstrap)?)?);
            }
            Ok(message.get("result").cloned())
        };
        self.pending.remove(&id);
        Ok(Incoming::Reply { id, result })
    }

    fn verify_bootstrap(&self, result: &Value) -> Result<Bootstrap, Error> {
        let playwright = self
            .objects
            .lookup(text(&result["playwright"], "guid")?, "Playwright")?;
        let initializer = self.initializer(&playwright)?.ok_or(Error::Bootstrap)?;
        let mut browser_types = BTreeMap::new();
        for name in ["chromium", "firefox", "webkit"] {
            let object = self
                .objects
                .lookup(text(&initializer[name], "guid")?, "BrowserType")?;
            let data = self.initializer(&object)?.ok_or(Error::Bootstrap)?;
            if data["name"] != name {
                return Err(Error::Bootstrap);
            }
            browser_types.insert(name.into(), object);
        }
        Ok(Bootstrap {
            playwright,
            browser_types,
        })
    }
}

fn method_key(kind: &str, method: &str, suffix: &str) -> Result<String, Error> {
    if method.is_empty()
        || method.len() > 128
        || !method.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(Error::Method);
    }
    let mut method = method.to_owned();
    method[..1].make_ascii_uppercase();
    let key = format!("{kind}{method}{suffix}");
    if !schema::contains(&key) {
        return Err(Error::Method);
    }
    Ok(key)
}

fn text<'a>(object: &'a Value, key: &str) -> Result<&'a str, Error> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Message)
}

fn fields(object: &Value, allowed: &[&str]) -> Result<(), Error> {
    if object
        .as_object()
        .is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
    {
        Ok(())
    } else {
        Err(Error::Message)
    }
}

#[cfg(test)]
pub(crate) mod tests;
