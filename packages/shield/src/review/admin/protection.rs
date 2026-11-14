//! Exact protected-file access review shared by terminal and MCP adapters.
//! The choice is the `protection` key of the policy files.
use crate::review::{
    Checkpoint, Context, Error, path_access::PathAccess, project, settings, storage::Store,
};
use serde_json::{Value, json};
use std::path::Path;

pub(in crate::review) struct Prepared {
    root: String,
    before: PathAccess,
    after: PathAccess,
    changed: bool,
}

impl Prepared {
    pub(in crate::review) fn new(context: &Context, after: PathAccess) -> Result<Self, Error> {
        let state = project::checkpoint(context)?;
        Ok(Self::from_current(
            context,
            settings::path_access(&state.settings),
            after,
        ))
    }

    /// A review from the access in effect when it was read.
    pub(in crate::review) fn from_current(
        context: &Context,
        before: PathAccess,
        after: PathAccess,
    ) -> Self {
        Self {
            root: context.project_root.clone(),
            before,
            after,
            changed: before != after,
        }
    }

    pub(in crate::review) fn review(&self) -> Value {
        json!({"operation":"change protected file access","before":self.before,"after":self.after,
            "fixed_protection":[".shield/state","compiled caches",".shield folder",".shield/.gitignore"],
            "warning":"Allowing changes to agent settings permits removal of Shield hooks and host confirmation settings."})
    }

    pub(in crate::review) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        if context.project_root != self.root {
            return Err(Error::StaleSnapshot);
        }
        let result = json!({"schema_version":1,"operation":"protected file access","path_access":self.after,"changed":self.changed});
        Store::open(Path::new(&context.project_root))?.update_checkpoint_if_changed(
            context,
            checkpoint,
            |state| {
                if settings::path_access(&state.settings) != self.before {
                    return Err(Error::StaleSnapshot);
                }
                if self.changed {
                    settings::write(context, &settings::protection(self.after))?;
                }
                Ok(false)
            },
        )?;
        Ok(result)
    }
}
