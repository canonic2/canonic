//! Borrowed presentation of effects derived inside a private source span.
use super::{Hints, NestedExecution, arguments::OptionValue};
use crate::policy::resolution::diagnostics::{Private, RequestEvidence};
use serde::{Serialize, ser::SerializeSeq};

pub(super) struct PrivateInput<'a>(pub(super) &'a Hints);
impl Serialize for PrivateInput<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            model: &'static str,
            tool_hint: &'static str,
            command: Option<Private<&'a str>>,
            effects: &'a std::collections::BTreeSet<super::Effect>,
            complete: bool,
            unresolved: &'a [&'static str],
            global_options: Options<'a>,
            command_path: Private<&'a [String]>,
            nested: Nested<'a>,
            request: RequestEvidence<'a>,
        }
        let hints = self.0;
        View {
            model: hints.model,
            tool_hint: hints.tool_hint,
            command: hints
                .command
                .as_deref()
                .map(|value| Private::new(value, true)),
            effects: &hints.effects,
            complete: hints.complete,
            unresolved: &hints.unresolved,
            global_options: Options(&hints.global_options),
            command_path: Private::new(hints.command_path.as_slice(), true),
            nested: Nested(&hints.nested),
            request: RequestEvidence::new(&hints.request, true),
        }
        .serialize(serializer)
    }
}
struct Options<'a>(&'a [OptionValue]);
impl Serialize for Options<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            index: usize,
            name: Private<&'a str>,
            value: Option<Private<&'a str>>,
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for option in self.0 {
            sequence.serialize_element(&View {
                index: option.index,
                name: Private::new(&option.name, true),
                value: option
                    .value
                    .as_deref()
                    .map(|value| Private::new(value, true)),
            })?;
        }
        sequence.end()
    }
}
struct Nested<'a>(&'a [NestedExecution]);
impl Serialize for Nested<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            kind: &'static str,
            source: Option<Private<&'a str>>,
            argv: Private<&'a [String]>,
            argument_index: usize,
            namespace: &'static str,
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for nested in self.0 {
            sequence.serialize_element(&View {
                kind: nested.kind,
                source: nested
                    .source
                    .as_deref()
                    .map(|value| Private::new(value, true)),
                argv: Private::new(nested.argv.as_slice(), true),
                argument_index: nested.argument_index,
                namespace: nested.namespace,
            })?;
        }
        sequence.end()
    }
}
