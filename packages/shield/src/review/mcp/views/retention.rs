//! Pressure-driven eviction of disposable captures; live reviews keep exact pins.
use super::{Context, Error, HANDLE_BYTES, Path, Store, Value, Views, configuration, identity};
use std::collections::BTreeSet;

impl Views {
    pub(in crate::review::mcp) fn retain_pair(
        &mut self,
        context: &Context,
        before: (&Value, &Value),
        after: (&Value, &Value),
        plans: Option<(configuration::Evaluator, configuration::Evaluator)>,
    ) -> Result<(String, String), Error> {
        let (base_plan, next_plan) = plans.map_or((None, None), |(a, b)| (Some(a), Some(b)));
        let base = self.retain_prepared(context, before.0, before.1, base_plan, None, None)?;
        let next = self.retain_prepared(context, after.0, after.1, next_plan, None, Some(&base))?;
        Ok((base, next))
    }

    pub(in crate::review::mcp) fn retain_recorded_pair(
        &mut self,
        context: &Context,
        before: (&Value, &Value),
        after: (&Value, &Value),
        before_revision: &str,
        after_revision: Option<&str>,
    ) -> Result<(String, String), Error> {
        let base = self.retain_prepared(
            context,
            before.0,
            before.1,
            None,
            Some(before_revision),
            None,
        )?;
        let next =
            self.retain_prepared(context, after.0, after.1, None, after_revision, Some(&base))?;
        Ok((base, next))
    }

    pub(super) fn handle_room(
        &mut self,
        context: &Context,
        bytes: usize,
        slots: usize,
        keep: &[Option<&str>],
    ) -> Result<(), Error> {
        if bytes > HANDLE_BYTES || slots > 256 {
            return Err(Error::Overloaded);
        }
        if self.handles.len() + slots <= 256 && self.bytes.saturating_add(bytes) <= HANDLE_BYTES {
            return Ok(());
        }
        // Read required authority only under pressure. A handle cache never owns
        // proposal lifetime and cannot decide that an outstanding review ended.
        let (state, _) =
            Store::inspect(Path::new(&context.project_root))?.read_diagnostic_checkpoint()?;
        state.check_inspection(context)?;
        let pinned: BTreeSet<_> = state
            .proposals
            .iter()
            .filter(|row| row.status == "pending")
            .flat_map(|row| {
                [
                    identity(&row.base_candidate, &row.base_metadata),
                    identity(&row.candidate, &row.candidate_metadata),
                ]
            })
            .collect();
        let mut victims: Vec<_> = self
            .handles
            .iter()
            .filter(|(id, handle)| {
                !keep.contains(&Some(id.as_str())) && !pinned.contains(&handle.key)
            })
            .map(|(id, handle)| (handle.used, id.clone()))
            .collect();
        victims.sort_unstable();
        // Check feasibility before changing the cache or invalidating any ID.
        let available: usize = victims.iter().map(|(_, id)| self.handles[id].bytes).sum();
        if self.handles.len() + slots > 256 + victims.len()
            || self.bytes.saturating_sub(available).saturating_add(bytes) > HANDLE_BYTES
        {
            return Err(Error::Overloaded);
        }
        for (_, id) in victims {
            if self.handles.len() + slots <= 256 && self.bytes.saturating_add(bytes) <= HANDLE_BYTES
            {
                break;
            }
            let handle = self
                .handles
                .remove(&id)
                .ok_or(Error::CandidateUnavailable)?;
            self.bytes -= handle.bytes;
            self.identities.remove(&handle.key);
            crate::diagnostics::trace::count("handle.reclaimed", 1);
        }
        Ok(())
    }
}
