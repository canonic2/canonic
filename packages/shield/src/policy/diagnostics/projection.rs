//! Borrow raw policy facts while annotating only owner-known input echoes.
use super::{GuidanceRecord, GuidanceRequest, Private, RequestEvidence};
use crate::policy::{Evaluation, Match, Request, rules::Check};
use serde::{
    Serialize,
    ser::{SerializeSeq, SerializeStruct},
};

struct Requests<'a> {
    values: &'a [Request],
    private: bool,
}
impl Serialize for Requests<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.values.len()))?;
        for request in self.values {
            values.serialize_element(&RequestEvidence::new(request, self.private))?;
        }
        values.end()
    }
}
struct Effects<'a> {
    value: &'a crate::effects::Hints,
    private: bool,
}
impl Serialize for Effects<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.private {
            self.value.private_input_view().serialize(serializer)
        } else {
            self.value.serialize(serializer)
        }
    }
}

impl Serialize for Evaluation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 11
            + usize::from(!self.mapped_commands.is_empty())
            + usize::from(self.effect_evidence.is_some())
            + usize::from(self.semantic_coverage.is_some());
        let mut value = serializer.serialize_struct("Evaluation", fields)?;
        value.serialize_field("schema_version", &self.schema_version)?;
        value.serialize_field("mode", self.mode)?;
        value.serialize_field("decision", &self.decision)?;
        value.serialize_field("revision", &self.revision)?;
        value.serialize_field("matches", &self.matches)?;
        if !self.mapped_commands.is_empty() {
            value.serialize_field(
                "mapped_commands",
                &Requests {
                    values: &self.mapped_commands,
                    private: self.private_input,
                },
            )?;
        }
        value.serialize_field("requirement_checks", &self.requirement_checks)?;
        value.serialize_field("guidance", &self.guidance)?;
        value.serialize_field("reason", self.reason)?;
        value.serialize_field("cwd", &Private::new(&self.cwd, self.private_input))?;
        value.serialize_field("project", &self.project)?;
        value.serialize_field("packs", &self.packs)?;
        if let Some(effects) = &self.effect_evidence {
            value.serialize_field(
                "effect_evidence",
                &Effects {
                    value: effects,
                    private: self.private_input,
                },
            )?;
        }
        if let Some(coverage) = self.semantic_coverage {
            value.serialize_field("semantic_coverage", coverage)?;
        }
        value.end()
    }
}

impl Serialize for Match {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 8
            + usize::from(self.fallback)
            + usize::from(self.precondition)
            + usize::from(self.decision_provenance.is_some())
            + usize::from(self.operation_ref.is_some())
            + usize::from(self.executable.is_present())
            + usize::from(self.rule_ref.is_some());
        let mut value = serializer.serialize_struct("Match", fields)?;
        value.serialize_field("source", &self.source)?;
        value.serialize_field("rule_id", &self.rule_id)?;
        value.serialize_field("decision", &self.decision)?;
        value.serialize_field("reason", &self.reason)?;
        value.serialize_field("specificity", &self.specificity)?;
        value.serialize_field("applied", &self.applied)?;
        value.serialize_field("branch", &self.branch)?;
        value.serialize_field("requirement_checks", &self.requirement_checks)?;
        if self.fallback {
            value.serialize_field("fallback", &true)?;
        }
        if self.precondition {
            value.serialize_field("precondition", &true)?;
        }
        if let Some(provenance) = &self.decision_provenance {
            value.serialize_field("decision_provenance", provenance)?;
        }
        if let Some(reference) = &self.operation_ref {
            value.serialize_field("operation_ref", reference)?;
        }
        if self.executable.is_present() {
            value.serialize_field(
                "executable",
                &self
                    .executable
                    .text()
                    .map(|name| Private::new(name, self.private_input)),
            )?;
        }
        if let Some(reference) = &self.rule_ref {
            value.serialize_field("rule_ref", reference)?;
        }
        value.end()
    }
}

impl Serialize for Check {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value =
            serializer.serialize_struct("Check", 7 + usize::from(self.evidence.is_some()))?;
        value.serialize_field("rule_ref", &self.rule_ref)?;
        value.serialize_field("operation_ref", &self.operation_ref)?;
        value.serialize_field("predicate_path", &self.predicate_path)?;
        value.serialize_field("status", self.status)?;
        value.serialize_field("code", self.code)?;
        value.serialize_field("branch", self.branch)?;
        value.serialize_field("applied", &self.applied)?;
        if let Some(evidence) = &self.evidence {
            value.serialize_field(
                "evidence",
                &Private::new(evidence, self.private_input && !evidence.is_null()),
            )?;
        }
        value.end()
    }
}

struct GuidanceRequestView<'a> {
    value: &'a GuidanceRequest,
    private: bool,
}
impl Serialize for GuidanceRequestView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serializer
            .serialize_struct("GuidanceRequest", 2 + usize::from(self.value.cwd.is_some()))?;
        value.serialize_field(
            "executable",
            &Private::new(&self.value.executable, self.private),
        )?;
        value.serialize_field(
            "arguments",
            &Private::new(&self.value.arguments, self.private),
        )?;
        if let Some(cwd) = self.value.cwd {
            value.serialize_field("cwd", cwd)?;
        }
        value.end()
    }
}
impl Serialize for GuidanceRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 4
            + usize::from(self.branch.is_some())
            + usize::from(self.rule_ref.is_some())
            + usize::from(self.source.is_some())
            + usize::from(self.request.is_some())
            + usize::from(self.fetch_url.is_present());
        let mut value = serializer.serialize_struct("GuidanceRecord", fields)?;
        value.serialize_field("alternatives", &self.alternatives)?;
        if let Some(branch) = self.branch {
            value.serialize_field("branch", branch)?;
        }
        value.serialize_field("candidate_only", &self.candidate_only)?;
        value.serialize_field(
            "message",
            &self
                .message
                .as_deref()
                .map(|message| Private::new(message, self.private_input && self.branch.is_none())),
        )?;
        value.serialize_field("operation_ref", &self.operation_ref)?;
        if let Some(reference) = &self.rule_ref {
            value.serialize_field("rule_ref", reference)?;
        }
        if let Some(source) = self.source {
            value.serialize_field("source", source)?;
        }
        if let Some(request) = &self.request {
            value.serialize_field(
                "request",
                &GuidanceRequestView {
                    value: request,
                    private: self.private_input,
                },
            )?;
        }
        if self.fetch_url.is_present() {
            value.serialize_field(
                "fetch_url",
                &self
                    .fetch_url
                    .text()
                    .map(|url| Private::new(url, self.private_input)),
            )?;
        }
        value.end()
    }
}
