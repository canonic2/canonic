//! Frozen operator review for one location-independent read-only SDK profile.
use super::{Profile, binary, current, files, mapping, model::PROFILE_LIMIT};
use crate::review::{Checkpoint, Context, Error, storage::objects};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) struct Prepared {
    project_id: String,
    root: String,
    profile: Profile,
    binary: binary::Receipt,
}

impl Prepared {
    pub(crate) fn new(context: &Context, adapter: &str) -> Result<Self, Error> {
        super::ensure_supported()?;
        if adapter != mapping::ID {
            return Err(Error::Invalid("SDK adapter must be claude-read-only"));
        }
        let state = current(Path::new(&context.project_root))?;
        state.check(context)?;
        let binary = binary::Receipt::capture()?;
        Ok(Self {
            project_id: state.project_id.clone(),
            root: state.project_root.clone(),
            profile: Profile {
                lineage: state.policy_lineage().into(),
                binary_digest: binary.digest().into(),
                mapping_digest: mapping::digest().into(),
            },
            binary,
        })
    }

    pub(crate) fn review(&self) -> Value {
        json!({
            "operation":"register fixed read-only SDK adapter",
            "adapter":mapping::ID,
            "protocol":"general-runtime-v1",
            "policy_lineage":self.profile.lineage,
            "binary_digest":self.profile.binary_digest,
            "mapping_digest":self.profile.mapping_digest,
            "mapping_files":mapping::paths(),
            "permission":"Only the registered plain-text Read mapping may run. Policy Deny blocks; policy Ask requires an independently reviewed exact-read approval. Shells, writes and unknown operations remain unsupported.",
            "consent_scope":"This developer and policy lineage, reusable across locations only for these exact binary and adapter mapping contents.",
            "runtime_context":"Each factory obtains a private context for this checkout that lasts while the factory owns its native lease. At most 128 live contexts are allowed per developer. A copied or moved checkout cannot replay it.",
            "policy_effect":"No policy permission, native integration or legacy SDK registration changes."
        })
    }

    /// Only the operator adapter calls this after its trusted terminal review.
    pub(crate) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        if context.surface != "cli" {
            return Err(Error::ApprovalRequired);
        }
        checkpoint()?;
        self.binary.check_current()?;
        let state = current(Path::new(&context.project_root))?;
        state.check(context)?;
        if state.project_id != self.project_id
            || state.project_root != self.root
            || state.policy_lineage() != self.profile.lineage
            || self.profile.mapping_digest != mapping::digest()
        {
            return Err(Error::StaleSnapshot);
        }
        let key = self.profile.key()?;
        let directory = files::directory("sdk-read-profiles", true)?;
        let _lease = objects::Directory::enter(&directory)?;
        let mut index = files::profiles(&directory)?;
        if !index.profiles.contains_key(&key) && index.profiles.len() >= PROFILE_LIMIT {
            return Err(Error::Overloaded);
        }
        let receipt = json!({"schema_version":1,"adapter":mapping::ID,"profile":key,
            "binary_digest":self.profile.binary_digest,
            "mapping_digest":self.profile.mapping_digest,
            "policy_lineage":self.profile.lineage,
            "status":"registered","policy_changed":false});
        index.profiles.insert(key, self.profile);
        files::write(&directory, &index)?;
        Ok(receipt)
    }
}
