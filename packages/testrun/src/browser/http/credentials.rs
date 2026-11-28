//! Pinned first-match HTTP credentials; secrets never enter diagnostics.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credential {
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send: Option<Send>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Send {
    Unauthorized,
    Always,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Credentials {
    Single(Credential),
    List(Vec<Credential>),
}
impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpCredentials")
            .field("count", &self.list().len())
            .finish()
    }
}
impl Credentials {
    pub(crate) fn len(&self) -> usize {
        self.list().len()
    }
    fn list(&self) -> &[Credential] {
        match self {
            Self::Single(value) => std::slice::from_ref(value),
            Self::List(values) => values,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let entries = self.list();
        if entries.len() > 32
            || entries.iter().any(|entry| {
                entry.username.len() > 4096
                    || entry.password.len() > 4096
                    || entry.origin.as_ref().is_some_and(|origin| {
                        origin.len() > 4096 || origin.chars().any(char::is_control)
                    })
            })
            || entries
                .iter()
                .map(|entry| {
                    entry.username.len()
                        + entry.password.len()
                        + entry.origin.as_ref().map_or(0, String::len)
                })
                .sum::<usize>()
                > 65536
        {
            return Err("HTTP_CREDENTIALS_BOUNDS");
        }
        Ok(())
    }
    pub(crate) fn matching(&self, url: &Url) -> Option<&Credential> {
        let origin = url.origin().ascii_serialization();
        self.list().iter().find(|entry| {
            entry.origin.as_ref().is_none_or(|configured| {
                configured.is_empty() || configured.to_lowercase() == origin
            })
        })
    }
    pub(crate) fn wire(&self) -> Option<Value> {
        (!self.list().is_empty()).then(|| json!(self.list()))
    }
    pub(crate) fn schema() -> Value {
        let entry = json!({"type":"object","properties":{"username":{"type":"string","maxLength":4096},"password":{"type":"string","maxLength":4096},"origin":{"type":["string","null"],"maxLength":4096},"send":{"anyOf":[{"enum":["unauthorized","always"]},{"type":"null"}]}},"required":["username","password"],"additionalProperties":false});
        json!({"anyOf":[entry,{"type":"array","maxItems":32,"items":entry},{"type":"null"}],"description":"Basic HTTP credentials: one entry or an ordered list; first matching lowercased origin wins, and an omitted/empty origin matches all. Omitted/null HTTP credentials inherit the associated context; [] disables inheritance. At most 64 KiB combined credential bytes. send defaults to unauthorized; always replaces the initial Authorization header."})
    }
}
impl Credential {
    pub(crate) fn always(&self) -> bool {
        self.send == Some(Send::Always)
    }
    pub(crate) fn header(&self) -> String {
        format!(
            "Basic {}",
            STANDARD.encode(format!("{}:{}", self.username, self.password))
        )
    }
}
