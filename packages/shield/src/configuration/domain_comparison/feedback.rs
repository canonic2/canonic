//! Structural permission identity keeps source admission separate from consent.
use crate::{
    configuration::{Artifact, LockDocument, LockedPack, Snapshot},
    policy::{self, Scope},
};
use serde::{Serialize, Serializer, ser::SerializeSeq};

struct ArtifactPath<'a>(&'a Artifact);
impl Serialize for ArtifactPath<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Artifact { path, digest: _ } = self.0;
        path.serialize(serializer)
    }
}
struct Selections<'a>(&'a [LockedPack]);
impl Serialize for Selections<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for selection in self.0 {
            let LockedPack {
                id,
                version,
                profile,
                artifact,
            } = selection;
            sequence.serialize_element(&(id, version, profile, ArtifactPath(artifact)))?;
        }
        sequence.end()
    }
}
struct Header<'a>(&'a LockDocument);
impl Serialize for Header<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Raw selected-artifact digests are represented by the separately
        // validated typed permission identities, not by their feedback bytes.
        let LockDocument {
            schema_version,
            kind,
            config_digest,
            packs,
            rules,
            access_paths,
            local_rules,
            disabled,
            mcp,
        } = self.0;
        (
            schema_version,
            kind,
            config_digest,
            Selections(packs),
            rules.as_ref().map(ArtifactPath),
            access_paths,
            local_rules.as_ref().map(ArtifactPath),
            disabled,
            mcp,
        )
            .serialize(serializer)
    }
}
pub(super) fn equivalent(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    inspection: &policy::feedback::Inspection,
) -> &'static str {
    if !inspection.summary.changed {
        return if inspection.unavailable_reason == Some("feedback-work-budget") {
            "feedback-work-budget"
        } else {
            "feedback-unchanged"
        };
    }
    let Some((first, second)) = inspection.before.as_ref().zip(inspection.after.as_ref()) else {
        return inspection
            .unavailable_reason
            .unwrap_or("source-evidence-unavailable");
    };
    if first.len() != second.len()
        || !first
            .iter()
            .zip(second)
            .all(|(first, second)| first.same(second))
    {
        return "permission-structure-differs";
    }
    let ([(first_scope, first)], [(second_scope, second)]) = (before, after) else {
        return "composition-differs";
    };
    let Some(first) = policy::feedback::descriptor(
        &(*first_scope, Header(&first.lock.document)),
        inspection.remaining,
    ) else {
        return "composition-budget";
    };
    let remaining = inspection
        .remaining
        .saturating_sub(policy::feedback::byte_len(&first));
    let Some(second) =
        policy::feedback::descriptor(&(*second_scope, Header(&second.lock.document)), remaining)
    else {
        return "composition-budget";
    };
    if first.same(&second) {
        "proved"
    } else {
        "composition-differs"
    }
}

pub(super) fn custom_decision_bound(
    before: &[(Scope, Snapshot)],
    after: &[(Scope, Snapshot)],
    budget: usize,
) -> Option<policy::profile_comparison::Bound> {
    let ([(first_scope, first)], [(second_scope, second)]) = (before, after) else {
        return None;
    };
    if !first.packs.is_empty()
        || !second.packs.is_empty()
        || first.lock.document.local_rules.is_some()
        || second.lock.document.local_rules.is_some()
    {
        return None;
    }
    let first_header =
        policy::feedback::descriptor(&(*first_scope, Header(&first.lock.document)), budget)?;
    let remaining = budget.checked_sub(policy::feedback::byte_len(&first_header))?;
    let second_header =
        policy::feedback::descriptor(&(*second_scope, Header(&second.lock.document)), remaining)?;
    let remaining = remaining.checked_sub(policy::feedback::byte_len(&second_header))?;
    if !first_header.same(&second_header) {
        return None;
    }
    let (Some(first), Some(second)) = (first.custom.as_ref(), second.custom.as_ref()) else {
        return None;
    };
    policy::feedback::custom_bound(first, second, remaining)
}
