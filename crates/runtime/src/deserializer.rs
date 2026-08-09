//! Deserialize values owned by QuickJS into Rust values.

use rquickjs::{object::ObjectIter, Array, Object, Type, Value};
use serde::{
    de::{
        self, value::StringDeserializer, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess,
        SeqAccess, VariantAccess, Visitor,
    },
    Deserialize,
};
use std::{error, fmt, str::FromStr};

/// An error produced while deserializing a QuickJS value.
#[derive(Debug)]
pub enum Error {
    QuickJs(rquickjs::Error),
    Message(String),
}

impl Error {
    /// Convert this error to the runtime's public error type.
    pub fn into_quickjs(self) -> rquickjs::Error {
        match self {
            Self::QuickJs(error) => error,
            Self::Message(message) => {
                rquickjs::Error::new_from_js_message("value", "deserializable value", message)
            }
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuickJs(error) => error.fmt(formatter),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::QuickJs(error) => Some(error),
            Self::Message(_) => None,
        }
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Message(message.to_string())
    }
}

impl From<rquickjs::Error> for Error {
    fn from(error: rquickjs::Error) -> Self {
        Self::QuickJs(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Deserialize a QuickJS value into an owned Rust value.
pub fn from_value<T>(value: Value<'_>) -> Result<T>
where
    T: DeserializeOwned,
{
    T::deserialize(Deserializer::new(value))
}

/// Deserialize a QuickJS object into an owned Rust value.
pub fn from_object<T>(object: Object<'_>) -> Result<T>
where
    T: DeserializeOwned,
{
    from_value(object.into_value())
}

/// A serde deserializer over a value in a QuickJS context.
pub struct Deserializer<'js> {
    value: Value<'js>,
}

impl<'js> Deserializer<'js> {
    pub fn new(value: Value<'js>) -> Self {
        Self { value }
    }

    fn number(&self, expected: &'static str) -> Result<f64> {
        self.value.as_number().ok_or_else(|| {
            Error::Message(format!(
                "expected {expected}, found {}",
                self.value.type_name()
            ))
        })
    }

    fn signed_integer(&self, expected: &'static str, min: f64, max_exclusive: f64) -> Result<f64> {
        let number = self.number(expected)?;
        if number.is_finite() && number.fract() == 0.0 && number >= min && number < max_exclusive {
            Ok(number)
        } else {
            Err(Error::Message(format!(
                "number {number} cannot be represented as {expected}"
            )))
        }
    }

    fn unsigned_integer(&self, expected: &'static str, max_exclusive: f64) -> Result<f64> {
        self.signed_integer(expected, 0.0, max_exclusive)
    }

    fn string(self, expected: &'static str) -> Result<String> {
        let actual = self.value.type_name();
        let string = self
            .value
            .into_string()
            .ok_or_else(|| Error::Message(format!("expected {expected}, found {actual}")))?;
        string.to_string().map_err(Error::from)
    }

    fn array(self, expected: &'static str) -> Result<Array<'js>> {
        let actual = self.value.type_name();
        self.value
            .into_array()
            .ok_or_else(|| Error::Message(format!("expected {expected}, found {actual}")))
    }

    fn object(self, expected: &'static str) -> Result<Object<'js>> {
        let actual = self.value.type_name();
        if self.value.is_array() {
            return Err(Error::Message(format!("expected {expected}, found array")));
        }
        self.value
            .into_object()
            .ok_or_else(|| Error::Message(format!("expected {expected}, found {actual}")))
    }
}

macro_rules! deserialize_signed {
    ($method:ident, $visit:ident, $type:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value>
        where
            V: Visitor<'de>,
        {
            let number = self.signed_integer(
                stringify!($type),
                <$type>::MIN as f64,
                (<$type>::MAX as f64) + 1.0,
            )?;
            visitor.$visit(number as $type)
        }
    };
}

macro_rules! deserialize_unsigned {
    ($method:ident, $visit:ident, $type:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value>
        where
            V: Visitor<'de>,
        {
            let number = self.unsigned_integer(stringify!($type), (<$type>::MAX as f64) + 1.0)?;
            visitor.$visit(number as $type)
        }
    };
}

impl<'de, 'js> de::Deserializer<'de> for Deserializer<'js> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value.type_of() {
            Type::Undefined | Type::Null | Type::Uninitialized => visitor.visit_unit(),
            Type::Bool => visitor.visit_bool(self.value.as_bool().unwrap()),
            Type::Int => visitor.visit_i32(self.value.as_int().unwrap()),
            Type::Float => visitor.visit_f64(self.value.as_float().unwrap()),
            Type::String => visitor.visit_string(self.string("a string")?),
            Type::Array => visitor.visit_seq(ArrayAccess::new(self.array("an array")?)),
            Type::Object => visitor.visit_map(ObjectAccess::new(self.object("an object")?)),
            other => Err(Error::Message(format!(
                "cannot deserialize QuickJS {other} value"
            ))),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let actual = self.value.type_name();
        let value = self
            .value
            .as_bool()
            .ok_or_else(|| Error::Message(format!("expected bool, found {actual}")))?;
        visitor.visit_bool(value)
    }

    deserialize_signed!(deserialize_i8, visit_i8, i8);
    deserialize_signed!(deserialize_i16, visit_i16, i16);
    deserialize_signed!(deserialize_i32, visit_i32, i32);
    deserialize_signed!(deserialize_i64, visit_i64, i64);
    deserialize_signed!(deserialize_i128, visit_i128, i128);
    deserialize_unsigned!(deserialize_u8, visit_u8, u8);
    deserialize_unsigned!(deserialize_u16, visit_u16, u16);
    deserialize_unsigned!(deserialize_u32, visit_u32, u32);
    deserialize_unsigned!(deserialize_u64, visit_u64, u64);
    deserialize_unsigned!(deserialize_u128, visit_u128, u128);

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f32(self.number("f32")? as f32)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f64(self.number("f64")?)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.string("a character")?)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.string("a string")?)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.value.is_null() || self.value.is_undefined() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.value.is_null() || self.value.is_undefined() {
            visitor.visit_unit()
        } else {
            Err(Error::Message(format!(
                "expected null or undefined, found {}",
                self.value.type_name()
            )))
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(ArrayAccess::new(self.array("an array")?))
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(ObjectAccess::new(self.object("an object")?))
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let (variant, value) = if self.value.is_string() {
            (self.string("an enum variant")?, None)
        } else {
            let mut properties = self
                .object("an externally tagged enum")?
                .props::<String, Value>();
            let (variant, value) = properties.next().transpose()?.ok_or_else(|| {
                Error::Message("expected an enum object with exactly one property".into())
            })?;
            if let Some(property) = properties.next() {
                property?;
                return Err(Error::Message(
                    "expected an enum object with exactly one property".into(),
                ));
            }
            (variant, Some(value))
        };
        visitor.visit_enum(EnumDeserializer { variant, value })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct ArrayAccess<'js> {
    array: Array<'js>,
    index: usize,
    length: usize,
}

impl<'js> ArrayAccess<'js> {
    fn new(array: Array<'js>) -> Self {
        let length = array.len();
        Self {
            array,
            index: 0,
            length,
        }
    }
}

impl<'de, 'js> SeqAccess<'de> for ArrayAccess<'js> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        if self.index == self.length {
            return Ok(None);
        }
        let value = self.array.get::<Value>(self.index)?;
        self.index += 1;
        seed.deserialize(Deserializer::new(value)).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.length - self.index)
    }
}

struct ObjectAccess<'js> {
    entries: ObjectIter<'js, String, Value<'js>>,
    value: Option<Value<'js>>,
}

struct MapKeyDeserializer {
    key: String,
}

impl MapKeyDeserializer {
    fn new(key: String) -> Self {
        Self { key }
    }

    fn parse<T>(&self, expected: &'static str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        self.key.parse().map_err(|error| {
            Error::Message(format!(
                "object key {:?} cannot be parsed as {expected}: {error}",
                self.key
            ))
        })
    }
}

macro_rules! deserialize_map_key {
    ($method:ident, $visit:ident, $type:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value>
        where
            V: Visitor<'de>,
        {
            visitor.$visit(self.parse::<$type>(stringify!($type))?)
        }
    };
}

impl<'de> de::Deserializer<'de> for MapKeyDeserializer {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.key)
    }

    deserialize_map_key!(deserialize_bool, visit_bool, bool);
    deserialize_map_key!(deserialize_i8, visit_i8, i8);
    deserialize_map_key!(deserialize_i16, visit_i16, i16);
    deserialize_map_key!(deserialize_i32, visit_i32, i32);
    deserialize_map_key!(deserialize_i64, visit_i64, i64);
    deserialize_map_key!(deserialize_i128, visit_i128, i128);
    deserialize_map_key!(deserialize_u8, visit_u8, u8);
    deserialize_map_key!(deserialize_u16, visit_u16, u16);
    deserialize_map_key!(deserialize_u32, visit_u32, u32);
    deserialize_map_key!(deserialize_u64, visit_u64, u64);
    deserialize_map_key!(deserialize_u128, visit_u128, u128);
    deserialize_map_key!(deserialize_f32, visit_f32, f32);
    deserialize_map_key!(deserialize_f64, visit_f64, f64);
    deserialize_map_key!(deserialize_char, visit_char, char);

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.key)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.key)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_enum(StringDeserializer::<Error>::new(self.key))
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    serde::forward_to_deserialize_any! {
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct ignored_any
    }
}

impl<'js> ObjectAccess<'js> {
    fn new(object: Object<'js>) -> Self {
        Self {
            entries: object.props(),
            value: None,
        }
    }
}

impl<'de, 'js> MapAccess<'de> for ObjectAccess<'js> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(entry) = self.entries.next() else {
            return Ok(None);
        };
        let (key, value) = entry?;
        self.value = Some(value);
        seed.deserialize(MapKeyDeserializer::new(key)).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .ok_or_else(|| Error::Message("value requested before object key".into()))?;
        seed.deserialize(Deserializer::new(value))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

struct EnumDeserializer<'js> {
    variant: String,
    value: Option<Value<'js>>,
}

impl<'de, 'js> EnumAccess<'de> for EnumDeserializer<'js> {
    type Error = Error;
    type Variant = EnumVariant<'js>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(StringDeserializer::<Error>::new(self.variant))?;
        Ok((variant, EnumVariant { value: self.value }))
    }
}

struct EnumVariant<'js> {
    value: Option<Value<'js>>,
}

impl<'de, 'js> VariantAccess<'de> for EnumVariant<'js> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        match self.value {
            None => Ok(()),
            Some(value) => <()>::deserialize(Deserializer::new(value)),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| Error::Message("expected a value for enum variant".into()))?;
        seed.deserialize(Deserializer::new(value))
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| Error::Message("expected an array for tuple variant".into()))?;
        de::Deserializer::deserialize_tuple(Deserializer::new(value), len, visitor)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| Error::Message("expected an object for struct variant".into()))?;
        de::Deserializer::deserialize_struct(Deserializer::new(value), "", fields, visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Example {
        event_type: String,
        position: [f64; 2],
        label: Option<String>,
        flags: BTreeMap<String, bool>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Event {
        Ready,
        Count(u32),
        Move(f64, f64),
        Message { text: String },
    }

    #[test]
    fn deserializes_a_quickjs_object_into_a_struct() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let object = context
                .eval::<Object, _>(
                    "({ eventType: 'move', position: [12.5, 30], label: undefined, flags: { primary: true } })",
                )
                .unwrap();

            assert_eq!(
                from_object::<Example>(object).unwrap(),
                Example {
                    event_type: "move".into(),
                    position: [12.5, 30.0],
                    label: None,
                    flags: [("primary".into(), true)].into(),
                }
            );
        });
    }

    #[test]
    fn deserializes_all_externally_tagged_enum_shapes() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let values = [
                ("'Ready'", Event::Ready),
                ("({ Count: 3 })", Event::Count(3)),
                ("({ Move: [1.5, 2.5] })", Event::Move(1.5, 2.5)),
                (
                    "({ Message: { text: 'hello' } })",
                    Event::Message {
                        text: "hello".into(),
                    },
                ),
            ];

            for (source, expected) in values {
                let value = context.eval::<Value, _>(source).unwrap();
                assert_eq!(from_value::<Event>(value).unwrap(), expected);
            }
        });
    }

    #[test]
    fn rejects_fractional_and_out_of_range_integers() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            for source in ["1.5", "-1", "256"] {
                let value = context.eval::<Value, _>(source).unwrap();
                assert!(from_value::<u8>(value).is_err(), "accepted {source}");
            }
        });
    }

    #[test]
    fn deserializes_numeric_and_boolean_object_keys() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let numbers = context
                .eval::<Value, _>("({ 1: 'one', 42: 'forty-two' })")
                .unwrap();
            let booleans = context.eval::<Value, _>("({ false: 0, true: 1 })").unwrap();

            assert_eq!(
                from_value::<BTreeMap<u32, String>>(numbers).unwrap(),
                [(1, "one".into()), (42, "forty-two".into())].into()
            );
            assert_eq!(
                from_value::<BTreeMap<bool, u8>>(booleans).unwrap(),
                [(false, 0), (true, 1)].into()
            );
        });
    }

    #[test]
    fn round_trips_numeric_and_boolean_map_keys() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let numbers = [(1_u32, "one".to_owned()), (42, "forty-two".to_owned())].into();
            let booleans = [(false, 0_u8), (true, 1)].into();

            let numbers_value = crate::serializer::to_value(&context, &numbers).unwrap();
            let booleans_value = crate::serializer::to_value(&context, &booleans).unwrap();

            assert_eq!(
                from_value::<BTreeMap<u32, String>>(numbers_value).unwrap(),
                numbers
            );
            assert_eq!(
                from_value::<BTreeMap<bool, u8>>(booleans_value).unwrap(),
                booleans
            );
        });
    }

    #[test]
    fn propagates_errors_from_property_getters() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let value = context
                .eval::<Value, _>("({ get eventType() { throw new Error('boom') } })")
                .unwrap();
            assert!(matches!(
                from_value::<BTreeMap<String, String>>(value),
                Err(Error::QuickJs(_))
            ));
        });
    }
}
