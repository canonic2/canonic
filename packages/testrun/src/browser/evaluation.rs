//! Backend-independent evaluation values and bounded Playwright value conversion.
//!
//! Graph IDs describe values inside one argument/result, never driver objects.
//! Map/Set require explicit page conversion or a retained handle. Functions in
//! results are descriptions only; this API does not register native callbacks.

use super::{contract::Reference, schema, values};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
mod selector;
pub use selector::SelectorOptions;
pub(crate) use selector::capabilities as selector_capabilities;
pub(crate) use selector::selector_schema;

pub(crate) const MAX_BYTES: usize = 1024 * 1024;
const MAX_ID: u32 = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluationValue {
    Json {
        value: Value,
    },
    Undefined {},
    Number {
        value: SpecialNumber,
    },
    Bigint {
        value: String,
    },
    Date {
        value: String,
    },
    Url {
        value: String,
    },
    Regexp {
        pattern: String,
        flags: String,
    },
    Error {
        name: String,
        message: String,
        stack: String,
    },
    TypedArray {
        element: Element,
        base64: String,
    },
    Array {
        id: u32,
        items: Vec<Self>,
    },
    Object {
        id: u32,
        entries: Vec<Entry>,
    },
    Reference {
        id: u32,
    },
    Handle {
        handle: Reference,
    },
    Function {
        name: String,
    },
}

impl Default for EvaluationValue {
    fn default() -> Self {
        Self::Undefined {}
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub key: String,
    pub value: EvaluationValue,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SpecialNumber {
    #[serde(rename = "nan")]
    Nan,
    #[serde(rename = "positive_infinity")]
    PositiveInfinity,
    #[serde(rename = "negative_infinity")]
    NegativeInfinity,
    #[serde(rename = "negative_zero")]
    NegativeZero,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    Bigint64,
    Biguint64,
}

impl Element {
    fn wire(self) -> (&'static str, usize) {
        match self {
            Self::Int8 => ("i8", 1),
            Self::Uint8 => ("ui8", 1),
            Self::Uint8Clamped => ("ui8c", 1),
            Self::Int16 => ("i16", 2),
            Self::Uint16 => ("ui16", 2),
            Self::Int32 => ("i32", 4),
            Self::Uint32 => ("ui32", 4),
            Self::Float32 => ("f32", 4),
            Self::Float64 => ("f64", 8),
            Self::Bigint64 => ("bi64", 8),
            Self::Biguint64 => ("bui64", 8),
        }
    }
    fn parse(value: &str) -> Result<Self, &'static str> {
        Ok(match value {
            "i8" => Self::Int8,
            "ui8" => Self::Uint8,
            "ui8c" => Self::Uint8Clamped,
            "i16" => Self::Int16,
            "ui16" => Self::Uint16,
            "i32" => Self::Int32,
            "ui32" => Self::Uint32,
            "f32" => Self::Float32,
            "f64" => Self::Float64,
            "bi64" => Self::Bigint64,
            "bui64" => Self::Biguint64,
            _ => return Err("INVALID_EVALUATION_VALUE"),
        })
    }
}

pub(crate) fn validate(value: &EvaluationValue) -> Result<(), &'static str> {
    argument(value, |_| Ok(json!({}))).map(|_| ())
}

/// Resolving a handle is the session owner's job, after document/context checks.
pub(crate) fn argument(
    value: &EvaluationValue,
    mut resolve: impl FnMut(Reference) -> Result<Value, &'static str>,
) -> Result<Value, &'static str> {
    let public = serde_json::to_value(value).map_err(|_| "VALUE_BOUNDS")?;
    schema::encoded_len(&public, MAX_BYTES).map_err(|_| "VALUE_BOUNDS")?;
    let mut handles = Vec::new();
    let value = encode(value, &mut BTreeSet::new(), &mut handles, &mut resolve)?;
    let result = json!({"value":value,"handles":handles});
    schema::encoded_len(&result, MAX_BYTES).map_err(|_| "VALUE_BOUNDS")?;
    Ok(result)
}

fn graph_id(ids: &mut BTreeSet<u32>, id: u32) -> Result<(), &'static str> {
    if id == 0 || id > MAX_ID || !ids.insert(id) {
        return Err("INVALID_VALUE_REFERENCE");
    }
    Ok(())
}

fn encode(
    value: &EvaluationValue,
    ids: &mut BTreeSet<u32>,
    handles: &mut Vec<Value>,
    resolve: &mut impl FnMut(Reference) -> Result<Value, &'static str>,
) -> Result<Value, &'static str> {
    Ok(match value {
        EvaluationValue::Json { value } => values::argument(value)?["value"].clone(),
        EvaluationValue::Undefined {} => json!({"v":"undefined"}),
        EvaluationValue::Number { value } => json!({"v":match value {
            SpecialNumber::Nan => "NaN",
            SpecialNumber::PositiveInfinity => "Infinity",
            SpecialNumber::NegativeInfinity => "-Infinity",
            SpecialNumber::NegativeZero => "-0",
        }}),
        EvaluationValue::Bigint { value } => {
            let digits = value.strip_prefix('-').unwrap_or(value);
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("INVALID_BIGINT");
            }
            json!({"bi":value})
        }
        EvaluationValue::Date { value } => {
            if !valid_date(value) {
                return Err("INVALID_DATE");
            }
            json!({"d":value})
        }
        EvaluationValue::Url { value } => {
            if !value.contains(':') || value.chars().any(char::is_control) {
                return Err("INVALID_VALUE_URL");
            }
            json!({"u":value})
        }
        EvaluationValue::Regexp { pattern, flags } => {
            let mut seen = BTreeSet::new();
            if flags
                .chars()
                .any(|flag| !"dgimsuvy".contains(flag) || !seen.insert(flag))
                || (seen.contains(&'u') && seen.contains(&'v'))
            {
                return Err("INVALID_REGEXP_FLAGS");
            }
            json!({"r":{"p":pattern,"f":flags}})
        }
        EvaluationValue::Error {
            name,
            message,
            stack,
        } => {
            json!({"e":{"n":name,"m":message,"s":stack}})
        }
        EvaluationValue::TypedArray { element, base64 } => {
            let bytes = STANDARD.decode(base64).map_err(|_| "INVALID_TYPED_ARRAY")?;
            let (kind, alignment) = element.wire();
            if bytes.len() % alignment != 0 || STANDARD.encode(&bytes) != *base64 {
                return Err("INVALID_TYPED_ARRAY");
            }
            json!({"ta":{"k":kind,"b":base64}})
        }
        EvaluationValue::Array { id, items } => {
            graph_id(ids, *id)?;
            json!({"id":id,"a":items.iter().map(|v| encode(v,ids,handles,resolve)).collect::<Result<Vec<_>,_>>()?})
        }
        EvaluationValue::Object { id, entries } => {
            graph_id(ids, *id)?;
            let mut keys = BTreeSet::new();
            let mut result = Vec::new();
            for entry in entries {
                if !keys.insert(&entry.key) || entry.key == "__proto__" {
                    return Err("INVALID_VALUE_KEY");
                }
                result.push(json!({"k":entry.key,"v":encode(&entry.value,ids,handles,resolve)?}));
            }
            json!({"id":id,"o":result})
        }
        EvaluationValue::Reference { id } => {
            if !ids.contains(id) {
                return Err("INVALID_VALUE_REFERENCE");
            }
            json!({"ref":id})
        }
        EvaluationValue::Handle { handle } => {
            if handles.len() >= 256 {
                return Err("HANDLE_CAPACITY");
            }
            let index = handles.len();
            handles.push(resolve(*handle)?);
            json!({"h":index})
        }
        EvaluationValue::Function { .. } => return Err("CALLBACK_ARGUMENT_UNAVAILABLE"),
    })
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 24
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return false;
    }
    let number =
        |start: usize, end: usize| value[start..end].parse::<u16>().expect("ASCII date digits");
    let Ok(month) = time::Month::try_from(number(5, 7) as u8) else {
        return false;
    };
    time::Date::from_calendar_date(number(0, 4) as i32, month, number(8, 10) as u8).is_ok()
        && time::Time::from_hms_milli(
            number(11, 13) as u8,
            number(14, 16) as u8,
            number(17, 19) as u8,
            number(20, 23),
        )
        .is_ok()
}

pub(crate) fn result(value: &Value) -> Result<EvaluationValue, &'static str> {
    schema::encoded_len(value, MAX_BYTES).map_err(|_| "VALUE_BOUNDS")?;
    let result = decode(value, &mut BTreeSet::new())?;
    schema::encoded_len(
        &serde_json::to_value(&result).map_err(|_| "VALUE_BOUNDS")?,
        MAX_BYTES,
    )
    .map_err(|_| "VALUE_BOUNDS")?;
    Ok(result)
}

fn string(value: &Value) -> Result<String, &'static str> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or("INVALID_EVALUATION_VALUE")
}

#[expect(
    clippy::too_many_lines,
    reason = "One exhaustive decoder of the pinned serialized value union"
)]
fn decode(value: &Value, ids: &mut BTreeSet<u32>) -> Result<EvaluationValue, &'static str> {
    let fields = value.as_object().ok_or("INVALID_EVALUATION_VALUE")?;
    let variants: Vec<_> = fields.keys().filter(|key| key.as_str() != "id").collect();
    if variants.len() != 1
        || (fields.contains_key("id") && !fields.contains_key("a") && !fields.contains_key("o"))
    {
        return Err("INVALID_EVALUATION_VALUE");
    }
    Ok(match variants[0].as_str() {
        "n" | "s" | "b" => EvaluationValue::Json {
            value: values::result(value)?,
        },
        "v" => match value["v"].as_str() {
            Some("null") => EvaluationValue::Json { value: Value::Null },
            Some("undefined") => EvaluationValue::Undefined {},
            Some("NaN") => EvaluationValue::Number {
                value: SpecialNumber::Nan,
            },
            Some("Infinity") => EvaluationValue::Number {
                value: SpecialNumber::PositiveInfinity,
            },
            Some("-Infinity") => EvaluationValue::Number {
                value: SpecialNumber::NegativeInfinity,
            },
            Some("-0") => EvaluationValue::Number {
                value: SpecialNumber::NegativeZero,
            },
            _ => return Err("INVALID_EVALUATION_VALUE"),
        },
        "bi" => EvaluationValue::Bigint {
            value: string(&value["bi"])?,
        },
        "d" => EvaluationValue::Date {
            value: string(&value["d"])?,
        },
        "u" => EvaluationValue::Url {
            value: string(&value["u"])?,
        },
        "r" => EvaluationValue::Regexp {
            pattern: string(&value["r"]["p"])?,
            flags: string(&value["r"]["f"])?,
        },
        "e" => EvaluationValue::Error {
            name: string(&value["e"]["n"])?,
            message: string(&value["e"]["m"])?,
            stack: string(&value["e"]["s"])?,
        },
        "ta" => {
            let result = EvaluationValue::TypedArray {
                element: Element::parse(value["ta"]["k"].as_str().ok_or("INVALID_TYPED_ARRAY")?)?,
                base64: string(&value["ta"]["b"])?,
            };
            validate(&result)?;
            result
        }
        "a" | "o" => {
            let id = value["id"]
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or("INVALID_VALUE_REFERENCE")?;
            graph_id(ids, id)?;
            if fields.contains_key("a") {
                EvaluationValue::Array {
                    id,
                    items: value["a"]
                        .as_array()
                        .ok_or("INVALID_EVALUATION_VALUE")?
                        .iter()
                        .map(|value| decode(value, ids))
                        .collect::<Result<_, _>>()?,
                }
            } else {
                let mut keys = BTreeSet::new();
                let mut entries = Vec::new();
                for entry in value["o"].as_array().ok_or("INVALID_EVALUATION_VALUE")? {
                    let key = string(&entry["k"])?;
                    if !keys.insert(key.clone()) {
                        return Err("INVALID_VALUE_KEY");
                    }
                    entries.push(Entry {
                        key,
                        value: decode(&entry["v"], ids)?,
                    });
                }
                EvaluationValue::Object { id, entries }
            }
        }
        "ref" => {
            let id = value["ref"]
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or("INVALID_VALUE_REFERENCE")?;
            if !ids.contains(&id) {
                return Err("INVALID_VALUE_REFERENCE");
            }
            EvaluationValue::Reference { id }
        }
        "fn" => EvaluationValue::Function {
            name: string(&value["fn"])?,
        },
        _ => return Err("INVALID_EVALUATION_VALUE"),
    })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PropertyQuery {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "property_limit")]
    pub limit: usize,
}
fn property_limit() -> usize {
    32
}
impl Default for PropertyQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: property_limit(),
        }
    }
}
impl PropertyQuery {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.offset > 10000 || !(1..=100).contains(&self.limit) {
            return Err("PROPERTY_QUERY_BOUNDS");
        }
        Ok(())
    }
}

pub(crate) fn property_schema() -> Value {
    object(
        json!({"offset":{"type":"integer","minimum":0,"maximum":10000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":32}}),
        &[],
    )
}

fn argument_schema() -> Value {
    json!({"$ref":"#/$defs/browserValue","default":{"kind":"undefined"},"description":"Omission is JavaScript undefined; pass kind:json with value:null for explicit null. Same-document/worker handles are accepted; nodes and primitives may also be adopted between live frames of one owned page. Cross-page/worker arguments refuse; native Chrome refuses cross-renderer node adoption."})
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        object(
            json!({"operation":{"const":"worker_list"},"page":reference}),
            &["operation", "page"],
        ),
        object(
            json!({"operation":{"const":"evaluate"},"scope":reference,"expression":{"type":"string","minLength":1,"maxLength":8192},"isFunction":{"type":["boolean","null"]},"argument":argument_schema(),"returnHandle":{"type":"boolean","default":false}}),
            &["operation", "scope", "expression"],
        ),
        object(
            json!({"operation":{"const":"handle_read"},"handle":reference}),
            &["operation", "handle"],
        ),
        object(
            json!({"operation":{"const":"handle_property"},"handle":reference,"name":{"type":"string","maxLength":1024}}),
            &["operation", "handle", "name"],
        ),
        object(
            json!({"operation":{"const":"handle_properties"},"handle":reference,"query":property_schema()}),
            &["operation", "handle"],
        ),
    ]
}

pub(crate) fn frame_relationship_schemas(reference: &Value) -> Vec<Value> {
    ["handle_owner_frame", "handle_content_frame"]
        .into_iter()
        .map(|operation| {
            object(
                json!({"operation":{"const":operation},"handle":reference}),
                &["operation", "handle"],
            )
        })
        .collect()
}

pub(crate) fn value_schema(reference: &Value) -> Value {
    let text = json!({"type":"string"});
    let id = json!({"type":"integer","minimum":1,"maximum":MAX_ID});
    let recursive = json!({"$ref":"#/$defs/browserValue"});
    let variants = vec![
        object(
            json!({"kind":{"const":"json"},"value":{}}),
            &["kind", "value"],
        ),
        object(json!({"kind":{"const":"undefined"}}), &["kind"]),
        object(
            json!({"kind":{"const":"number"},"value":{"enum":["nan","positive_infinity","negative_infinity","negative_zero"]}}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"bigint"},"value":{"type":"string","description":"Base-10 integer digits with an optional leading minus; validated by the evaluation argument decoder."}}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"date"},"value":text}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"url"},"value":text}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"regexp"},"pattern":text,"flags":text}),
            &["kind", "pattern", "flags"],
        ),
        object(
            json!({"kind":{"const":"error"},"name":text,"message":text,"stack":text}),
            &["kind", "name", "message", "stack"],
        ),
        object(
            json!({"kind":{"const":"typed_array"},"element":{"enum":["int8","uint8","uint8_clamped","int16","uint16","int32","uint32","float32","float64","bigint64","biguint64"]},"base64":text}),
            &["kind", "element", "base64"],
        ),
        object(
            json!({"kind":{"const":"array"},"id":id,"items":{"type":"array","items":recursive}}),
            &["kind", "id", "items"],
        ),
        object(
            json!({"kind":{"const":"object"},"id":id,"entries":{"type":"array","items":object(json!({"key":text,"value":recursive}), &["key","value"])}}),
            &["kind", "id", "entries"],
        ),
        object(
            json!({"kind":{"const":"reference"},"id":id}),
            &["kind", "id"],
        ),
        object(
            json!({"kind":{"const":"handle"},"handle":reference}),
            &["kind", "handle"],
        ),
    ];
    json!({"oneOf":variants,"description":"Bounded evaluation argument; graph IDs are local to this value. Handles require the same execution world, except nodes and primitives adopted between live frames of one owned page. Native cross-renderer nodes and cross-page/worker arguments refuse. Functions cannot be supplied as arguments."})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn array_buffer_public_projection_keeps_injected_only_tags_out_of_the_client_protocol() {
        assert!(
            serde_json::from_value::<EvaluationValue>(json!({"kind":"array_buffer","base64":""}))
                .is_err()
        );
        assert_eq!(
            result(&json!({"ab":{"b":""}})).unwrap_err(),
            "INVALID_EVALUATION_VALUE"
        );
        let wire =
            json!({"id":1,"o":[{"k":"a","v":{"id":2,"o":[]}},{"k":"b","v":{"id":3,"o":[]}}]});
        let value = result(&wire).unwrap();
        assert_eq!(
            argument(&value, |_| panic!("Projection has no handle")).unwrap(),
            json!({"value":wire,"handles":[]})
        );
        let schema = json!({"$ref":"#/$defs/browserValue","$defs":{"browserValue":value_schema(&json!({}))}});
        crate::schema::validate(&schema, &serde_json::to_value(value).unwrap()).unwrap();
    }
    #[test]
    fn omitted_evaluation_arguments_are_undefined_without_changing_null_results() {
        let scope = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        for request in [
            json!({"operation":"evaluate","scope":scope,"expression":"value=>value"}),
            json!({"operation":"selector_evaluate","scope":scope,"options":{"locator":{"kind":"css","selector":"body"},"expression":"(element,value)=>value"}}),
        ] {
            let request: crate::browser::contract::Request =
                serde_json::from_value(request).unwrap();
            let value = match request {
                crate::browser::contract::Request::Evaluate { argument, .. } => argument,
                crate::browser::contract::Request::SelectorEvaluate { options, .. } => {
                    options.argument
                }
                _ => unreachable!(),
            };
            assert_eq!(
                argument(&value, |_| Err("FOREIGN_HANDLE")).unwrap()["value"],
                json!({"v":"undefined"})
            );
        }
        for (wire, public) in [
            (json!({"v":"null"}), json!({"kind":"json","value":null})),
            (json!({"v":"undefined"}), json!({"kind":"undefined"})),
            (
                json!({"id":1,"a":[{"v":"null"},{"v":"undefined"}]}),
                json!({"kind":"array","id":1,"items":[{"kind":"json","value":null},{"kind":"undefined"}]}),
            ),
        ] {
            let decoded = result(&wire).unwrap();
            assert_eq!(serde_json::to_value(&decoded).unwrap(), public);
            assert_eq!(
                argument(&decoded, |_| Err("FOREIGN_HANDLE")).unwrap()["value"],
                wire
            );
        }
    }

    #[test]
    fn special_values_and_cyclic_graphs_preserve_their_types() {
        let wire = json!({"id":1,"o":[
            {"k":"self","v":{"ref":1}},
            {"k":"items","v":{"id":2,"a":[{"v":"undefined"},{"v":"NaN"},{"v":"Infinity"},{"v":"-Infinity"},{"v":"-0"},{"bi":"90071992547409931234"},{"d":"2026-10-25T00:00:00.000Z"},{"u":"https://example.com/"},{"r":{"p":"Acme","f":"gi"}},{"e":{"n":"Error","m":"Acme failure","s":"private stack"}},{"ta":{"k":"ui16","b":"AQACAA=="}}]}},
            {"k":"shared","v":{"ref":2}}
        ]});
        let decoded = result(&wire).unwrap();
        assert_eq!(
            argument(&decoded, |_| unreachable!()).unwrap()["value"],
            wire
        );
        assert!(matches!(
            result(&json!({"fn":"Acme"})).unwrap(),
            EvaluationValue::Function { .. }
        ));
        assert_eq!(
            validate(&EvaluationValue::Function {
                name: "Acme".into()
            }),
            Err("CALLBACK_ARGUMENT_UNAVAILABLE")
        );
    }

    #[test]
    fn malformed_graphs_and_binary_values_are_refused_before_resolution() {
        for wire in [
            json!({"ref":1}),
            json!({"a":[],"id":0}),
            json!({"id":1,"a":[{"id":1,"o":[]}]}),
            json!({"s":"Acme","b":true}),
            json!({"h":0}),
        ] {
            assert!(result(&wire).is_err(), "{wire}");
        }
        for value in [
            EvaluationValue::TypedArray {
                element: Element::Uint16,
                base64: "AQ==".into(),
            },
            EvaluationValue::TypedArray {
                element: Element::Uint8,
                base64: "%%%".into(),
            },
            EvaluationValue::Reference { id: 1 },
            EvaluationValue::Bigint {
                value: "1e6".into(),
            },
            EvaluationValue::Date {
                value: "2026-02-30T00:00:00.000Z".into(),
            },
            EvaluationValue::Regexp {
                pattern: "Acme".into(),
                flags: "ii".into(),
            },
            EvaluationValue::Regexp {
                pattern: "Acme".into(),
                flags: "uv".into(),
            },
            EvaluationValue::Object {
                id: 1,
                entries: vec![Entry {
                    key: "__proto__".into(),
                    value: EvaluationValue::default(),
                }],
            },
        ] {
            assert!(validate(&value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn handle_arguments_are_resolved_separately_from_graph_references() {
        let handle = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let value = EvaluationValue::Array {
            id: 1,
            items: vec![
                EvaluationValue::Handle { handle },
                EvaluationValue::Reference { id: 1 },
            ],
        };
        let argument = argument(&value, |reference| {
            assert_eq!(reference, handle);
            Ok(json!({"guid":"private-wire-id"}))
        })
        .unwrap();
        assert_eq!(
            argument,
            json!({"value":{"id":1,"a":[{"h":0},{"ref":1}]},"handles":[{"guid":"private-wire-id"}]})
        );
        assert_eq!(
            super::argument(&value, |_| Err("STALE_REFERENCE")),
            Err("STALE_REFERENCE")
        );
    }

    #[test]
    fn oversized_results_and_arguments_are_refused_without_truncation() {
        let large = "A".repeat(MAX_BYTES);
        assert_eq!(result(&json!({"s":large})).unwrap_err(), "VALUE_BOUNDS");
        assert_eq!(
            validate(&EvaluationValue::Json {
                value: json!(large)
            }),
            Err("VALUE_BOUNDS")
        );
        assert_eq!(
            validate(&EvaluationValue::Json {
                value: json!(u64::MAX)
            }),
            Err("UNSAFE_INTEGER")
        );
    }
}
