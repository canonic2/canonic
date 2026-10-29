//! Terminal review and confirmed mutation of protected file access settings.
use super::terminal::{Step, Terminal};
use crate::review::{
    Context, Error,
    path_access::{Change, PathAccess, Read, load},
    storage::Store,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

fn word<T: Serialize>(value: T) -> String {
    json!(value).as_str().unwrap_or_default().to_owned()
}

/// Guided change, confirmed in the operator terminal and refused in JSON mode.
pub(super) fn configure(context: &Context, json_mode: bool) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let meta = crate::review::project::metadata(&crate::review::project::load(context)?);
    // An invalid record blocks agents; the operator replaces it from the defaults.
    let invalid = load(&meta).is_err();
    let before = load(&meta).unwrap_or_default();
    if invalid {
        tty.note(
            "Invalid saved choice",
            "The saved protected file access is invalid, so agents are blocked. Choices start from the defaults.",
        )?;
    }
    tty.note(
        "Protected files",
        ".shield/review, compiled caches, the .shield folder and .shield/.gitignore stay protected whatever you choose here.",
    )?;
    let after = chosen(before, ask(&mut tty, before)?)?;
    if after == before && !invalid {
        return Ok(receipt(after, false));
    }
    tty.confirm(
        &json!({"operation":"change protected file access","before":before,"after":after}),
    )?;
    Store::open(Path::new(&context.project_root))?.update(context, &|| Ok(()), |state| {
        if state.operator.get("path_access") != meta.get("path_access") {
            return Err(Error::Invalid(
                "protected file access changed during confirmation; review again",
            ));
        }
        state.operator["path_access"] = json!(after);
        Ok(json!({}))
    })?;
    Ok(receipt(after, true))
}

/// The three access questions, starting from the current answers. Escape goes
/// back one question with answers kept; on the first it cancels.
fn ask(tty: &mut Terminal, before: PathAccess) -> Result<[String; 3], Error> {
    let reading = format!(
        ".mcp.json and agent settings ({}) may hold credentials.",
        agent_settings().join(", ")
    );
    let mut answers = [
        word(before.agent_settings_read),
        word(before.agent_settings_change),
        word(before.shield_config_change),
    ];
    let mut step = 0;
    while step < answers.len() {
        let (title, context, choices): (&str, &str, &[(&str, &str, &str)]) = match step {
            0 => (
                "Reading agent settings",
                &reading,
                &[
                    (
                        "ask",
                        "Ask first",
                        "Ask where the agent can prompt, otherwise block",
                    ),
                    ("allow", "Allow", "Agents read them without asking"),
                ],
            ),
            1 => (
                "Changing agent settings",
                "An agent that can change these files can remove Shield's hooks and turn protection off, or remove the host confirmation policy changes through Shield's MCP tools rely on, so a proposal could activate without you.",
                &[
                    (
                        "block",
                        "Block",
                        "Only you change them, with shield mcp setup",
                    ),
                    ("allow", "Allow", "Agents may edit or remove them"),
                ],
            ),
            _ => (
                "Changing Shield policy files",
                "Edits to .shield/config.yml, rules.yml and the other policy files apply only after you review them with shield policy review.",
                &[
                    (
                        "block",
                        "Block",
                        "Agents propose changes through Shield's MCP tools",
                    ),
                    (
                        "allow",
                        "Allow",
                        "Agents may edit them; you still review before activation",
                    ),
                ],
            ),
        };
        match tty.choose_step(title, context, choices, &answers[step], step > 0)? {
            Step::Done(answer) => {
                answers[step] = answer;
                step += 1;
            }
            Step::Back if step > 0 => step -= 1,
            Step::Back => return Err(Error::Cancelled),
        }
    }
    Ok(answers)
}

/// The access the answers select; an empty answer keeps the current choice.
fn chosen(before: PathAccess, [read, settings, config]: [String; 3]) -> Result<PathAccess, Error> {
    let change = |answer: &str, current: Change| match answer {
        "" => Ok(current),
        "block" => Ok(Change::Block),
        "allow" => Ok(Change::Allow),
        _ => Err(Error::Invalid("choose block or allow")),
    };
    Ok(PathAccess {
        agent_settings_read: match read.as_str() {
            "" => before.agent_settings_read,
            "ask" => Read::Ask,
            "allow" => Read::Allow,
            _ => return Err(Error::Invalid("choose ask or allow")),
        },
        agent_settings_change: change(&settings, before.agent_settings_change)?,
        shield_config_change: change(&config, before.shield_config_change)?,
    })
}

/// Every registered adapter's settings files, in adapter order: the protected
/// administration paths other than the `.shield` folder and `.mcp.json`.
fn agent_settings() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = Vec::new();
    for path in crate::review::integrations::protected_files() {
        if !matches!(*path, ".shield" | ".mcp.json") && !paths.contains(path) {
            paths.push(path);
        }
    }
    paths
}

fn receipt(access: PathAccess, changed: bool) -> Value {
    json!({"schema_version":1,"operation":"protected file access","path_access":access,"changed":changed})
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_settings_name_every_adapter_file_once() {
        let paths = super::agent_settings();
        for adapter in crate::agents::ADAPTERS {
            let spec = adapter.spec();
            for path in std::iter::once(spec.hooks_path)
                .chain(spec.mcp_path)
                .chain(spec.prefix_rules_path)
                .chain(spec.settings_paths.iter().copied())
            {
                assert_eq!(paths.iter().filter(|p| **p == path).count(), 1, "{path}");
            }
        }
        assert!(paths.contains(&".codex/rules/shield.rules"), "{paths:?}");
        assert!(!paths.contains(&".mcp.json") && !paths.contains(&".shield"));
    }
}
