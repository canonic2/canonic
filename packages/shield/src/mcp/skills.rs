//! Immutable distributable workflows, independent of enrolled project state.
use canonic_mcp_kit::{SkillCatalog, SkillDefinition, SkillError, SkillFile};

pub(super) fn catalog() -> Result<SkillCatalog, SkillError> {
    SkillCatalog::new(vec![SkillDefinition::new(
        "skill://shield/policy-review",
        vec![SkillFile::text(
            "SKILL.md",
            include_str!("../../skills/policy-review/SKILL.md"),
        )],
    )])
}
