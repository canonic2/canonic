//! Explicit variable assignments and rule overrides, validated against pack declarations.
use crate::policy::{self, Binding, Override};
use crate::review::Error;
use serde_json::Value;

/// Command-line changes to one selection.
#[derive(Default)]
pub(in crate::review) struct Edits<'a> {
    pub(in crate::review) set: &'a [String],
    pub(in crate::review) unset: &'a [String],
    pub(in crate::review) overrides: &'a [String],
    pub(in crate::review) clear: &'a [String],
    /// Change the local layer instead of the shared configuration.
    pub(in crate::review) local: bool,
}

impl Edits<'_> {
    pub(in crate::review) fn is_empty(&self) -> bool {
        self.set.is_empty()
            && self.unset.is_empty()
            && self.overrides.is_empty()
            && self.clear.is_empty()
    }
}

fn invalid(message: &'static str) -> Error {
    Error::Invalid(message)
}

/// Apply `--set name=value`, `--unset name`, `--override id=value` and
/// `--clear-override id` to a binding, checking names and values against the pack.
pub(in crate::review) fn apply(
    artifact: &str,
    binding: &mut Binding,
    edits: &Edits<'_>,
) -> Result<(), Error> {
    let declarations = policy::declarations(artifact)?;
    for assignment in edits.set {
        let (name, text) = assignment
            .split_once('=')
            .ok_or(invalid("use --set name=value"))?;
        let declaration = declarations
            .get(name)
            .ok_or(invalid("the pack declares no such variable"))?;
        binding
            .variables
            .insert(name.to_owned(), declaration.parse(text)?);
    }
    for name in edits.unset {
        if !declarations.contains_key(name) {
            return Err(invalid("the pack declares no such variable"));
        }
        binding.variables.remove(name);
    }
    for assignment in edits.overrides {
        let (id, text) = assignment.split_once('=').ok_or(invalid(
            "use --override id=off|allow|ask|deny|on_unmet:ask|on_unmet:deny",
        ))?;
        binding
            .overrides
            .insert(id.to_owned(), parse_override(text)?);
    }
    for id in edits.clear {
        binding.overrides.remove(id);
    }
    Ok(())
}

/// Bind typed values and remove names, checking both against the pack.
pub(in crate::review) fn assign(
    artifact: &str,
    binding: &mut Binding,
    set: &std::collections::BTreeMap<String, Value>,
    unset: &[String],
) -> Result<(), Error> {
    let declarations = policy::declarations(artifact)?;
    for (name, value) in set {
        declarations
            .get(name)
            .ok_or(invalid("the pack declares no such variable"))?
            .check(value)
            .map_err(|error| match error {
                policy::Error::Contract(reason) => policy::Error::binding(name, reason),
                other => other,
            })?;
        binding.variables.insert(name.clone(), value.clone());
    }
    for name in unset {
        if !declarations.contains_key(name) {
            return Err(invalid("the pack declares no such variable"));
        }
        binding.variables.remove(name);
    }
    Ok(())
}

/// `off`, `allow`, `ask`, `deny`, `on_unmet:ask`, `on_unmet:deny`, or a JSON object.
pub(in crate::review) fn parse_override(text: &str) -> Result<Override, Error> {
    let value = if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|_| invalid("invalid override object"))?
    } else if let Some(decision) = text.strip_prefix("on_unmet:") {
        serde_json::json!({"on_unmet":decision})
    } else {
        serde_json::json!(text)
    };
    serde_json::from_value(value)
        .map_err(|_| invalid("overrides are off, allow, ask, deny or on_unmet:ask|deny"))
}
