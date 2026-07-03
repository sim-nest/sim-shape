//! Codec helpers for shape citizens: encode shapes to constructor expressions
//! and decode constructor fields (symbols, expr kinds, shape lists, table
//! fields, extra policies) back into shape values.

use std::sync::Arc;

use sim_citizen::{CitizenField, field_error, value_from_expr};
use sim_kernel::{
    Cx, Error, Expr, ObjectEncode, ObjectEncoding, Result, Shape, Symbol, Value, force_list_to_vec,
};

use crate::{
    AndShape, AnyShape, ClassShape, ExactExprShape, ExprKind, ExprKindShape, HookedShape,
    ListShape, MatchHook, MatchHookObject, NotShape, OrShape, OrStrategy, RepeatShape, ShapeObject,
    TableExtraPolicy, TableFieldSpec, TableShape, VennShapeSet, hook_ref_arc,
    shape_value_with_encoding,
};

use super::{
    and_shape_class_symbol, any_shape_class_symbol, class_shape_class_symbol,
    exact_expr_shape_class_symbol, expr_kind_shape_class_symbol, hooked_shape_class_symbol,
    list_shape_class_symbol, not_shape_class_symbol, or_shape_class_symbol,
    repeat_shape_class_symbol, table_shape_class_symbol, venn_shape_set_class_symbol,
};

impl ObjectEncode for VennShapeSet {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(constructor_encoding(
            venn_shape_set_class_symbol(),
            vec![encode_venn_members(self.members())?],
        ))
    }
}

impl sim_citizen::Citizen for VennShapeSet {
    fn citizen_symbol() -> Symbol {
        venn_shape_set_class_symbol()
    }

    fn citizen_version() -> u32 {
        1
    }

    fn citizen_arity() -> usize {
        1
    }

    fn citizen_fields() -> &'static [&'static str] {
        &["members"]
    }
}

pub(crate) fn build_shape_value(symbol: Symbol, shape: Arc<dyn Shape>, fields: Vec<Expr>) -> Value {
    shape_value_with_encoding(symbol.clone(), shape, constructor_encoding(symbol, fields))
}

pub(crate) fn constructor_encoding(class: Symbol, fields: Vec<Expr>) -> ObjectEncoding {
    ObjectEncoding::Constructor {
        class,
        args: constructor_args(fields),
    }
}

fn constructor_expr(class: Symbol, fields: Vec<Expr>) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(class)),
        args: constructor_args(fields),
    }
}

fn constructor_args(fields: Vec<Expr>) -> Vec<Expr> {
    let mut args = Vec::with_capacity(fields.len() + 1);
    args.push(Expr::Symbol(Symbol::new("v1")));
    args.extend(fields);
    args
}

pub(crate) fn int_expr(value: impl ToString) -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("citizen", "int"),
        canonical: value.to_string(),
    })
}

pub(crate) fn decode_symbol(cx: &mut Cx, value: Value, field: &'static str) -> Result<Symbol> {
    match value.object().as_expr(cx)? {
        Expr::Symbol(symbol) => Ok(symbol),
        Expr::String(text) => Ok(Symbol::new(text)),
        other => Err(field_error(
            field,
            format!("expected symbol or string, found {other:?}"),
        )),
    }
}

pub(crate) fn expr_kind_symbol(kind: &ExprKind) -> Symbol {
    Symbol::new(kind.name())
}

pub(crate) fn decode_expr_kind(cx: &mut Cx, value: Value) -> Result<ExprKind> {
    let symbol = decode_symbol(cx, value, "kind")?;
    match symbol.name.as_ref() {
        "nil" => Ok(ExprKind::Nil),
        "bool" => Ok(ExprKind::Bool),
        "number" => Ok(ExprKind::Number),
        "symbol" => Ok(ExprKind::Symbol),
        "string" => Ok(ExprKind::String),
        "bytes" => Ok(ExprKind::Bytes),
        "list" => Ok(ExprKind::List),
        "vector" => Ok(ExprKind::Vector),
        "map" => Ok(ExprKind::Map),
        "set" => Ok(ExprKind::Set),
        "call" => Ok(ExprKind::Call),
        "infix" => Ok(ExprKind::Infix),
        "prefix" => Ok(ExprKind::Prefix),
        "postfix" => Ok(ExprKind::Postfix),
        "block" => Ok(ExprKind::Block),
        "quote" => Ok(ExprKind::Quote),
        "annotated" => Ok(ExprKind::Annotated),
        "extension" => Ok(ExprKind::Extension),
        other => Err(field_error("kind", format!("unknown expr kind {other}"))),
    }
}

pub(crate) fn decode_shape_value(
    cx: &mut Cx,
    value: Value,
    field: &'static str,
) -> Result<Arc<dyn Shape>> {
    if let Some(shape) = value.object().downcast_ref::<ShapeObject>() {
        return Ok(shape.shape.clone());
    }
    if let Some(shape) = value.object().as_shape() {
        return clone_supported_shape(shape, field);
    }
    let expr = value.object().as_expr(cx)?;
    let constructed = construct_from_expr(cx, &expr, field)?;
    constructed
        .object()
        .downcast_ref::<ShapeObject>()
        .map(|shape| shape.shape.clone())
        .ok_or_else(|| field_error(field, "constructor did not produce a shape value"))
}

fn clone_supported_shape(shape: &dyn Shape, field: &'static str) -> Result<Arc<dyn Shape>> {
    if shape.as_any().is::<AnyShape>() {
        return Ok(Arc::new(AnyShape));
    }
    if let Some(exact) = shape.as_any().downcast_ref::<ExactExprShape>() {
        return Ok(Arc::new(ExactExprShape::new(exact.expected().clone())));
    }
    if let Some(kind) = shape.as_any().downcast_ref::<ExprKindShape>() {
        return Ok(Arc::new(ExprKindShape::new(kind.kind().clone())));
    }
    if let Some(class) = shape.as_any().downcast_ref::<ClassShape>() {
        return Ok(Arc::new(ClassShape::new(class.symbol().clone())));
    }
    if let Some(list) = shape.as_any().downcast_ref::<ListShape>() {
        let items = list
            .items()
            .iter()
            .map(|item| clone_supported_shape(item.as_ref(), field))
            .collect::<Result<Vec<_>>>()?;
        return Ok(match list.rest() {
            Some(rest) => Arc::new(ListShape::with_rest(
                items,
                clone_supported_shape(rest.as_ref(), field)?,
            )),
            None => Arc::new(ListShape::new(items)),
        });
    }
    Err(field_error(
        field,
        "shape value is not one of the citizen-supported pure descriptors",
    ))
}

fn construct_from_expr(cx: &mut Cx, expr: &Expr, field: &'static str) -> Result<Value> {
    let (class, args) = match expr {
        Expr::Call { operator, args } => match operator.as_ref() {
            Expr::Symbol(class) => (class.clone(), args.as_slice()),
            _ => return Err(field_error(field, "constructor operator must be a symbol")),
        },
        Expr::List(items) => match items.split_first() {
            Some((Expr::Symbol(class), args)) => (class.clone(), args),
            _ => {
                return Err(field_error(
                    field,
                    "constructor list must start with a symbol",
                ));
            }
        },
        _ => return Err(field_error(field, "expected constructor expression")),
    };
    let values = args
        .iter()
        .map(|arg| value_from_expr(cx, arg))
        .collect::<Result<Vec<_>>>()?;
    cx.read_construct(&class, values)
}

pub(crate) fn encode_shape_expr(shape: &dyn Shape) -> Result<Expr> {
    if shape.as_any().is::<AnyShape>() {
        return Ok(constructor_expr(any_shape_class_symbol(), Vec::new()));
    }
    if let Some(exact) = shape.as_any().downcast_ref::<ExactExprShape>() {
        return Ok(constructor_expr(
            exact_expr_shape_class_symbol(),
            vec![exact.expected().clone()],
        ));
    }
    if let Some(kind) = shape.as_any().downcast_ref::<ExprKindShape>() {
        return Ok(constructor_expr(
            expr_kind_shape_class_symbol(),
            vec![Expr::Symbol(expr_kind_symbol(kind.kind()))],
        ));
    }
    if let Some(class) = shape.as_any().downcast_ref::<ClassShape>() {
        return Ok(constructor_expr(
            class_shape_class_symbol(),
            vec![Expr::Symbol(class.symbol().clone())],
        ));
    }
    if let Some(list) = shape.as_any().downcast_ref::<ListShape>() {
        return Ok(constructor_expr(
            list_shape_class_symbol(),
            vec![
                encode_shape_list(list.items())?,
                list.rest()
                    .map(|shape| encode_shape_expr(shape.as_ref()))
                    .transpose()?
                    .unwrap_or(Expr::Nil),
            ],
        ));
    }
    if let Some(table) = shape.as_any().downcast_ref::<TableShape>() {
        return Ok(constructor_expr(
            table_shape_class_symbol(),
            vec![
                encode_table_fields(table.fields())?,
                encode_extra(table.extra())?,
            ],
        ));
    }
    if let Some(or) = shape.as_any().downcast_ref::<OrShape>() {
        return Ok(constructor_expr(
            or_shape_class_symbol(),
            vec![
                encode_shape_list(or.choices())?,
                Expr::Symbol(or_strategy_symbol(or.strategy())),
            ],
        ));
    }
    if let Some(and) = shape.as_any().downcast_ref::<AndShape>() {
        return Ok(constructor_expr(
            and_shape_class_symbol(),
            vec![encode_shape_list(and.parts())?],
        ));
    }
    if let Some(not) = shape.as_any().downcast_ref::<NotShape>() {
        return Ok(constructor_expr(
            not_shape_class_symbol(),
            vec![encode_shape_expr(not.inner().as_ref())?],
        ));
    }
    if let Some(repeat) = shape.as_any().downcast_ref::<RepeatShape>() {
        return Ok(constructor_expr(
            repeat_shape_class_symbol(),
            vec![
                encode_shape_expr(repeat.body().as_ref())?,
                int_expr(repeat.min()),
                repeat.max().map(int_expr).unwrap_or(Expr::Nil),
            ],
        ));
    }
    if let Some(hooked) = shape.as_any().downcast_ref::<HookedShape>() {
        return Ok(constructor_expr(
            hooked_shape_class_symbol(),
            vec![
                encode_shape_expr(hooked.inner().as_ref())?,
                encode_hooks(hooked.hooks())?,
            ],
        ));
    }
    Err(Error::Eval(
        "shape is not a citizen-supported pure descriptor".to_owned(),
    ))
}

pub(crate) fn encode_shape_list(shapes: &[Arc<dyn Shape>]) -> Result<Expr> {
    Ok(Expr::List(
        shapes
            .iter()
            .map(|shape| encode_shape_expr(shape.as_ref()))
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn decode_shape_list(
    cx: &mut Cx,
    value: Value,
    field: &'static str,
) -> Result<Vec<Arc<dyn Shape>>> {
    let list = value
        .object()
        .as_list()
        .ok_or_else(|| field_error(field, "expected list of shape constructor descriptors"))?;
    force_list_to_vec(cx, list, field)?
        .into_iter()
        .map(|value| decode_shape_value(cx, value, field))
        .collect()
}

pub(crate) fn encode_table_fields(fields: &[TableFieldSpec]) -> Result<Expr> {
    Ok(Expr::List(
        fields
            .iter()
            .map(|field| {
                Ok(Expr::List(vec![
                    Expr::Symbol(field.key.clone()),
                    Expr::Bool(field.required),
                    encode_shape_expr(field.shape.as_ref())?,
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn decode_table_fields(cx: &mut Cx, value: Value) -> Result<Vec<TableFieldSpec>> {
    let list = value
        .object()
        .as_list()
        .ok_or_else(|| field_error("fields", "expected table field list"))?;
    force_list_to_vec(cx, list, "fields")?
        .into_iter()
        .map(|entry| {
            let parts = entry
                .object()
                .as_list()
                .ok_or_else(|| field_error("fields", "table field must be a list"))?;
            let parts = force_list_to_vec(cx, parts, "fields")?;
            let [key, required, shape] = parts.as_slice() else {
                return Err(field_error(
                    "fields",
                    "table field must have key, required, shape",
                ));
            };
            Ok(TableFieldSpec {
                key: decode_symbol(cx, key.clone(), "field-key")?,
                required: bool::decode_field_value(cx, required.clone(), "required")?,
                shape: decode_shape_value(cx, shape.clone(), "field-shape")?,
            })
        })
        .collect()
}

pub(crate) fn encode_extra(extra: &TableExtraPolicy) -> Result<Expr> {
    match extra {
        TableExtraPolicy::Allow => Ok(Expr::Symbol(Symbol::new("allow"))),
        TableExtraPolicy::Reject => Ok(Expr::Symbol(Symbol::new("reject"))),
        TableExtraPolicy::Shape(shape) => Ok(Expr::List(vec![
            Expr::Symbol(Symbol::new("shape")),
            encode_shape_expr(shape.as_ref())?,
        ])),
    }
}

pub(crate) fn decode_extra(cx: &mut Cx, value: Value) -> Result<TableExtraPolicy> {
    match value.object().as_expr(cx)? {
        Expr::Symbol(symbol) if symbol.name.as_ref() == "allow" => Ok(TableExtraPolicy::Allow),
        Expr::Symbol(symbol) if symbol.name.as_ref() == "reject" => Ok(TableExtraPolicy::Reject),
        Expr::List(items) => match items.as_slice() {
            [Expr::Symbol(head), shape] if head.name.as_ref() == "shape" => {
                let value = value_from_expr(cx, shape)?;
                Ok(TableExtraPolicy::Shape(decode_shape_value(
                    cx, value, "extra",
                )?))
            }
            _ => Err(field_error("extra", "expected (shape descriptor)")),
        },
        other => Err(field_error(
            "extra",
            format!("expected allow, reject, or shape policy, found {other:?}"),
        )),
    }
}

pub(crate) fn or_strategy_symbol(strategy: OrStrategy) -> Symbol {
    match strategy {
        OrStrategy::FirstMatch => Symbol::new("first-match"),
        OrStrategy::BestScore => Symbol::new("best-score"),
    }
}

pub(crate) fn decode_or_strategy(cx: &mut Cx, value: Value) -> Result<OrStrategy> {
    let symbol = decode_symbol(cx, value, "strategy")?;
    match symbol.name.as_ref() {
        "first-match" => Ok(OrStrategy::FirstMatch),
        "best-score" => Ok(OrStrategy::BestScore),
        other => Err(field_error("strategy", format!("unknown strategy {other}"))),
    }
}

pub(crate) fn encode_hooks(hooks: &[Arc<dyn MatchHook>]) -> Result<Expr> {
    Ok(Expr::List(
        hooks
            .iter()
            .map(|hook| match hook.object_encoding() {
                Some(ObjectEncoding::Constructor { class, args }) => Ok(Expr::Call {
                    operator: Box::new(Expr::Symbol(class)),
                    args,
                }),
                _ => Err(Error::Eval(format!(
                    "shape hook {} is not a pure descriptor citizen",
                    hook.symbol()
                ))),
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn decode_hooks(cx: &mut Cx, value: Value) -> Result<Vec<Arc<dyn MatchHook>>> {
    let list = value
        .object()
        .as_list()
        .ok_or_else(|| field_error("hooks", "expected hook descriptor list"))?;
    force_list_to_vec(cx, list, "hooks")?
        .into_iter()
        .map(|value| {
            if let Some(hook) = value.object().downcast_ref::<MatchHookObject>() {
                return Ok(hook.hook());
            }
            let expr = value.object().as_expr(cx)?;
            let constructed = construct_from_expr(cx, &expr, "hooks")?;
            hook_ref_arc(&constructed)
        })
        .collect()
}

pub(crate) fn encode_venn_members(members: &[(Symbol, Arc<dyn Shape>)]) -> Result<Expr> {
    Ok(Expr::List(
        members
            .iter()
            .map(|(name, shape)| {
                Ok(Expr::List(vec![
                    Expr::Symbol(name.clone()),
                    encode_shape_expr(shape.as_ref())?,
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn decode_venn_members(
    cx: &mut Cx,
    value: Value,
) -> Result<Vec<(Symbol, Arc<dyn Shape>)>> {
    let list = value
        .object()
        .as_list()
        .ok_or_else(|| field_error("members", "expected Venn member list"))?;
    force_list_to_vec(cx, list, "members")?
        .into_iter()
        .map(|entry| {
            let parts = entry
                .object()
                .as_list()
                .ok_or_else(|| field_error("members", "Venn member must be a list"))?;
            let parts = force_list_to_vec(cx, parts, "members")?;
            let [name, shape] = parts.as_slice() else {
                return Err(field_error(
                    "members",
                    "Venn member must have name and shape",
                ));
            };
            Ok((
                decode_symbol(cx, name.clone(), "member-name")?,
                decode_shape_value(cx, shape.clone(), "member-shape")?,
            ))
        })
        .collect()
}
