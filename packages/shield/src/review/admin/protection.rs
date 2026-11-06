//! Exact protected-file access review shared by terminal and MCP adapters.
use crate::review::{
    Checkpoint, Context, Error,
    path_access::{PathAccess, load},
    project,
    storage::Store,
};
use serde_json::{Value, json};
use std::path::Path;

pub(in crate::review) struct Prepared {
    root: String,
    before: Value,
    after: PathAccess,
    changed: bool,
}

impl Prepared {
    pub(in crate::review) fn new(context: &Context, after: PathAccess) -> Result<Self, Error> {
        let metadata = project::metadata(&project::checkpoint(context)?);
        Ok(Self::from_metadata(context, &metadata, after))
    }

    pub(in crate::review) fn from_metadata(
        context: &Context,
        metadata: &Value,
        after: PathAccess,
    ) -> Self {
        let changed = load(metadata).map_or(true, |before| before != after);
        Self {
            root: context.project_root.clone(),
            before: metadata["path_access"].clone(),
            after,
            changed,
        }
    }

    pub(in crate::review) fn review(&self) -> Value {
        json!({"operation":"change protected file access","before":self.before,"after":self.after,
            "fixed_protection":[".shield/review","compiled caches",".shield folder",".shield/.gitignore"],
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
                if state.operator["path_access"] != self.before {
                    return Err(Error::StaleSnapshot);
                }
                if self.changed {
                    state.operator["path_access"] = json!(self.after);
                }
                Ok(self.changed)
            },
        )?;
        Ok(result)
    }
}
