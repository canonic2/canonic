//! Final authenticated request validation, shared by atomic approval consumers.
use super::{EvaluatedRequest, NativeSource, SnapshotIdentity};
use crate::review::{Checkpoint, Context, Error, storage::Store};
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// Opaque shared source authority. Retain it through durable consumption and the
/// final owned handoff; it grants no operation without exact request validation.
pub(crate) struct EvaluatedAuthority {
    identity: SnapshotIdentity,
    modes: Value,
    installed: Value,
    _registry: crate::review::registry_trust::Guard,
}

impl EvaluatedRequest {
    pub(crate) fn authorization(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<EvaluatedAuthority, Error> {
        checkpoint()?;
        if self.identity.project_id != context.project_id
            || self.identity.project_root != context.project_root
        {
            return Err(Error::StaleSnapshot);
        }
        let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
        state.check(context)?;
        NativeSource {
            identity: self.identity.clone(),
            evaluator: Arc::clone(&self.evaluator),
        }
        .check(&state)?;
        // Global trust precedes the consumer's project authority transaction.
        let registry = crate::review::registry_trust::guard_effective(
            &state.operator,
            self.evaluator.effective(),
        )?;
        if registry.revision != self.identity.registry_revision {
            return Err(Error::StaleSnapshot);
        }
        checkpoint()?;
        Ok(EvaluatedAuthority {
            identity: self.identity.clone(),
            modes: state.operator["registry_trust"].clone(),
            installed: state.operator["installed"].clone(),
            _registry: registry,
        })
    }
}

impl EvaluatedAuthority {
    /// Called inside the existing durable mutation or handoff transaction. This
    /// never acquires project authority recursively or reloads policy sources.
    pub(crate) fn check_locked(
        &self,
        state: &super::super::State,
        evaluated: &EvaluatedRequest,
        checkpoint: Checkpoint<'_>,
    ) -> Result<(), Error> {
        checkpoint()?;
        if self.identity != evaluated.identity
            || state.operator["registry_trust"] != self.modes
            || state.operator["installed"] != self.installed
        {
            return Err(Error::StaleSnapshot);
        }
        NativeSource {
            identity: self.identity.clone(),
            evaluator: Arc::clone(&evaluated.evaluator),
        }
        .check(state)?;
        if evaluated.time_sensitive {
            let current = if let Some(resource) = &evaluated.resource {
                evaluated.evaluator.evaluate_resource_mcp(
                    &evaluated.request,
                    &resource.input,
                    &resource.context,
                    resource.mcp.as_deref(),
                    checkpoint,
                )?
            } else {
                evaluated
                    .evaluator
                    .evaluate_controlled(&evaluated.request, checkpoint)?
            };
            if current.decision != evaluated.evaluation.decision {
                return Err(Error::StaleSnapshot);
            }
        }
        Ok(checkpoint()?)
    }
}
