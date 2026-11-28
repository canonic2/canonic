//! Published MCP skills: the selected workflow files embedded in this binary.
use canonic_mcp_kit::{SkillCatalog, SkillDefinition, SkillError, SkillFile};

include!(concat!(env!("OUT_DIR"), "/testrun_skills.rs"));

/// The URI namespace that holds every Testrun skill root.
const NAMESPACE: &str = "skill://testrun";

/// Validates the embedded files into one immutable catalog.
pub(crate) fn catalog() -> Result<SkillCatalog, SkillError> {
    let mut definitions: Vec<(&str, Vec<SkillFile>)> = Vec::new();
    for (skill, path, bytes) in SKILL_FILES {
        // Markdown is served as text; the build already checked it is UTF-8.
        let file = match std::str::from_utf8(bytes) {
            Ok(text) if path.ends_with(".md") => SkillFile::text(*path, text),
            _ => SkillFile::binary(*path, *bytes),
        };
        match definitions.iter_mut().find(|(name, _)| name == skill) {
            Some((_, files)) => files.push(file),
            None => definitions.push((skill, vec![file])),
        }
    }
    SkillCatalog::new(
        definitions
            .into_iter()
            .map(|(skill, files)| SkillDefinition::new(format!("{NAMESPACE}/{skill}"), files))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
