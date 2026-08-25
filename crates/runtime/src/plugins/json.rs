use crate::{JsOptions, Plugin, Result};
use rquickjs::{
    function::{Args, Opt},
    Coerced, Ctx, Function, Object, Value,
};

/// Installs the standard `JSON` global with `parse` and `stringify` methods.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonPlugin;

impl Plugin for JsonPlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        let json = Object::new(context.clone())?;

        let parse = Function::new(context.clone(), parse_json)?.with_name("parse")?;
        parse.set_length(2)?;
        json.set("parse", parse)?;

        let stringify = Function::new(context.clone(), stringify_json)?.with_name("stringify")?;
        stringify.set_length(3)?;
        json.set("stringify", stringify)?;

        context.globals().set("JSON", json)?;
        Ok(())
    }
}

fn parse_json<'js>(
    context: Ctx<'js>,
    source: Opt<Coerced<String>>,
    reviver: Opt<Value<'js>>,
) -> Result<Value<'js>> {
    let source = source
        .0
        .map(|source| source.0)
        .unwrap_or_else(|| "undefined".into());
    let parsed = context.json_parse(source)?;
    let Some(reviver) = reviver.0.as_ref().and_then(Value::as_function).cloned() else {
        return Ok(parsed);
    };

    let root = Object::new(context)?;
    root.set("", parsed)?;
    internalize_json_property(&root, "", &reviver)
}

fn internalize_json_property<'js>(
    holder: &Object<'js>,
    key: &str,
    reviver: &Function<'js>,
) -> Result<Value<'js>> {
    let value: Value = holder.get(key)?;

    if let Some(object) = value.as_object() {
        if let Some(array) = object.clone().into_array() {
            for index in 0..array.len() {
                let index = index.to_string();
                let revived = internalize_json_property(array.as_object(), &index, reviver)?;
                if revived.is_undefined() {
                    array.as_object().remove(index)?;
                } else {
                    array.as_object().set(index, revived)?;
                }
            }
        } else {
            let keys = object.keys::<String>().collect::<Result<Vec<_>>>()?;
            for key in keys {
                let revived = internalize_json_property(object, &key, reviver)?;
                if revived.is_undefined() {
                    object.remove(key)?;
                } else {
                    object.set(key, revived)?;
                }
            }
        }
    }

    let mut arguments = Args::new(holder.ctx().clone(), 2);
    arguments.this(holder.clone())?;
    arguments.push_arg(key)?;
    arguments.push_arg(value)?;
    arguments.apply(reviver)
}

fn stringify_json<'js>(
    context: Ctx<'js>,
    value: Opt<Value<'js>>,
    replacer: Opt<Value<'js>>,
    space: Opt<Value<'js>>,
) -> Result<JsOptions<rquickjs::String<'js>>> {
    let value = value
        .0
        .unwrap_or_else(|| Value::new_undefined(context.clone()));
    let replacer = replacer
        .0
        .unwrap_or_else(|| Value::new_undefined(context.clone()));
    let space = space
        .0
        .unwrap_or_else(|| Value::new_undefined(context.clone()));

    Ok(
        match context.json_stringify_replacer_space(value, replacer, space)? {
            Some(json) => JsOptions::Some(json),
            None => JsOptions::Undefined,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::JsonPlugin;
    use crate::Runtime;

    #[tokio::test(flavor = "current_thread")]
    async fn parses_and_stringifies_json() {
        let runtime = Runtime::builder().plugin(JsonPlugin).build().await.unwrap();

        let output: Vec<String> = runtime.eval(include_str!("scripts/json.js")).await.unwrap();

        assert_eq!(
            output,
            [
                "{\n  \"parsed\": {\n    \"count\": 3\n  }\n}",
                "3:10:false:30",
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn matches_json_edge_behavior() {
        let runtime = Runtime::builder().plugin(JsonPlugin).build().await.unwrap();

        let behavior: Vec<bool> = runtime
            .eval(include_str!("scripts/json_edge_behavior.js"))
            .await
            .unwrap();

        assert_eq!(behavior, [true, true, true, true]);
    }
}
