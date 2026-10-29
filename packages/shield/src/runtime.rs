//! Owned active-policy consumers and revision-checked local handoff.
//!
//! This API binds local retained policy and request evidence. It does not establish
//! a protected operator boundary, one-time grants, or launch permission. Consumers
//! implement their applicable enforcement contracts separately. Checked execution
//! validates resources and launches it owns.
//! [`crate::enforcement`] adds allow-only handoff and classified access restrictions;
//! integrations must map submitted requests and intercept their applicable paths.
//!
//! ```no_run
//! use shield::{policy::Request, runtime::{Error, Project}};
//! use std::path::Path;
//!
//! let project = Project::open(Path::new("/path/to/acme"))?;
//! let snapshot = project.snapshot()?;
//! let root = snapshot.identity().project_root().to_str().expect("UTF-8 project");
//! let evidence = snapshot.evaluate(Request {
//!     executable: "acme".into(), arguments: vec!["read".into()],
//!     cwd: root.into(), project: root.into(),
//! })?;
//! let decision = project.with_current(&evidence, |_, evaluation| evaluation.decision)?;
//! // This is diagnostic evidence, not a reusable execution grant.
//! # let _ = decision;
//! # Ok::<(), Error>(())
//! ```
pub use crate::review::runtime::consumer::{
    EvaluatedRequest, Project, RuntimeError as Error, Snapshot, SnapshotIdentity,
};
