use runtime::rquickjs::{Ctx, Exception, Result};

use crate::ui::elements::{DomError, ElementTagError, StyleError};

pub(super) fn map_dom<T>(
    context: &Ctx<'_>,
    operation: &str,
    result: std::result::Result<T, DomError>,
) -> Result<T> {
    result.map_err(|error| throw_dom(context, operation, error))
}

pub(super) fn map_style<T>(
    context: &Ctx<'_>,
    operation: &str,
    result: std::result::Result<T, StyleError>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let name = match error {
                StyleError::NodeNotFound(_) => "InvalidStateError",
                StyleError::NodeNotElement(_) => "InvalidNodeTypeError",
                StyleError::UnsupportedProperty(_) => "UnsupportedStylePropertyError",
                StyleError::InvalidValue { .. } => "InvalidStyleValueError",
            };
            throw_named(context, name, format!("{operation}: {error}"))
        }
    }
}

pub(super) fn invalid_tag<T>(context: &Ctx<'_>, error: ElementTagError) -> Result<T> {
    Err(Exception::throw_type(context, &error.to_string()))
}

pub(super) fn invalid_token<T>(context: &Ctx<'_>) -> Result<T> {
    throw_named(
        context,
        "InvalidStateError",
        "invalid or malformed native node handle",
    )
}

pub(super) fn borrow_conflict(context: &Ctx<'_>) -> runtime::Error {
    Exception::throw_internal(
        context,
        "the live DOM is already borrowed by reentrant host work",
    )
}

fn throw_dom(context: &Ctx<'_>, operation: &str, error: DomError) -> runtime::Error {
    let name = match error {
        DomError::NodeNotFound(_) => "InvalidStateError",
        DomError::NodeNotElement(_)
        | DomError::NodeNotText(_)
        | DomError::ElementTagMismatch(_)
        | DomError::TextContentNotSupported(_) => "InvalidNodeTypeError",
        DomError::IndexOutOfBounds { .. } | DomError::NotAChild { .. } => "NotFoundError",
        DomError::AppMustBeRoot
        | DomError::InvalidRelationship { .. }
        | DomError::AppAlreadyHasWindow
        | DomError::Cycle { .. }
        | DomError::CannotDetachRoot
        | DomError::CannotRemoveRoot => "HierarchyRequestError",
    };

    match named_exception(context, name, format!("{operation}: {error}")) {
        Ok(exception) => exception.throw(),
        Err(error) => error,
    }
}

fn throw_named<T>(context: &Ctx<'_>, name: &str, message: impl AsRef<str>) -> Result<T> {
    let exception = named_exception(context, name, message)?;
    Err(exception.throw())
}

fn named_exception<'js>(
    context: &Ctx<'js>,
    name: &str,
    message: impl AsRef<str>,
) -> Result<Exception<'js>> {
    let exception = Exception::from_message(context.clone(), message.as_ref())?;
    exception.as_object().set("name", name)?;
    exception.as_object().set("code", name)?;
    Ok(exception)
}
