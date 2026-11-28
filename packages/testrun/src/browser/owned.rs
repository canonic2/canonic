//! Backend selection for the shared named-session browser lifetime.

use super::{
    cdp,
    contract::{LaunchOptions, Request},
    driver::DiagnosticBundle,
    lifecycle::Operation,
    preparation::ChromeBundle,
    profiles::Profile,
    session::{self, Error},
};
use serde_json::Value;
use std::path::Path;
use tokio::time::Instant;
use uuid::Uuid;

pub(super) enum Bundle {
    Playwright(DiagnosticBundle),
    Chrome {
        bundle: ChromeBundle,
        disposable_profile: bool,
    },
    Attachment(super::attachment::Attachment),
}

#[derive(Clone)]
pub(crate) enum Session {
    Playwright(session::Session),
    Chrome(cdp::session::Session),
}

impl Session {
    pub(super) fn launch_connection(
        bundle: Bundle,
        connection: Uuid,
        owner: Uuid,
        generation: Uuid,
        options: LaunchOptions,
        profile: Option<Profile>,
        directory: &Path,
    ) -> Result<Self, Error> {
        match bundle {
            Bundle::Playwright(bundle) => session::Session::launch_selected(
                session::BackendStart::Launch(bundle),
                connection,
                owner,
                generation,
                options,
                profile,
                directory,
            )
            .map(Self::Playwright),
            Bundle::Chrome {
                bundle,
                disposable_profile,
            } => cdp::session::Session::launch_selected(
                cdp::session::BackendStart::Launch {
                    bundle,
                    disposable_profile,
                },
                connection,
                owner,
                generation,
                options,
                profile,
                directory,
            )
            .map(Self::Chrome),
            Bundle::Attachment(attachment) if options.engine == super::contract::Engine::Chrome => {
                cdp::session::Session::launch_selected(
                    cdp::session::BackendStart::Attach(attachment),
                    connection,
                    owner,
                    generation,
                    options,
                    None,
                    directory,
                )
                .map(Self::Chrome)
            }
            Bundle::Attachment(attachment) => session::Session::launch_selected(
                session::BackendStart::Attach(attachment),
                connection,
                owner,
                generation,
                options,
                None,
                directory,
            )
            .map(Self::Playwright),
        }
    }
    pub(crate) fn startup(&self) -> Uuid {
        match self {
            Self::Playwright(s) => s.startup(),
            Self::Chrome(s) => s.startup(),
        }
    }
    pub(crate) fn status(&self) -> Value {
        match self {
            Self::Playwright(s) => s.status(),
            Self::Chrome(s) => s.status(),
        }
    }
    pub(crate) fn compact_status(&self, limit: usize) -> Value {
        match self {
            Self::Playwright(s) => s.compact_status(limit),
            Self::Chrome(s) => s.compact_status(limit),
        }
    }
    pub(crate) fn pending_operation(&self) -> Option<Uuid> {
        match self {
            Self::Playwright(s) => s.pending_operation(),
            Self::Chrome(s) => s.pending_operation(),
        }
    }
    pub(crate) fn operation(&self, id: Uuid) -> Option<Operation> {
        match self {
            Self::Playwright(s) => s.operation(id),
            Self::Chrome(s) => s.operation(id),
        }
    }
    pub(crate) fn control_status(&self) -> Value {
        match self {
            Self::Playwright(s) => s.control_status(),
            Self::Chrome(s) => s.control_status(),
        }
    }
    pub(crate) fn diagnostic_operation(
        &self,
        id: Uuid,
        request: &Request,
    ) -> crate::error::Result<Value> {
        match self {
            Self::Playwright(s) => s.diagnostic_operation(id, request),
            Self::Chrome(s) => s.diagnostic_operation(id, request),
        }
    }
    pub(crate) async fn change_control(&self, name: &str, args: Value) -> Result<Value, Error> {
        match self {
            Self::Playwright(s) => s.change_control(name, args).await,
            Self::Chrome(s) => s.change_control(name, args).await,
        }
    }
    pub(crate) fn submit(
        &self,
        id: Uuid,
        request: Request,
        timeout_ms: u64,
    ) -> Result<Uuid, Error> {
        match self {
            Self::Playwright(s) => s.submit(id, request, timeout_ms),
            Self::Chrome(s) => s.submit(id, request, timeout_ms),
        }
    }
    pub(crate) async fn cancel(&self, id: Uuid) -> Result<(), Error> {
        match self {
            Self::Playwright(s) => s.cancel(id).await,
            Self::Chrome(s) => s.cancel(id).await,
        }
    }
    pub(crate) async fn wait(&self, id: Uuid, deadline: Instant) -> Result<Operation, Error> {
        match self {
            Self::Playwright(s) => s.wait(id, deadline).await,
            Self::Chrome(s) => s.wait(id, deadline).await,
        }
    }
    pub(crate) fn disconnect(&self, timeout_ms: u64) -> Result<Uuid, Error> {
        match self {
            Self::Playwright(s) => s.disconnect(timeout_ms),
            Self::Chrome(s) => s.disconnect(timeout_ms),
        }
    }
}
