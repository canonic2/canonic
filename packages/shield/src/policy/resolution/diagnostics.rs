//! Typed resolver evidence; identities and feedback supply no action authority.
use serde::Serialize;
mod projection;

/// Diagnostic schemas distinguish an omitted field from an explicit null.
#[derive(Clone, Debug)]
pub(crate) enum TextField {
    Absent,
    Null,
    Text(String),
}
impl From<Option<String>> for TextField {
    fn from(value: Option<String>) -> Self {
        value.map_or(Self::Null, Self::Text)
    }
}
impl TextField {
    pub(in crate::policy) fn is_present(&self) -> bool {
        !matches!(self, Self::Absent)
    }
    pub(in crate::policy) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Absent | Self::Null => None,
        }
    }
}

pub(crate) struct Private<T> {
    value: T,
    private: bool,
}
impl<T> Private<T> {
    pub(crate) fn new(value: T, private: bool) -> Self {
        Self { value, private }
    }
}
impl<T: Serialize> Serialize for Private<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.private {
            crate::evidence::projection::Sensitive(&self.value).serialize(serializer)
        } else {
            self.value.serialize(serializer)
        }
    }
}

pub(crate) struct RequestEvidence<'a> {
    request: &'a crate::policy::Request,
    private: bool,
}
impl<'a> RequestEvidence<'a> {
    pub(crate) fn new(request: &'a crate::policy::Request, private: bool) -> Self {
        Self { request, private }
    }
}
impl Serialize for RequestEvidence<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            executable: Private<&'a str>,
            arguments: Private<&'a [String]>,
            cwd: Private<&'a str>,
            project: &'a str,
        }
        View {
            executable: Private::new(&self.request.executable, self.private),
            arguments: Private::new(self.request.arguments.as_slice(), self.private),
            cwd: Private::new(&self.request.cwd, self.private),
            project: &self.request.project,
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Scope {
    Custom,
    Pack,
}

/// Field order retains the canonical JSON shape of existing resolver evidence.
#[derive(Clone, Debug)]
pub(crate) struct RuleReference {
    pack_selector: Option<String>,
    rule_id: String,
    scope: Scope,
    unavailable: bool,
}

impl Serialize for RuleReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        if self.unavailable {
            return serializer.serialize_none();
        }
        let mut record = serializer.serialize_struct(
            "RuleReference",
            if self.pack_selector.is_some() { 3 } else { 2 },
        )?;
        if let Some(selector) = &self.pack_selector {
            record.serialize_field("pack_selector", selector)?;
        }
        record.serialize_field("rule_id", &self.rule_id)?;
        record.serialize_field("scope", &self.scope)?;
        record.end()
    }
}

impl RuleReference {
    pub(in crate::policy) fn for_rule(source: &str, rule_id: &str, tier: u8) -> Self {
        Self {
            pack_selector: (tier != 0).then(|| source.to_owned()),
            rule_id: rule_id.to_owned(),
            scope: if tier == 0 {
                Scope::Custom
            } else {
                Scope::Pack
            },
            unavailable: false,
        }
    }
    pub(in crate::policy) fn annotate(
        &mut self,
        selectors: &std::collections::BTreeMap<String, String>,
    ) {
        self.unavailable = false;
        if let Some(source) = &self.pack_selector {
            if source.starts_with("pack:") {
                return;
            }
            match selectors.get(source) {
                Some(selector) => self.pack_selector = Some(selector.clone()),
                None => self.unavailable = true,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GuidanceRecord {
    pub(crate) alternatives: Vec<String>,
    pub(crate) branch: Option<&'static str>,
    pub(crate) candidate_only: bool,
    // An absent authored message is an explicit null in diagnostic evidence.
    pub(crate) message: Option<String>,
    pub(crate) operation_ref: String,
    pub(crate) rule_ref: Option<RuleReference>,
    pub(crate) source: Option<&'static str>,
    pub(crate) request: Option<GuidanceRequest>,
    pub(crate) fetch_url: TextField,
    pub(crate) private_input: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GuidanceRequest {
    pub(crate) executable: String,
    pub(crate) arguments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cwd: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_rule_references_preserve_custom_pack_and_feedback_wire_shapes() {
        for (tier, expected) in [
            (0, json!({"scope":"custom","rule_id":"read"})),
            (
                1,
                json!({"scope":"pack","pack_selector":"acme/tools","rule_id":"read"}),
            ),
        ] {
            let reference = RuleReference::for_rule("acme/tools", "read", tier);
            assert_eq!(serde_json::to_value(&reference).unwrap(), expected);
            assert_eq!(
                serde_json::to_string(&reference).unwrap(),
                expected.to_string()
            );
            let guidance = GuidanceRecord {
                alternatives: vec![],
                branch: Some("unmet"),
                candidate_only: true,
                message: None,
                operation_ref: "request".into(),
                rule_ref: Some(reference),
                source: None,
                request: None,
                fetch_url: TextField::Absent,
                private_input: false,
            };
            let expected = json!({"alternatives":[],"branch":"unmet","candidate_only":true,
                "message":null,"operation_ref":"request","rule_ref":expected});
            assert_eq!(serde_json::to_value(&guidance).unwrap(), expected);
            assert_eq!(
                serde_json::to_string(&guidance).unwrap(),
                expected.to_string()
            );
        }
    }
}
