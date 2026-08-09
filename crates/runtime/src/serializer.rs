//! Serialize Rust values directly into values owned by a QuickJS context.

use rquickjs::{object::Property, Array, Ctx, IntoJs, Object, Value};
use serde::ser::{
    self, Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct,
    SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};
use std::{error, fmt};

/// An error produced while serializing a Rust value into QuickJS.
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
                rquickjs::Error::new_into_js_message("serializable value", "value", message)
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

impl ser::Error for Error {
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

fn define_own_property<'js>(object: &Object<'js>, key: &str, value: Value<'js>) -> Result<()> {
    object.prop(
        key,
        Property::from(value).writable().enumerable().configurable(),
    )?;
    Ok(())
}

/// Serialize a value into the supplied QuickJS context.
pub fn to_value<'js, T>(context: &Ctx<'js>, value: &T) -> Result<Value<'js>>
where
    T: Serialize + ?Sized,
{
    value.serialize(Serializer::new(context.clone()))
}

/// Serialize a struct or map into a QuickJS object.
pub fn to_object<'js, T>(context: &Ctx<'js>, value: &T) -> Result<Object<'js>>
where
    T: Serialize + ?Sized,
{
    let value = to_value(context, value)?;
    value
        .into_object()
        .ok_or_else(|| Error::Message("serialized value is not an object".into()))
}

/// A serde serializer that creates values in one QuickJS context.
#[derive(Clone)]
pub struct Serializer<'js> {
    context: Ctx<'js>,
}

impl<'js> Serializer<'js> {
    pub fn new(context: Ctx<'js>) -> Self {
        Self { context }
    }

    fn convert<T: IntoJs<'js>>(&self, value: T) -> Result<Value<'js>> {
        value.into_js(&self.context).map_err(Error::from)
    }
}

macro_rules! serialize_number {
    ($method:ident, $type:ty) => {
        fn $method(self, value: $type) -> Result<Self::Ok> {
            self.convert(value)
        }
    };
}

impl<'js> ser::Serializer for Serializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;
    type SerializeSeq = SequenceSerializer<'js>;
    type SerializeTuple = SequenceSerializer<'js>;
    type SerializeTupleStruct = SequenceSerializer<'js>;
    type SerializeTupleVariant = TupleVariantSerializer<'js>;
    type SerializeMap = MapSerializer<'js>;
    type SerializeStruct = ObjectSerializer<'js>;
    type SerializeStructVariant = StructVariantSerializer<'js>;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok> {
        self.convert(value)
    }

    serialize_number!(serialize_i8, i8);
    serialize_number!(serialize_i16, i16);
    serialize_number!(serialize_i32, i32);
    serialize_number!(serialize_i64, i64);
    serialize_number!(serialize_u8, u8);
    serialize_number!(serialize_u16, u16);
    serialize_number!(serialize_u32, u32);
    serialize_number!(serialize_u64, u64);
    serialize_number!(serialize_f32, f32);
    serialize_number!(serialize_f64, f64);

    fn serialize_i128(self, value: i128) -> Result<Self::Ok> {
        self.convert(value as f64)
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok> {
        self.convert(value as f64)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok> {
        self.convert(value)
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok> {
        self.convert(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok> {
        let mut sequence = SequenceSerializer::new(self.context, Some(value.len()))?;
        for byte in value {
            SerializeSeq::serialize_element(&mut sequence, byte)?;
        }
        SerializeSeq::end(sequence)
    }

    fn serialize_none(self) -> Result<Self::Ok> {
        Ok(Value::new_undefined(self.context))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok> {
        Ok(Value::new_null(self.context))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        let object = Object::new(self.context.clone())?;
        define_own_property(&object, variant, value.serialize(self)?)?;
        Ok(object.into_value())
    }

    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq> {
        SequenceSerializer::new(self.context, length)
    }

    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(length))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(length))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        TupleVariantSerializer::new(self.context, variant, length)
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap> {
        MapSerializer::new(self.context)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct> {
        ObjectSerializer::new(self.context)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant> {
        StructVariantSerializer::new(self.context, variant)
    }

    fn collect_str<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: fmt::Display + ?Sized,
    {
        self.serialize_str(&value.to_string())
    }
}

pub struct SequenceSerializer<'js> {
    context: Ctx<'js>,
    array: Array<'js>,
    index: usize,
}

impl<'js> SequenceSerializer<'js> {
    fn new(context: Ctx<'js>, _length: Option<usize>) -> Result<Self> {
        Ok(Self {
            array: Array::new(context.clone())?,
            context,
            index: 0,
        })
    }

    fn push<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        let value = value.serialize(Serializer::new(self.context.clone()))?;
        self.array.set(self.index, value)?;
        self.index += 1;
        Ok(())
    }

    fn finish(self) -> Result<Value<'js>> {
        Ok(self.array.into_value())
    }
}

impl<'js> SerializeSeq for SequenceSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok> {
        self.finish()
    }
}

impl<'js> SerializeTuple for SequenceSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok> {
        self.finish()
    }
}

impl<'js> SerializeTupleStruct for SequenceSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok> {
        self.finish()
    }
}

pub struct TupleVariantSerializer<'js> {
    context: Ctx<'js>,
    variant: &'static str,
    sequence: SequenceSerializer<'js>,
}

impl<'js> TupleVariantSerializer<'js> {
    fn new(context: Ctx<'js>, variant: &'static str, length: usize) -> Result<Self> {
        Ok(Self {
            sequence: SequenceSerializer::new(context.clone(), Some(length))?,
            context,
            variant,
        })
    }
}

impl<'js> SerializeTupleVariant for TupleVariantSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.sequence.push(value)
    }

    fn end(self) -> Result<Self::Ok> {
        let object = Object::new(self.context)?;
        define_own_property(&object, self.variant, self.sequence.finish()?)?;
        Ok(object.into_value())
    }
}

pub struct ObjectSerializer<'js> {
    context: Ctx<'js>,
    object: Object<'js>,
}

impl<'js> ObjectSerializer<'js> {
    fn new(context: Ctx<'js>) -> Result<Self> {
        Ok(Self {
            object: Object::new(context.clone())?,
            context,
        })
    }

    fn set<T>(&self, key: &str, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        let value = value.serialize(Serializer::new(self.context.clone()))?;
        define_own_property(&self.object, key, value)
    }

    fn finish(self) -> Result<Value<'js>> {
        Ok(self.object.into_value())
    }
}

impl<'js> SerializeStruct for ObjectSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.set(key, value)
    }

    fn end(self) -> Result<Self::Ok> {
        self.finish()
    }
}

pub struct MapSerializer<'js> {
    object: ObjectSerializer<'js>,
    next_key: Option<String>,
}

impl<'js> MapSerializer<'js> {
    fn new(context: Ctx<'js>) -> Result<Self> {
        Ok(Self {
            object: ObjectSerializer::new(context)?,
            next_key: None,
        })
    }
}

impl<'js> SerializeMap for MapSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        if self.next_key.is_some() {
            return Err(Error::Message(
                "serialize_key called before serialize_value".into(),
            ));
        }
        self.next_key = Some(key.serialize(KeySerializer)?);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| Error::Message("serialize_value called before serialize_key".into()))?;
        self.object.set(&key, value)
    }

    fn end(self) -> Result<Self::Ok> {
        if self.next_key.is_some() {
            return Err(Error::Message(
                "map ended before serializing a value".into(),
            ));
        }
        self.object.finish()
    }
}

pub struct StructVariantSerializer<'js> {
    context: Ctx<'js>,
    variant: &'static str,
    object: ObjectSerializer<'js>,
}

impl<'js> StructVariantSerializer<'js> {
    fn new(context: Ctx<'js>, variant: &'static str) -> Result<Self> {
        Ok(Self {
            object: ObjectSerializer::new(context.clone())?,
            context,
            variant,
        })
    }
}

impl<'js> SerializeStructVariant for StructVariantSerializer<'js> {
    type Ok = Value<'js>;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        self.object.set(key, value)
    }

    fn end(self) -> Result<Self::Ok> {
        let outer = Object::new(self.context)?;
        define_own_property(&outer, self.variant, self.object.finish()?)?;
        Ok(outer.into_value())
    }
}

struct KeySerializer;

macro_rules! serialize_key_number {
    ($method:ident, $type:ty) => {
        fn $method(self, value: $type) -> Result<Self::Ok> {
            Ok(value.to_string())
        }
    };
}

impl ser::Serializer for KeySerializer {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = Impossible<String, Error>;
    type SerializeTuple = Impossible<String, Error>;
    type SerializeTupleStruct = Impossible<String, Error>;
    type SerializeTupleVariant = Impossible<String, Error>;
    type SerializeMap = Impossible<String, Error>;
    type SerializeStruct = Impossible<String, Error>;
    type SerializeStructVariant = Impossible<String, Error>;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok> {
        Ok(value.to_string())
    }

    serialize_key_number!(serialize_i8, i8);
    serialize_key_number!(serialize_i16, i16);
    serialize_key_number!(serialize_i32, i32);
    serialize_key_number!(serialize_i64, i64);
    serialize_key_number!(serialize_i128, i128);
    serialize_key_number!(serialize_u8, u8);
    serialize_key_number!(serialize_u16, u16);
    serialize_key_number!(serialize_u32, u32);
    serialize_key_number!(serialize_u64, u64);
    serialize_key_number!(serialize_u128, u128);
    serialize_key_number!(serialize_f32, f32);
    serialize_key_number!(serialize_f64, f64);

    fn serialize_char(self, value: char) -> Result<Self::Ok> {
        Ok(value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok> {
        Ok(value.to_owned())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok> {
        Err(Error::Message("byte arrays cannot be object keys".into()))
    }

    fn serialize_none(self) -> Result<Self::Ok> {
        Err(Error::Message("none cannot be an object key".into()))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok> {
        Err(Error::Message("unit cannot be an object key".into()))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok> {
        Ok(variant.to_owned())
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok>
    where
        T: Serialize + ?Sized,
    {
        Err(Error::Message(
            "newtype variants cannot be object keys".into(),
        ))
    }

    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::Message("sequences cannot be object keys".into()))
    }

    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple> {
        Err(Error::Message("tuples cannot be object keys".into()))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(Error::Message("tuple structs cannot be object keys".into()))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::Message(
            "tuple variants cannot be object keys".into(),
        ))
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap> {
        Err(Error::Message("maps cannot be object keys".into()))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct> {
        Err(Error::Message("structs cannot be object keys".into()))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::Message(
            "struct variants cannot be object keys".into(),
        ))
    }

    fn collect_str<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: fmt::Display + ?Sized,
    {
        Ok(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};
    use serde::Serialize;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Example<'a> {
        event_type: &'a str,
        position: [f64; 2],
        label: Option<&'a str>,
    }

    #[derive(Serialize)]
    struct RenamedProtoField<'a> {
        #[serde(rename = "__proto__")]
        proto: &'a str,
    }

    #[test]
    fn serializes_a_struct_directly_to_a_quickjs_object() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let object = to_object(
                &context,
                &Example {
                    event_type: "move",
                    position: [12.5, 30.0],
                    label: None,
                },
            )
            .unwrap();

            assert_eq!(object.get::<_, String>("eventType").unwrap(), "move");
            assert_eq!(object.get::<_, Vec<f64>>("position").unwrap(), [12.5, 30.0]);
            let label = object.get::<_, Value>("label").unwrap();
            assert!(label.is_undefined());
        });
    }

    #[test]
    fn serializes_proto_keys_as_own_data_properties() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let struct_object =
                to_object(&context, &RenamedProtoField { proto: "struct" }).unwrap();
            let map_object = to_object(
                &context,
                &[("__proto__", "map")]
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
            .unwrap();

            assert_eq!(
                struct_object
                    .keys::<String>()
                    .collect::<rquickjs::Result<Vec<_>>>()
                    .unwrap(),
                ["__proto__"]
            );
            assert_eq!(
                map_object
                    .keys::<String>()
                    .collect::<rquickjs::Result<Vec<_>>>()
                    .unwrap(),
                ["__proto__"]
            );
            assert_eq!(
                struct_object.get::<_, String>("__proto__").unwrap(),
                "struct"
            );
            assert_eq!(map_object.get::<_, String>("__proto__").unwrap(), "map");
        });
    }
}
