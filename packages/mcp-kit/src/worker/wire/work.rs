//! Borrowed assignment projection shared by admission measurement and owned work.
use super::{Request, RpcError, invalid};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

#[derive(Serialize)]
pub(super) struct View<'a> {
    pub(super) kind: &'static str,
    pub(super) params: Params<'a>,
}

impl<'a> View<'a> {
    pub(super) fn new(request: &'a Request) -> Result<Self, RpcError> {
        let kind = match request {
            Request::Tool(_) => "tool",
            Request::Prompt(_) => "prompt",
            Request::Completion(_) => "completion",
            Request::Task(task) => task.method(),
            Request::Method { .. } => "method",
            Request::Notification { .. } => "notification",
            Request::Resource(_) => return Err(invalid()),
        };
        Ok(Self {
            kind,
            params: Params(request),
        })
    }
}

pub(super) struct Params<'a>(&'a Request);

impl Serialize for Params<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Progress<'a> {
            #[serde(rename = "progressToken")]
            token: &'a Value,
        }
        #[derive(Serialize)]
        struct Arguments<'a> {
            arguments: &'a std::collections::BTreeMap<String, String>,
        }
        let mut map = serializer.serialize_map(None)?;
        let token = match self.0 {
            Request::Tool(call) => {
                map.serialize_entry("name", &call.name)?;
                map.serialize_entry("arguments", &call.arguments)?;
                call.progress_token.as_ref()
            }
            Request::Prompt(call) => {
                map.serialize_entry("name", &call.name)?;
                map.serialize_entry("arguments", &call.arguments)?;
                call.progress_token.as_ref()
            }
            Request::Completion(call) => {
                map.serialize_entry("ref", &call.reference)?;
                map.serialize_entry("argument", &call.argument)?;
                map.serialize_entry(
                    "context",
                    &Arguments {
                        arguments: &call.context,
                    },
                )?;
                call.progress_token.as_ref()
            }
            Request::Task(task) => {
                map.serialize_entry("taskId", task.id())?;
                if let crate::task::TaskRequest::Update { responses, .. } = task {
                    map.serialize_entry("inputResponses", responses)?;
                }
                None
            }
            Request::Method { method, params } | Request::Notification { method, params } => {
                map.serialize_entry("method", method)?;
                map.serialize_entry("params", params)?;
                None
            }
            Request::Resource(_) => {
                return Err(serde::ser::Error::custom("frontend-owned resource"));
            }
        };
        if let Some(token) = token {
            map.serialize_entry("_meta", &Progress { token })?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
