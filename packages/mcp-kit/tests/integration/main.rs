//! Drives the stdio server through in-memory pipes, as a host would, and the
//! HTTP exchange from an independent runtime without sockets.

mod completion;
mod confirmation;
mod contention;
mod disconnection;
#[cfg(feature = "docs-build")]
mod docs;
mod elicitation;
mod fixture;
mod http_authorization;
mod http_exchange;
mod lifecycle;
mod metadata;
mod modern;
mod observation;
mod pagination;
mod prompts;
mod resource_content;
mod resources;
mod retained_session;
mod scaling;
mod scheduling;
mod schema;
mod skills;
mod subscriptions;
#[cfg(unix)]
mod tasks;
