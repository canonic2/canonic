//! Common MCP setup interaction labels and safe final confirmation.
use super::{Host, Outcome, PickerPrompt};
use crate::picker::Choice;
use std::io;

/// The operation selected before choosing agents. This conveys intent only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Install or repair the selected registrations.
    Install,
    /// Remove only owned registrations for the selected agents.
    Remove,
}

/// Build the common action page without acquiring the terminal.
/// # Errors
/// Returns invalid choice descriptors.
pub fn action_prompt() -> io::Result<PickerPrompt<'static>> {
    PickerPrompt::single(
        "MCP integration action",
        "",
        vec![
            Choice::new("install", "Install or repair"),
            Choice::new("remove", "Remove owned registrations"),
        ],
        Some("install"),
    )
}

/// Build the common agent page. Consumers supply supported agents, status,
/// disabled reasons and the appropriate saved/detected preselection.
/// # Errors
/// Duplicate IDs or no enabled agents return errors before terminal acquisition.
pub fn agent_prompt(
    choices: Vec<Choice>,
    marked: &[&str],
    remove: bool,
) -> io::Result<PickerPrompt<'static>> {
    let context = if remove {
        "Space marks agents to remove; Enter continues to the review. Unmarked agents keep their configuration."
    } else if marked.is_empty() {
        "Space marks agents to install or repair; Enter continues. Unmarked agents keep their configuration."
    } else {
        "Existing setup or detected CLIs supply the preselection. Space marks agents to install or repair; Enter continues. Unmarked agents keep their configuration."
    };
    PickerPrompt::multiple(
        "Select agents",
        context,
        choices,
        if remove { &[] } else { marked },
        1,
        "continue",
    )
}

/// Run the common agent page, returning no selection on cancellation.
/// # Errors
/// Invalid choices and terminal failures are returned.
pub fn agents(
    choices: Vec<Choice>,
    marked: &[&str],
    remove: bool,
) -> io::Result<Option<Vec<String>>> {
    let mut prompt = agent_prompt(choices, marked, remove)?;
    Ok(match Host::open(16)?.pick(&mut prompt)? {
        Outcome::Submitted(ids) => Some(ids),
        Outcome::Back | Outcome::Cancelled => None,
    })
}

/// Choose install/repair or removal; cancellation changes nothing.
/// # Errors
/// Returns terminal acquisition, input, rendering and restoration failures.
pub fn action() -> io::Result<Option<Action>> {
    Ok(match Host::open(12)?.pick(&mut action_prompt()?)? {
        Outcome::Submitted(ids) if ids.first().is_some_and(|id| id == "install") => {
            Some(Action::Install)
        }
        Outcome::Submitted(ids) if ids.first().is_some_and(|id| id == "remove") => {
            Some(Action::Remove)
        }
        _ => None,
    })
}

/// Confirm an already printed complete review, defaulting to Cancel. The
/// consumer owns publication and must validate that the reviewed plan is current.
/// # Errors
/// Returns terminal acquisition, input, rendering and restoration failures.
pub fn confirm(context: &str) -> io::Result<bool> {
    Ok(
        matches!(Host::open(12)?.pick(&mut confirmation_prompt(context)?)?, Outcome::Submitted(ids) if ids.first().is_some_and(|id| id == "apply")),
    )
}

/// Build the safe final page for a consumer with an existing checkpoint owner.
/// # Errors
/// Returns invalid choice descriptors.
pub fn confirmation_prompt(context: &str) -> io::Result<PickerPrompt<'_>> {
    PickerPrompt::single(
        "Apply MCP setup?",
        context,
        vec![
            Choice::new("cancel", "Cancel"),
            Choice::new("apply", "Apply reviewed changes"),
        ],
        Some("cancel"),
    )
}
