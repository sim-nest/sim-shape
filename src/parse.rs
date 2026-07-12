//! Shape grammar parser: turns an `Expr` into a `Shape` and runs shape checks
//! against expressions and values.

use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, ShapeId, Symbol, Value};

use crate::ExprKind;
use crate::algebra::{
    AndShape, NotShape, OrShape, RepeatShape, TableExtraPolicy, TableFieldSpec, TableShape,
};
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
/// [`CaptureShape`], `(fields ...)` builds an anonymous [`FieldShape`],
/// `(and ...)`/`(or ...)`/`(not ...)` build boolean algebra, `(list-rest ...)`
/// and `(repeat...)` build collection algebra, `(table-open ...)` and
/// `(table-closed ...)` build table algebra, and any other list becomes a
/// [`ListShape`] over its parsed items.
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
        return parse_tuple_shape(items);
    };

    match shape_form_name(head) {
        Some("and" | "all") => Ok(Arc::new(AndShape::new(parse_shape_items(
            shape_sequence_args(head, &items[1..]),
        )?))),
        Some("or" | "any") => Ok(Arc::new(OrShape::new(parse_shape_items(
            shape_sequence_args(head, &items[1..]),
        )?))),
        Some("not" | "none") => {
            expect_arity(head, items, 2)?;
            Ok(Arc::new(NotShape::new(parse_shape_expr(&items[1])?)))
        }
        Some("capture") => parse_capture_shape(head, items),
        Some("fields") => parse_fields_shape(&items[1..]),
        Some("list") => Ok(Arc::new(ListShape::new(parse_shape_items(
            shape_sequence_args(head, &items[1..]),
        )?))),
        Some("list-rest") => parse_list_rest_shape(head, items),
        Some("repeat") => {
            expect_arity(head, items, 2)?;
            Ok(Arc::new(RepeatShape::new(parse_shape_expr(&items[1])?)))
        }
        Some("repeat-bounds") => parse_repeat_bounds_shape(head, items),
        Some("table") => parse_single_table_shape(head, items),
        Some("table-required" | "table-open") => {
            parse_table_shape(&items[1..], TableExtraPolicy::Allow)
        }
        Some("table-closed") => parse_table_shape(&items[1..], TableExtraPolicy::Reject),
        Some("without" | "difference") => parse_without_shape(head, items),
        _ => parse_tuple_shape(items),
    }
}

fn parse_tuple_shape(items: &[Expr]) -> Result<Arc<dyn Shape>> {
    let items = items
        .iter()
        .map(parse_shape_expr)
        .collect::<Result<Vec<_>>>()?;
    Ok(Arc::new(ListShape::new(items)))
}

fn parse_shape_items(items: &[Expr]) -> Result<Vec<Arc<dyn Shape>>> {
    items.iter().map(parse_shape_expr).collect()
}

fn shape_sequence_args<'a>(head: &Symbol, items: &'a [Expr]) -> &'a [Expr] {
    if head.namespace.as_deref() == Some("shape")
        && let [Expr::List(shapes)] = items
    {
        return shapes;
    }
    items
}

fn parse_capture_shape(head: &Symbol, items: &[Expr]) -> Result<Arc<dyn Shape>> {
    expect_arity(head, items, 3)?;
    let Expr::Symbol(name) = &items[1] else {
        return Err(Error::Eval("capture name must be a symbol".to_owned()));
    };
    Ok(Arc::new(CaptureShape::new(
        name.clone(),
        parse_shape_expr(&items[2])?,
    )))
}

fn parse_fields_shape(items: &[Expr]) -> Result<Arc<dyn Shape>> {
    let specs = items
        .iter()
        .map(parse_field_spec_expr)
        .collect::<Result<Vec<_>>>()?;
    Ok(Arc::new(FieldShape::anonymous(specs)))
}

fn parse_list_rest_shape(head: &Symbol, items: &[Expr]) -> Result<Arc<dyn Shape>> {
    expect_arity(head, items, 3)?;
    let Expr::List(prefix) = &items[1] else {
        return Err(Error::Eval(
            "list-rest prefix must be a list of shapes".to_owned(),
        ));
    };
    Ok(Arc::new(ListShape::with_rest(
        parse_shape_items(prefix)?,
        parse_shape_expr(&items[2])?,
    )))
}

fn parse_repeat_bounds_shape(head: &Symbol, items: &[Expr]) -> Result<Arc<dyn Shape>> {
    expect_arity(head, items, 4)?;
    let min = parse_usize_expr(&items[2], "repeat-bounds min")?;
    let max = parse_optional_usize_expr(&items[3], "repeat-bounds max")?;
    if matches!(max, Some(max) if max < min) {
        return Err(Error::Eval(
            "repeat-bounds max must be greater than or equal to min".to_owned(),
        ));
    }
    Ok(Arc::new(RepeatShape::with_bounds(
        parse_shape_expr(&items[1])?,
        min,
        max,
    )))
}

fn parse_table_shape(items: &[Expr], extra: TableExtraPolicy) -> Result<Arc<dyn Shape>> {
    let fields = table_field_exprs(items)
        .iter()
        .map(parse_table_field_spec_expr)
        .collect::<Result<Vec<_>>>()?;
    Ok(Arc::new(TableShape::new(fields, extra)))
}

fn parse_single_table_shape(head: &Symbol, items: &[Expr]) -> Result<Arc<dyn Shape>> {
    expect_arity(head, items, 3)?;
    let Expr::Symbol(name) = &items[1] else {
        return Err(Error::Eval("table key must be a symbol".to_owned()));
    };
    Ok(Arc::new(TableShape::single(
        normalize_field_symbol(name),
        parse_shape_expr(&items[2])?,
    )))
}

fn table_field_exprs(items: &[Expr]) -> &[Expr] {
    if let [Expr::List(fields)] = items
        && (fields.is_empty() || fields.iter().all(is_table_field_expr))
    {
        return fields;
    }
    items
}

fn is_table_field_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::List(items) if matches!(items.as_slice(), [Expr::Symbol(_), _]))
}

fn parse_table_field_spec_expr(expr: &Expr) -> Result<TableFieldSpec> {
    let Expr::List(items) = expr else {
        return Err(Error::Eval("table field shape must be a list".to_owned()));
    };
    let [Expr::Symbol(name), shape] = items.as_slice() else {
        return Err(Error::Eval(
            "table field shape must be of the form (:field Shape)".to_owned(),
        ));
    };
    Ok(TableFieldSpec {
        key: normalize_field_symbol(name),
        shape: parse_shape_expr(shape)?,
        required: true,
    })
}

fn parse_without_shape(head: &Symbol, items: &[Expr]) -> Result<Arc<dyn Shape>> {
    expect_arity(head, items, 3)?;
    let left = parse_shape_expr(&items[1])?;
    let right = parse_shape_expr(&items[2])?;
    let negated_right: Arc<dyn Shape> = Arc::new(NotShape::new(right));
    Ok(Arc::new(AndShape::new(vec![left, negated_right])))
}

fn parse_optional_usize_expr(expr: &Expr, context: &str) -> Result<Option<usize>> {
    if matches!(expr, Expr::Nil) {
        Ok(None)
    } else {
        parse_usize_expr(expr, context).map(Some)
    }
}

fn parse_usize_expr(expr: &Expr, context: &str) -> Result<usize> {
    let Expr::Number(number) = expr else {
        return Err(Error::Eval(format!("{context} expects a number")));
    };
    number
        .canonical
        .parse::<usize>()
        .map_err(|_| Error::Eval(format!("{context} expects a non-negative integer")))
}

fn shape_form_name(symbol: &Symbol) -> Option<&str> {
    if symbol.namespace.is_none() || symbol.namespace.as_deref() == Some("shape") {
        Some(symbol.name.as_ref())
    } else {
        None
    }
}

fn expect_arity(head: &Symbol, items: &[Expr], expected: usize) -> Result<()> {
    if items.len() == expected {
        Ok(())
    } else {
        Err(Error::Eval(format!(
            "{head} expects {} argument(s), got {}",
            expected - 1,
            items.len().saturating_sub(1)
        )))
    }
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
