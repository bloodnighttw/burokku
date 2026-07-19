use runtime::rquickjs::{Ctx, Error, Function, Result};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

use super::document::{ElementKind, UiMutation, UiStyleValue, UiUpdate};

pub fn install<'js>(context: &Ctx<'js>, sender: UnboundedSender<UiUpdate>) -> Result<()> {
    let clock_origin = Instant::now();
    let now = Function::new(context.clone(), move || {
        clock_origin.elapsed().as_secs_f64() * 1_000.0
    })?;
    context.globals().set("__burokku_now", now)?;

    let mutation_sender = sender.clone();
    let create = Function::new(context.clone(), move |id: u64, name: String| {
        let kind = ElementKind::from_name(&name).ok_or_else(|| {
            Error::new_from_js_message("string", "ElementKind", "unsupported UI element")
        })?;
        let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::Create { id, kind }));
        Ok::<_, Error>(())
    })?;
    context.globals().set("__burokku_create", create)?;

    let mutation_sender = sender.clone();
    let set_text = Function::new(context.clone(), move |id: u64, text: String| {
        let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::SetText { id, text }));
    })?;
    context.globals().set("__burokku_set_text", set_text)?;

    let mutation_sender = sender.clone();
    let set_number = Function::new(
        context.clone(),
        move |id: u64, name: String, value: f64| -> Result<()> {
            if !value.is_finite() || value < -(f32::MAX as f64) || value > f32::MAX as f64 {
                return Err(Error::new_from_js_message(
                    "number",
                    "f32",
                    "style value must be a finite 32-bit number",
                ));
            }
            let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::SetStyle {
                id,
                name,
                value: UiStyleValue::Number(value as f32),
            }));
            Ok(())
        },
    )?;
    context
        .globals()
        .set("__burokku_set_style_number", set_number)?;

    let mutation_sender = sender.clone();
    let set_string = Function::new(
        context.clone(),
        move |id: u64, name: String, value: String| {
            let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::SetStyle {
                id,
                name,
                value: UiStyleValue::String(value),
            }));
        },
    )?;
    context
        .globals()
        .set("__burokku_set_style_string", set_string)?;

    let mutation_sender = sender.clone();
    let set_color = Function::new(
        context.clone(),
        move |id: u64, name: String, red: u8, green: u8, blue: u8, alpha: u8| {
            let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::SetStyle {
                id,
                name,
                value: UiStyleValue::Color([red, green, blue, alpha]),
            }));
        },
    )?;
    context
        .globals()
        .set("__burokku_set_style_color", set_color)?;

    let mutation_sender = sender.clone();
    let clear_style = Function::new(context.clone(), move |id: u64, name: String| {
        let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::ClearStyle { id, name }));
    })?;
    context
        .globals()
        .set("__burokku_clear_style", clear_style)?;

    let mutation_sender = sender.clone();
    let insert = Function::new(
        context.clone(),
        move |parent: u64, child: u64, before: i64| {
            let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::Insert {
                parent,
                child,
                before: u64::try_from(before).ok(),
            }));
        },
    )?;
    context.globals().set("__burokku_insert", insert)?;

    let mutation_sender = sender.clone();
    let remove = Function::new(context.clone(), move |parent: u64, child: u64| {
        let _ = mutation_sender.send(UiUpdate::Mutation(UiMutation::Remove { parent, child }));
    })?;
    context.globals().set("__burokku_remove", remove)?;

    let flush = Function::new(context.clone(), move |commit_id: u64| {
        let _ = sender.send(UiUpdate::Flush(commit_id));
    })?;
    context.globals().set("__burokku_flush", flush)?;
    Ok(())
}
