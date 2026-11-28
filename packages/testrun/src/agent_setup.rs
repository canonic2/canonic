//! Testrun's public setup API delegates native registration to Agent Kit.
#[cfg(test)]
pub use canonic_agent_kit::Scope;
pub use canonic_agent_kit::{Context, Error, Options, Result, Tool, human, run};
