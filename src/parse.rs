//! Shape grammar parser: turns an `Expr` into a `Shape` and runs shape checks
//! against expressions and values.

use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, ShapeId, Symbol, Value};

use crate::ExprKind;
use crate::base::{Shape, ShapeMatch};
use crate::primitives::{
    AnyShape, CaptureShape, ClassShape, ExprKindShape, FieldShape, FieldSpec, ListShape,
    NumberValueShape,
};

/// Build a [`Shape`] from a shape-grammar expression.
///
/// Bare symbols name the built-in atomic shapes (`Any`, `Number`, `String`,
/// `Bool`, `Symbol`, `Map`, `List`, `Nil`, and the `core` `Number` value
/// shape); any other symbol becomes a [`ClassShape`] for that class. Lists
/// drive the combinator grammar: `(capture name Shape)` wraps a shape in a
/// [`CaptureShape`], `(fields ...)` builds an anonymous [`FieldShape`], and any
/// other list becomes a [`ListShape`] over its parsed items.
///
/// This is parser behavior layered on the kernel `Shape` protocol; the kernel
/// owns the protocol, this function owns the concrete grammar.
///
/// # Examples
///
/// ```rust
/// use sim_kernel::{Expr, Symbol};
/// use sim_shape::{Shape, parse_shape_expr};
///
/// let shape = parse_shape_expr(&Expr::Symbol(Symbol::new("Number"))).unwrap();
/// assert!(!shape.is_total());
/// ```
pub fn parse_shape_expr(expr: &Expr) -> Result<Arc<dyn Shape>> {
    // Bound the recursive grammar descent so a pathologically deep expression
    // tree fails closed instead of overflowing the stack.
    let Some(_guard) = crate::recursion::DepthGuard::enter() else {
        return Err(Error::Eval(
            "shape grammar nesting exceeds the recursion budget".to_owned(),
        ));
    };
    match expr {
        Expr::Symbol(symbol) => Ok(match symbol.name.as_ref() {
            "Any" if symbol.namespace.is_none() => Arc::new(AnyShape),
            "Number" if symbol.namespace.is_none() => {
                Arc::new(ExprKindShape::new(ExprKind::Number))
            }
            "Number" if symbol.namespace.as_deref() == Some("core") => Arc::new(NumberValueShape),
            "String" if symbol.namespace.is_none() => {
                Arc::new(ExprKindShape::new(ExprKind::String))
            }
            "Bool" if symbol.namespace.is_none() => Arc::new(ExprKindShape::new(ExprKind::Bool)),
            "Symbol" if symbol.namespace.is_none() => {
                Arc::new(ExprKindShape::new(ExprKind::Symbol))
            }
            "Map" if symbol.namespace.is_none() => Arc::new(ExprKindShape::new(ExprKind::Map)),
            "List" if symbol.namespace.is_none() => Arc::new(ExprKindShape::new(ExprKind::List)),
            "Nil" if symbol.namespace.is_none() => Arc::new(ExprKindShape::new(ExprKind::Nil)),
            _ => Arc::new(ClassShape::new(symbol.clone())),
        }),
        Expr::List(items) => parse_shape_list(items),
        other => Err(Error::Eval(format!(
            "cannot build shape from expression kind {:?}",
            other
        ))),
    }
}

fn parse_shape_list(items: &[Expr]) -> Result<Arc<dyn Shape>> {
    let Some(Expr::Symbol(head)) = items.first() else {
        let items = items
            .iter()
            .map(parse_shape_expr)
            .collect::<Result<Vec<_>>>()?;
        return Ok(Arc::new(ListShape::new(items)));
    };

    if head.namespace.is_none() && head.name.as_ref() == "capture" && items.len() == 3 {
        let Expr::Symbol(name) = &items[1] else {
            return Err(Error::Eval("capture name must be a symbol".to_owned()));
        };
        return Ok(Arc::new(CaptureShape::new(
            name.clone(),
            parse_shape_expr(&items[2])?,
        )));
    }

    if head.namespace.is_none() && head.name.as_ref() == "fields" {
        let specs = items
            .iter()
            .skip(1)
            .map(parse_field_spec_expr)
            .collect::<Result<Vec<_>>>()?;
        return Ok(Arc::new(FieldShape::anonymous(specs)));
    }

    let items = items
        .iter()
        .map(parse_shape_expr)
        .collect::<Result<Vec<_>>>()?;
    Ok(Arc::new(ListShape::new(items)))
}

fn parse_field_spec_expr(expr: &Expr) -> Result<FieldSpec> {
    let Expr::List(items) = expr else {
        return Err(Error::Eval("field shape must be a list".to_owned()));
    };
    let [Expr::Symbol(name), shape] = items.as_slice() else {
        return Err(Error::Eval(
            "field shape must be of the form (:field Shape)".to_owned(),
        ));
    };
    Ok(FieldSpec::required(
        normalize_field_symbol(name),
        parse_shape_expr(shape)?,
    ))
}

fn normalize_field_symbol(symbol: &Symbol) -> Symbol {
    if symbol.namespace.is_none()
        && let Some(stripped) = symbol.name.strip_prefix(':')
    {
        return Symbol::new(stripped.to_owned());
    }
    symbol.clone()
}

/// Check an expression against a shape, returning the resulting match.
///
/// A thin entry point that defers to the shape's own `check_expr`; it exists so
/// callers can drive a shape without depending on the `Shape` trait directly.
pub fn check_shape_on_expr(shape: &dyn Shape, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
    shape.check_expr(cx, expr)
}

/// Check a runtime value against a shape, returning the resulting match.
///
/// A thin entry point that defers to the shape's own `check_value`; it exists so
/// callers can drive a shape without depending on the `Shape` trait directly.
pub fn check_shape_on_value(shape: &dyn Shape, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
    shape.check_value(cx, value)
}

/// Build the `WrongShape` error an expression produces against an expected shape.
///
/// Re-runs the check and packages the rejection diagnostics into an
/// `Error::WrongShape`. Returns a `HostError` instead if the expression is in
/// fact accepted, since there is no error to report in that case.
pub fn shape_error(expected: &dyn Shape, cx: &mut Cx, expr: &Expr) -> Result<Error> {
    let matched = expected.check_expr(cx, expr)?;
    if matched.accepted {
        Err(Error::HostError(
            "shape_error called for an accepted shape".to_owned(),
        ))
    } else {
        Ok(Error::WrongShape {
            expected: expected.id().unwrap_or(ShapeId(0)),
            diagnostics: matched.diagnostics,
        })
    }
}
