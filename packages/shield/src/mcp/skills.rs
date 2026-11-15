//! Immutable distributable workflows, independent of enrolled project state.
//!
//! Each bundled skill is a `SkillDefinition` whose files are embedded from
//! `skills/<name>/` with `include_str!`; `build.rs` checks those folders.
use canonic_mcp_kit::{SkillCatalog, SkillDefinition, SkillError, SkillFile};

/// Each skill's URI and its `SKILL.md` source.
pub(super) const SOURCES: [(&str, &str); 4] = [
    (
        "skill://shield/custom-rules",
        include_str!("../../skills/custom-rules/SKILL.md"),
    ),
    (
        "skill://shield/pack-update",
        include_str!("../../skills/pack-update/SKILL.md"),
    ),
    (
        "skill://shield/policy-audit",
        include_str!("../../skills/policy-audit/SKILL.md"),
    ),
    (
        "skill://shield/project-setup",
        include_str!("../../skills/project-setup/SKILL.md"),
    ),
];

pub(super) fn catalog() -> Result<SkillCatalog, SkillError> {
    SkillCatalog::new(
        SOURCES
            .iter()
            .map(|&(uri, source)| {
                SkillDefinition::new(uri, vec![SkillFile::text("SKILL.md", source)])
            })
            .collect(),
    )
}
