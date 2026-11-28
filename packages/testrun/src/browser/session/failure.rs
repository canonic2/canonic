//! Payload-free failure witnesses; public outcomes and dispatch policy are unchanged.
use super::{Error, Value, connection, json};

impl Error {
    pub(super) fn failure_evidence(&self) -> Option<Value> {
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(self);
        // A malformed/cyclic external source chain must not pin settlement.
        for _ in 0..16 {
            let error = cause?;
            if let Some(error) = error.downcast_ref::<connection::Error>() {
                return match error {
                    connection::Error::Remote(remote) => Some(json!({
                        "source":"sdk-reply",
                        "kind":match remote.error["error"]["name"].as_str() {
                            Some("TimeoutError") => "timeout",
                            Some("TargetClosedError") => "target-closed",
                            _ => "method-error",
                        },
                        "dispatch":"replied",
                    })),
                    connection::Error::Timeout { dispatch }
                    | connection::Error::Lost { dispatch } => Some(json!({
                        "source":"driver-transport",
                        "kind":if matches!(error,connection::Error::Timeout {..}) {"deadline"} else {"lost"},
                        "dispatch":match dispatch {
                            connection::Dispatch::NotDispatched => "not-dispatched",
                            connection::Dispatch::PossiblyDispatched => "possibly-dispatched",
                        },
                    })),
                    _ => None,
                };
            }
            cause = error.source();
        }
        None
    }
}
