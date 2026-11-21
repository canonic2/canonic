//! Preserve host JSON key order without changing policy/state canonical encoding.
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::{Value, value::RawValue};

pub(super) fn format(source: Option<&str>, value: &Value) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&Ordered { source, value }).map(|text| format!("{text}\n"))
}

struct Ordered<'a> {
    source: Option<&'a str>,
    value: &'a Value,
}

struct Entries(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Entries;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Entries, M::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        input.deserialize_map(Object)
    }
}

impl Serialize for Ordered<'_> {
    fn serialize<S: Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        match self.value {
            Value::Object(values) => {
                let previous = self
                    .source
                    .and_then(|source| serde_json::from_str::<Entries>(source).ok())
                    .map_or_default(|entries| entries.0);
                let mut map = output.serialize_map(Some(values.len()))?;
                let retained: std::collections::BTreeSet<_> =
                    previous.iter().map(|(key, _)| key).collect();
                for (key, source) in &previous {
                    if let Some(value) = values.get(key) {
                        map.serialize_entry(
                            key,
                            &Ordered {
                                source: Some(source.get()),
                                value,
                            },
                        )?;
                    }
                }
                for (key, value) in values {
                    if !retained.contains(key) {
                        map.serialize_entry(
                            key,
                            &Ordered {
                                source: None,
                                value,
                            },
                        )?;
                    }
                }
                map.end()
            }
            Value::Array(values) => {
                let previous = self
                    .source
                    .and_then(|source| serde_json::from_str::<Vec<Box<RawValue>>>(source).ok())
                    .unwrap_or_default();
                let mut sequence = output.serialize_seq(Some(values.len()))?;
                for (index, value) in values.iter().enumerate() {
                    sequence.serialize_element(&Ordered {
                        source: previous.get(index).map(|raw| raw.get()),
                        value,
                    })?;
                }
                sequence.end()
            }
            scalar => scalar.serialize(output),
        }
    }
}
