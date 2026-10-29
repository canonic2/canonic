//! Controlling-terminal confirmation outside structured/model-supplied input.
use super::presentation::Card;
use crate::review::Error;
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{IsTerminal, Write},
};

mod inline;

/// One setup question: listed answers, optionally followed by a typed row.
pub(super) struct Question {
    pub(super) title: String,
    pub(super) prompt: String,
    pub(super) context: String,
    /// `(id, label, detail)` in display order.
    pub(super) choices: Vec<(String, String, String)>,
    /// Mark any number of choices instead of picking one.
    pub(super) multiple: bool,
    /// The typed row's label and optional placeholder.
    pub(super) typed: Option<(String, Option<String>)>,
    /// Highlighting never answers.
    pub(super) default: Option<String>,
    /// An earlier step exists: Escape reads "back" instead of "cancel".
    pub(super) back: bool,
}

/// Escape returns to the previous step; Ctrl-C cancels the whole flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Step<T> {
    Done(T),
    Back,
}
impl<T> Step<T> {
    /// The answer of a flow's first step, where going back cancels.
    pub(super) fn or_cancel(self) -> Result<T, Error> {
        match self {
            Self::Done(value) => Ok(value),
            Self::Back => Err(Error::Cancelled),
        }
    }
}

/// The operator's answer to a [`Question`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Reply {
    /// Choice IDs, possibly none for an optional multiple choice.
    Choices(Vec<String>),
    /// Text typed into the typed row.
    Typed(String),
}
/// Checks a reply before it is accepted; a reason reopens the question.
pub(super) type Check<'a> = dyn FnMut(&Reply) -> Result<(), String> + 'a;

/// Operator interaction through the controlling terminal, separate from stdin.
pub(super) struct Terminal {
    output: File,
}
impl Terminal {
    pub(super) fn required(json: bool) -> Result<Self, Error> {
        if json {
            return Err(Error::Invalid(
                "JSON is read-only; run this command interactively",
            ));
        }
        Self::open().ok_or(Error::Invalid(
            "operator terminal required; run this command interactively",
        ))
    }
    pub(super) fn open() -> Option<Self> {
        #[cfg(unix)]
        let (input_path, output_path) = ("/dev/tty", "/dev/tty");
        #[cfg(windows)]
        let (input_path, output_path) = ("CONIN$", "CONOUT$");
        let input = OpenOptions::new().read(true).open(input_path).ok()?;
        let output = OpenOptions::new().write(true).open(output_path).ok()?;
        if !input.is_terminal() || !output.is_terminal() {
            return None;
        }
        Some(Self { output })
    }
    pub(super) fn prompt(&mut self, message: &str) -> Result<String, Error> {
        inline::input(self.host(5)?, message)
    }
    /// Flush settled output before the inline host takes over the terminal.
    fn host(&mut self, height: u16) -> Result<inline::Host, Error> {
        self.output.flush()?;
        inline::open(height)
    }
    pub(super) fn confirm(&mut self, value: &Value) -> Result<(), Error> {
        self.display(value)?;
        let result = inline::choose(
            self.host(7)?,
            "Review and confirm",
            "Apply the change shown above to this project?",
            &[
                ("cancel", "Cancel", "Leave this change unapplied"),
                ("apply", "Apply change", "Confirm the reviewed scope"),
            ],
            "cancel",
        )?;
        if result == "apply" {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
    /// The operator's proposal decision: `accept`, `reject` or `leave`.
    /// Choosing never applies it; the caller confirms and applies.
    pub(super) fn decide(&mut self, card: &Card, waiting: usize) -> Result<String, Error> {
        inline::review(self.host(12)?, card, waiting)
    }
    pub(super) fn display(&mut self, value: &Value) -> Result<(), Error> {
        write!(self.output, "{}", super::presentation::review(value))?;
        self.output.flush()?;
        Ok(())
    }
    /// One line saying what Shield is doing while the operator waits.
    pub(super) fn progress(&mut self, text: &str) -> Result<(), Error> {
        writeln!(self.output, "{text}")?;
        Ok(())
    }
    pub(super) fn note(&mut self, title: &str, detail: &str) -> Result<(), Error> {
        writeln!(self.output, "\n{title}\n{detail}\n")?;
        Ok(())
    }
    pub(super) fn choose(
        &mut self,
        title: &str,
        context: &str,
        choices: &[(&str, &str, &str)],
        default: &str,
    ) -> Result<String, Error> {
        inline::choose(self.host(12)?, title, context, choices, default)
    }
    /// Typed text survives validation refusal in the shared question form.
    pub(super) fn question(
        &mut self,
        question: &Question,
        check: &mut Check<'_>,
    ) -> Result<Step<Reply>, Error> {
        inline::question(self.host(12)?, question, check)
    }
    /// A guided choice: Escape returns [`Step::Back`], labeled as back or cancel
    /// according to whether an earlier step exists.
    pub(super) fn choose_step(
        &mut self,
        title: &str,
        context: &str,
        choices: &[(&str, &str, &str)],
        default: &str,
        back: bool,
    ) -> Result<Step<String>, Error> {
        inline::choose_step(
            self.host(12)?,
            title,
            context,
            choices,
            default,
            if back { "back" } else { "cancel" },
        )
    }
    /// A pack profile, with Recommended preselected; init and add share it.
    pub(super) fn choose_profile(
        &mut self,
        title: &str,
        context: &str,
    ) -> Result<crate::policy::Profile, Error> {
        self.choose_profile_step(title, context, crate::policy::Profile::Recommended, false)?
            .or_cancel()
    }
    /// The profile as one step of a guided flow, starting at `default`.
    pub(super) fn choose_profile_step(
        &mut self,
        title: &str,
        context: &str,
        default: crate::policy::Profile,
        back: bool,
    ) -> Result<Step<crate::policy::Profile>, Error> {
        use crate::policy::Profile;
        let default = match default {
            Profile::Recommended => "recommended",
            Profile::Strict => "strict",
            Profile::Loose => "loose",
        };
        let Step::Done(chosen) = self.choose_step(
            title,
            context,
            &[
                (
                    "recommended",
                    "Recommended",
                    "Balanced defaults supplied by the pack",
                ),
                ("strict", "Strict", "The pack's most restrictive profile"),
                ("loose", "Loose", "The pack's least restrictive profile"),
            ],
            default,
            back,
        )?
        else {
            return Ok(Step::Back);
        };
        match chosen.as_str() {
            "recommended" => Ok(Step::Done(Profile::Recommended)),
            "strict" => Ok(Step::Done(Profile::Strict)),
            "loose" => Ok(Step::Done(Profile::Loose)),
            _ => Err(Error::Invalid("invalid pack profile")),
        }
    }
    pub(super) fn choose_many(
        &mut self,
        title: &str,
        context: &str,
        choices: &[(&str, &str, &str)],
        marked: &[&str],
        empty: &str,
    ) -> Result<Vec<String>, Error> {
        inline::choose_many(self.host(12)?, title, context, choices, marked, empty)
    }
    /// A guided multiple choice: Escape returns [`Step::Back`], labeled as
    /// back or cancel according to whether an earlier step exists.
    pub(super) fn choose_many_step(
        &mut self,
        title: &str,
        context: &str,
        choices: &[(&str, &str, &str)],
        marked: &[&str],
        empty: &str,
        back: bool,
    ) -> Result<Step<Vec<String>>, Error> {
        inline::choose_many_step(
            self.host(12)?,
            title,
            context,
            choices,
            marked,
            empty,
            if back { "back" } else { "cancel" },
        )
    }
}
