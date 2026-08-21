use rquickjs::{Ctx, FromJs, IntoJs, Value};

/// An optional JavaScript value that preserves the distinction between
/// `undefined` and `null`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JsOptions<T> {
    /// JavaScript `undefined`.
    #[default]
    Undefined,
    /// JavaScript `null`.
    Null,
    /// A present value.
    Some(T),
}

impl<T> JsOptions<T> {
    pub const fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }

    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub const fn is_some(&self) -> bool {
        matches!(self, Self::Some(_))
    }

    pub const fn as_ref(&self) -> JsOptions<&T> {
        match self {
            Self::Undefined => JsOptions::Undefined,
            Self::Null => JsOptions::Null,
            Self::Some(value) => JsOptions::Some(value),
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> JsOptions<U> {
        match self {
            Self::Undefined => JsOptions::Undefined,
            Self::Null => JsOptions::Null,
            Self::Some(value) => JsOptions::Some(map(value)),
        }
    }
}

impl<'js, T> FromJs<'js> for JsOptions<T>
where
    T: FromJs<'js>,
{
    fn from_js(context: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<Self> {
        if value.is_undefined() {
            Ok(Self::Undefined)
        } else if value.is_null() {
            Ok(Self::Null)
        } else {
            T::from_js(context, value).map(Self::Some)
        }
    }
}

impl<'js, T> IntoJs<'js> for JsOptions<T>
where
    T: IntoJs<'js>,
{
    fn into_js(self, context: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        match self {
            Self::Undefined => Ok(Value::new_undefined(context.clone())),
            Self::Null => Ok(Value::new_null(context.clone())),
            Self::Some(value) => value.into_js(context),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JsOptions;
    use rquickjs::{Context, Runtime};

    #[test]
    fn converts_to_distinct_javascript_values() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let globals = context.globals();
            globals
                .set("rustUndefined", JsOptions::<String>::Undefined)
                .unwrap();
            globals.set("rustNull", JsOptions::<String>::Null).unwrap();
            globals
                .set("rustSome", JsOptions::Some("value".to_owned()))
                .unwrap();

            let states: Vec<bool> = context
                .eval(
                    r#"[
                        rustUndefined === undefined,
                        rustNull === null,
                        rustSome === "value"
                    ]"#,
                )
                .unwrap();
            assert_eq!(states, [true, true, true]);
        });
    }

    #[test]
    fn converts_from_distinct_javascript_values() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let undefined: JsOptions<String> = context.eval("undefined").unwrap();
            let null: JsOptions<String> = context.eval("null").unwrap();
            let some: JsOptions<String> = context.eval(r#""value""#).unwrap();

            assert_eq!(undefined, JsOptions::Undefined);
            assert_eq!(null, JsOptions::Null);
            assert_eq!(some, JsOptions::Some("value".to_owned()));
        });
    }
}
