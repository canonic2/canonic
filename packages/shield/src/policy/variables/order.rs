//! The order a pack source declares its variables in, which typed parsing loses.
use super::super::{Error, parse_yaml};
use serde::{
    Deserialize, Deserializer,
    de::{IgnoredAny, MapAccess, Visitor},
};
use std::fmt;

/// Variable names in source order; empty when the pack declares none.
///
/// # Errors
/// Returns a syntax error for a document that is neither JSON nor YAML.
pub(crate) fn read(source: &str) -> Result<Vec<String>, Error> {
    let document: Document = match serde_json::from_str(source) {
        Ok(document) => document,
        Err(_) => parse_yaml(source)?,
    };
    Ok(document.0)
}

struct Document(Vec<String>);
struct Names(Vec<String>);

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Document;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a pack document")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut fields: A) -> Result<Document, A::Error> {
                let mut names = Vec::new();
                while let Some(key) = fields.next_key::<String>()? {
                    if key == "variables" {
                        names = fields.next_value::<Names>()?.0;
                    } else {
                        fields.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(Document(names))
            }
        }
        input.deserialize_map(Fields)
    }
}

impl<'de> Deserialize<'de> for Names {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Keys;
        impl<'de> Visitor<'de> for Keys {
            type Value = Names;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("variable declarations")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Names, A::Error> {
                let mut names = Vec::new();
                while let Some(name) = entries.next_key::<String>()? {
                    entries.next_value::<IgnoredAny>()?;
                    names.push(name);
                }
                Ok(Names(names))
            }
        }
        input.deserialize_map(Keys)
    }
}

#[cfg(test)]
mod tests {
    use super::read;

    #[test]
    fn json_and_yaml_keep_declaration_order() {
        let json = r#"{"id":"acme/x","variables":{"zone":{},"alpha":{},"mid":{}},"rules":[]}"#;
        assert_eq!(read(json).unwrap(), ["zone", "alpha", "mid"]);
        let yaml = "id: acme/x\nvariables:\n  zone: {type: string}\n  alpha: {type: string}\n";
        assert_eq!(read(yaml).unwrap(), ["zone", "alpha"]);
        assert_eq!(
            read("id: acme/x\nrules: []\n").unwrap(),
            Vec::<String>::new()
        );
    }
}
