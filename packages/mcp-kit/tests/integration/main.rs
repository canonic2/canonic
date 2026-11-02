//! Drives the stdio server through in-memory pipes, as a host would.

mod completion;
mod confirmation;
mod disconnection;
#[cfg(feature = "docs-build")]
mod docs;
mod elicitation;
mod fixture;
mod lifecycle;
mod modern;
mod pagination;
mod prompts;
mod resource_content;
mod resources;
mod scheduling;
mod schema;
mod skills;
#[cfg(unix)]
mod tasks;
