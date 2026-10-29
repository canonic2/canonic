//! Model Context Protocol server building blocks.
//!
//! The library has two layers. The protocol core ([`Session`], [`rpc`],
//! [`tool`], [`resource`], [`skill`], [`docs`], [`modern`], [`schema`], [`json`] and [`control`]) performs no I/O and starts
//! no threads, so any runtime can drive it. The [`stdio`] driver runs a
//! [`stdio::Handler`] over newline-delimited JSON-RPC with bounded lanes,
//! deadlines, cancellation, progress, host confirmation and disconnection
//! rules. The `docs-build` feature adds the only file-reading part: a
//! build-script helper that bundles a server's guides for [`docs`].
//!
//! A product supplies its tool [`Catalog`], server identity and tool
//! implementations. The library never interprets tool arguments beyond their
//! input schema, never retries work and never grants authority: a host
//! confirmation only reports what the host's user chose.
//!
//! ```
//! use canonic_mcp::{Catalog, Inbound, ServerInfo, Session, SessionOptions, Tool};
//! use serde_json::json;
//! use std::sync::Arc;
//!
//! let catalog = Catalog::new(vec![Tool::new(
//!     "acme_status",
//!     "Return the Acme service status.",
//!     json!({"type": "object", "additionalProperties": false}),
//! )])?;
//! let mut session = Session::new(Arc::new(catalog), SessionOptions::default());
//! let frame = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-host","version":"1.0"}}}"#;
//! let Inbound::Initialize { id, client } = session.receive(frame) else { unreachable!() };
//! assert_eq!(client.name, "acme-host");
//! let reply = session.accept_initialize(id, &ServerInfo::new("acme", "1.0.0"));
//! assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
//! # Ok::<(), canonic_mcp::CatalogError>(())
//! ```

pub mod control;
pub mod docs;
pub mod json;
pub mod modern;
pub mod resource;
pub mod rpc;
pub mod schema;
mod session;
pub mod skill;
pub mod stdio;
pub mod tool;

pub use control::{Control, Interrupted};
pub use modern::{CachePolicy, CacheScope, InputRequiredResult, ModernOptions, RequestContext};
pub use resource::{
    ReadResourceResult, Resource, ResourceCatalog, ResourceContents, ResourceError,
    ResourceTemplate,
};
pub use rpc::RpcError;
pub use session::{
    Client, ConfigurationError, DEFAULT_MAX_MESSAGE_BYTES, Decision, Inbound,
    LATEST_PROTOCOL_VERSION, MAX_CLIENT_FIELD_BYTES, MAX_VERSION_BYTES,
    SUPPORTED_PROTOCOL_VERSIONS, ServerInfo, Session, SessionOptions,
};
pub use skill::{BundledRead, SkillCatalog, SkillDefinition, SkillError, SkillFile};
pub use tool::{Annotations, Catalog, CatalogError, Content, Tool, ToolCall, ToolResult};
