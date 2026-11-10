//! Encode typed evidence directly into its redacted JSON projection.
//! Secret fields are discarded before traversing their values. Dynamic input
//! remains borrowed, rather than becoming a second unredacted output tree.
use serde::{Serialize, ser};
use serde_json::{Map, Value};
use std::cell::Cell;

type Error = serde_json::Error;
type Result<T> = std::result::Result<T, Error>;
const SECRET: &str = "[REDACTED]";
const SENSITIVE: &str = "ShieldOwnedSensitiveEvidence";

/// A typed owner's explicit secret annotation. Ordinary raw serializers retain
/// the original evidence; this diagnostic encoder discards it before traversal.
pub(crate) struct Sensitive<T>(pub(crate) T);
impl<T: Serialize> Serialize for Sensitive<T> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(SENSITIVE, &self.0)
    }
}

#[cfg(test)]
fn encode<T: Serialize + ?Sized>(value: &T) -> Result<(Value, bool)> {
    encode_controlled(value, usize::MAX, &|| Ok(()))
}

#[cfg(test)]
fn encode_controlled<T: Serialize + ?Sized>(
    value: &T,
    limit: usize,
    checkpoint: &dyn Fn() -> Result<()>,
) -> Result<(Value, bool)> {
    encode_controlled_with_sources(value, limit, checkpoint, &[])
}

pub(crate) fn encode_controlled_with_sources<T: Serialize + ?Sized>(
    value: &T,
    limit: usize,
    checkpoint: &dyn Fn() -> Result<()>,
    sensitive_sources: &[&str],
) -> Result<(Value, bool)> {
    let changed = Cell::new(false);
    let budget = Budget {
        used: Cell::new(0),
        limit,
        checkpoint,
        sensitive_sources,
    };
    budget.charge(0)?;
    let value = value.serialize(Encoder {
        changed: &changed,
        budget: &budget,
        mask: Mask::None,
        argument_secret: None,
    })?;
    Ok((value, changed.get()))
}

struct Budget<'a> {
    used: Cell<usize>,
    limit: usize,
    checkpoint: &'a dyn Fn() -> Result<()>,
    sensitive_sources: &'a [&'a str],
}
impl Budget<'_> {
    fn charge(&self, bytes: usize) -> Result<()> {
        (self.checkpoint)()?;
        self.used.set(
            self.used
                .get()
                .checked_add(bytes)
                .filter(|size| *size <= self.limit)
                .ok_or_else(|| {
                    <Error as ser::Error>::custom("diagnostic projection exceeds output limit")
                })?,
        );
        Ok(())
    }
    fn scalar(&self, value: &Value) -> Result<()> {
        self.charge(crate::json::encoded_len(value)?)
    }
    fn wrapper(&self, variant: Option<&str>) -> Result<()> {
        if let Some(name) = variant {
            self.charge(3 + crate::json::encoded_len(&name)?)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Mask {
    None,
    NonNull,
    All,
}

struct Encoder<'a> {
    changed: &'a Cell<bool>,
    budget: &'a Budget<'a>,
    mask: Mask,
    // Only the array element's own string can consume the next argument.
    argument_secret: Option<&'a Cell<bool>>,
}

impl<'a> Encoder<'a> {
    fn output(self, value: Value) -> Result<Value> {
        if matches!(self.mask, Mask::All)
            || (matches!(self.mask, Mask::NonNull) && !value.is_null())
        {
            self.changed.set(true);
            self.budget.charge(12)?;
            Ok(Value::String(SECRET.into()))
        } else {
            self.budget.scalar(&value)?;
            Ok(value)
        }
    }
    fn sequence(self) -> Sequence<'a> {
        Sequence {
            changed: self.changed,
            budget: self.budget,
            masked: !matches!(self.mask, Mask::None),
            values: Vec::new(),
            next_secret: Cell::new(false),
            variant: None,
        }
    }
    fn object(self) -> Object<'a> {
        Object {
            changed: self.changed,
            budget: self.budget,
            masked: !matches!(self.mask, Mask::None),
            fields: Map::new(),
            key: None,
            variant: None,
        }
    }
}

macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {$ (
        fn $method(self, value: $ty) -> Result<Value> {
            self.output(value.serialize(serde_json::value::Serializer)?)
        }
    )*};
}

impl<'a> ser::Serializer for Encoder<'a> {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = Sequence<'a>;
    type SerializeTuple = Sequence<'a>;
    type SerializeTupleStruct = Sequence<'a>;
    type SerializeTupleVariant = Sequence<'a>;
    type SerializeMap = Object<'a>;
    type SerializeStruct = Object<'a>;
    type SerializeStructVariant = Object<'a>;

    scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char)
    );

    fn serialize_str(self, value: &str) -> Result<Value> {
        if let Some(flag) = self.argument_secret {
            flag.set(
                value.starts_with("--") && super::sensitive_key(value.trim_start_matches('-')),
            );
        }
        if !matches!(self.mask, Mask::None) {
            return self.output(Value::Bool(false));
        }
        if !value.is_empty() && self.budget.sensitive_sources.contains(&value) {
            self.changed.set(true);
            self.budget.charge(12)?;
            return Ok(Value::String(SECRET.into()));
        }
        // Reuse the credential grammar, but allocate only this leaf, never an
        // unredacted enclosing report. Safe whitespace is preserved exactly.
        let mut value = Value::String(value.into());
        if super::redact(&mut value) {
            self.changed.set(true);
        }
        self.budget.scalar(&value)?;
        Ok(value)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Value> {
        if !matches!(self.mask, Mask::None) {
            return self.output(Value::Bool(false));
        }
        value.serialize(self)
    }
    fn serialize_none(self) -> Result<Value> {
        self.serialize_unit()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value> {
        self.output(Value::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Value> {
        if name == SENSITIVE {
            self.changed.set(true);
            self.budget.charge(12)?;
            return Ok(Value::String(SECRET.into()));
        }
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value> {
        let mut object = self.object();
        object.field(variant, value)?;
        object.finish()
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(self.sequence())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Ok(self.sequence())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(self.sequence())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        let mut sequence = self.sequence();
        sequence.variant = Some(variant);
        Ok(sequence)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(self.object())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        Ok(self.object())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        let mut object = self.object();
        object.variant = Some(variant);
        Ok(object)
    }
    fn collect_str<T: std::fmt::Display + ?Sized>(self, value: &T) -> Result<Value> {
        self.serialize_str(&value.to_string())
    }
}

struct Sequence<'a> {
    changed: &'a Cell<bool>,
    budget: &'a Budget<'a>,
    masked: bool,
    values: Vec<Value>,
    next_secret: Cell<bool>,
    variant: Option<&'static str>,
}
impl Sequence<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        if self.masked {
            return Ok(());
        }
        self.budget.charge(usize::from(!self.values.is_empty()))?;
        let mask = if self.next_secret.replace(false) {
            Mask::All
        } else {
            Mask::None
        };
        let value = value.serialize(Encoder {
            changed: self.changed,
            budget: self.budget,
            mask,
            argument_secret: Some(&self.next_secret),
        })?;
        // A consumed secret never becomes the next option, even if its own
        // string happens to look like a credential flag.
        if matches!(mask, Mask::All) {
            self.next_secret.set(false);
        }
        self.values.push(value);
        Ok(())
    }
    fn finish(self) -> Result<Value> {
        if self.masked {
            self.changed.set(true);
            self.budget.charge(12)?;
            return Ok(Value::String(SECRET.into()));
        }
        self.budget.charge(2)?;
        self.budget.wrapper(self.variant)?;
        let value = Value::Array(self.values);
        Ok(match self.variant {
            Some(name) => Value::Object(Map::from_iter([(name.into(), value)])),
            None => value,
        })
    }
}
macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Sequence<'_> {
            type Ok = Value;
            type Error = Error;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
                self.element(value)
            }
            fn end(self) -> Result<Value> {
                self.finish()
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);

struct Object<'a> {
    changed: &'a Cell<bool>,
    budget: &'a Budget<'a>,
    masked: bool,
    fields: Map<String, Value>,
    key: Option<String>,
    variant: Option<&'static str>,
}
impl Object<'_> {
    fn field<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<()> {
        if self.masked {
            return Ok(());
        }
        self.budget
            .charge(crate::json::encoded_len(&key)? + 1 + usize::from(!self.fields.is_empty()))?;
        let mask = if super::sensitive_key(key) {
            Mask::NonNull
        } else {
            Mask::None
        };
        let value = value.serialize(Encoder {
            changed: self.changed,
            budget: self.budget,
            mask,
            argument_secret: None,
        })?;
        self.fields.insert(key.into(), value);
        Ok(())
    }
    fn finish(self) -> Result<Value> {
        if self.masked {
            self.changed.set(true);
            self.budget.charge(12)?;
            return Ok(Value::String(SECRET.into()));
        }
        self.budget.charge(2)?;
        self.budget.wrapper(self.variant)?;
        let value = Value::Object(self.fields);
        Ok(match self.variant {
            Some(name) => Value::Object(Map::from_iter([(name.into(), value)])),
            None => value,
        })
    }
}
impl ser::SerializeMap for Object<'_> {
    type Ok = Value;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        if self.masked {
            return Ok(());
        }
        self.key = Some(match serde_json::to_value(key)? {
            Value::String(key) => key,
            Value::Bool(key) => key.to_string(),
            Value::Number(key) => key.to_string(),
            _ => return Err(ser::Error::custom("JSON object key must be a string")),
        });
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        if self.masked {
            return Ok(());
        }
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("missing object key"))?;
        self.field(&key, value)
    }
    fn end(self) -> Result<Value> {
        self.finish()
    }
}
impl ser::SerializeStruct for Object<'_> {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.field(key, value)
    }
    fn end(self) -> Result<Value> {
        self.finish()
    }
}
impl ser::SerializeStructVariant for Object<'_> {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.field(key, value)
    }
    fn end(self) -> Result<Value> {
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounded_projection_charges_escaped_utf8_and_stops_before_later_fields() {
        struct Later;
        impl Serialize for Later {
            fn serialize<S: ser::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
                panic!("visited after exhausted budget")
            }
        }
        for value in [
            json!({"é":"\n\t\\\"", "x":[false,null,"😃"]}),
            json!(["--token", "hidden"]),
        ] {
            let (expected, _) = encode(&value).unwrap();
            let length = expected.to_string().len();
            assert_eq!(
                encode_controlled(&value, length, &|| Ok(())).unwrap().0,
                expected
            );
            assert!(encode_controlled(&value, length - 1, &|| Ok(())).is_err());
        }
        assert!(encode_controlled(&("x".repeat(100), Later), 16, &|| Ok(())).is_err());
        assert!(
            encode_controlled(&Later, 16, &|| Err(<Error as ser::Error>::custom(
                "cancelled"
            )))
            .is_err()
        );
    }

    #[test]
    fn direct_projection_matches_existing_redaction_for_nested_secret_shapes() {
        for original in [
            json!({"password":null,"token":{"arbitrary":["hidden"]},"safe":"  text\t\n"}),
            json!({"arguments":["--token","--secret","visible","--token=hidden","--token",null]}),
            json!({"original_input":{"Authorization":"Bearer hidden","source":"safe --api-key=hidden"}}),
            json!(["--token",{"nested":"hidden"},"  safe\twords",["--secret","hidden"]]),
        ] {
            let mut expected = original.clone();
            let changed = super::super::redact(&mut expected);
            assert_eq!(encode(&original).unwrap(), (expected, changed));
        }
    }

    #[test]
    fn secret_compound_is_never_traversed() {
        struct Forbidden;
        impl Serialize for Forbidden {
            fn serialize<S: ser::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut fields = serializer.serialize_struct("Forbidden", 1)?;
                ser::SerializeStruct::serialize_field(&mut fields, "child", &Panic)?;
                ser::SerializeStruct::end(fields)
            }
        }
        struct Panic;
        impl Serialize for Panic {
            fn serialize<S: ser::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
                panic!("secret child visited")
            }
        }
        #[derive(Serialize)]
        struct Report {
            password: Forbidden,
        }
        assert_eq!(
            encode(&Report {
                password: Forbidden
            })
            .unwrap(),
            (json!({"password":SECRET}), true)
        );
        assert_eq!(encode(&Sensitive(Panic)).unwrap(), (json!(SECRET), true));
        assert_eq!(
            serde_json::to_value(Sensitive("raw owned evidence")).unwrap(),
            json!("raw owned evidence")
        );
        assert_eq!(
            encode(&json!({SENSITIVE:"submitted literal key"})).unwrap(),
            (json!({SENSITIVE:"submitted literal key"}), false)
        );
    }
}
