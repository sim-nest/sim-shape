//! Constructors for shape citizens: build each concrete shape as a value from
//! its decoded constructor fields and expose the matching shape-value helpers.

use std::sync::Arc;

use sim_citizen::{CitizenField, arity_error, decode_version};
use sim_kernel::{Cx, Expr, Result, Shape, Symbol, Value};

use crate::{
    AcceptOnNoDiagnosticsHook, AndShape, AnyShape, ClassShape, DiscardOnDiagnosticPrefixHook,
    ExactExprShape, ExprKind, ExprKindShape, HookedShape, ListShape, NotShape, OrShape, OrStrategy,
    RepeatShape, ScoreFloorHook, ShapeDefRef, ShapeDefs, TableExtraPolicy, TableFieldSpec,
    TableShape, TraceMarkHook, VennShapeSet, hook_value,
};

use super::{
    and_shape_class_symbol, any_shape_class_symbol, build_shape_value, class_shape_class_symbol,
    decode_expr_kind, decode_extra, decode_hooks, decode_shape_defs, decode_shape_list,
    decode_shape_value, decode_symbol, decode_table_fields, decode_venn_members,
    exact_expr_shape_class_symbol, expr_kind_shape_class_symbol, expr_kind_symbol,
    hooked_shape_class_symbol, list_shape_class_symbol, not_shape_class_symbol,
    or_shape_class_symbol, or_strategy_symbol, repeat_shape_class_symbol,
    shape_def_ref_class_symbol, shape_defs_class_symbol, table_shape_class_symbol,
    venn_shape_set_class_symbol,
};

fn fields<'a>(
    cx: &mut Cx,
    class: Symbol,
    args: &'a [Value],
    arity_without_version: usize,
) -> Result<&'a [Value]> {
    let expected = arity_without_version + 1;
    if args.len() != expected {
        return Err(arity_error(class, expected, args.len()));
    }
    decode_version(cx, args[0].clone(), 1, class)?;
    Ok(&args[1..])
}

pub(super) fn any_shape_value() -> Value {
    build_shape_value(any_shape_class_symbol(), Arc::new(AnyShape), Vec::new())
}

pub(super) fn construct_any_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    fields(cx, any_shape_class_symbol(), &args, 0)?;
    Ok(any_shape_value())
}

pub(super) fn exact_expr_shape_value(expected: Expr) -> Value {
    build_shape_value(
        exact_expr_shape_class_symbol(),
        Arc::new(ExactExprShape::new(expected.clone())),
        vec![expected],
    )
}

pub(super) fn construct_exact_expr_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, exact_expr_shape_class_symbol(), &args, 1)?;
    Ok(exact_expr_shape_value(fields[0].object().as_expr(cx)?))
}

pub(super) fn expr_kind_shape_value(kind: ExprKind) -> Value {
    build_shape_value(
        expr_kind_shape_class_symbol(),
        Arc::new(ExprKindShape::new(kind.clone())),
        vec![Expr::Symbol(expr_kind_symbol(&kind))],
    )
}

pub(super) fn construct_expr_kind_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, expr_kind_shape_class_symbol(), &args, 1)?;
    Ok(expr_kind_shape_value(decode_expr_kind(
        cx,
        fields[0].clone(),
    )?))
}

pub(super) fn class_shape_value(symbol: Symbol) -> Value {
    build_shape_value(
        class_shape_class_symbol(),
        Arc::new(ClassShape::new(symbol.clone())),
        vec![Expr::Symbol(symbol)],
    )
}

pub(super) fn construct_class_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, class_shape_class_symbol(), &args, 1)?;
    Ok(class_shape_value(decode_symbol(
        cx,
        fields[0].clone(),
        "class",
    )?))
}

pub(super) fn list_shape_value(
    items: Vec<Arc<dyn Shape>>,
    rest: Option<Arc<dyn Shape>>,
) -> Result<Value> {
    let shape: Arc<dyn Shape> = match rest.clone() {
        Some(rest) => Arc::new(ListShape::with_rest(items.clone(), rest)),
        None => Arc::new(ListShape::new(items.clone())),
    };
    Ok(build_shape_value(
        list_shape_class_symbol(),
        shape,
        vec![
            super::encode_shape_list(&items)?,
            rest.as_ref()
                .map(|shape| super::encode_shape_expr(shape.as_ref()))
                .transpose()?
                .unwrap_or(Expr::Nil),
        ],
    ))
}

pub(super) fn construct_list_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, list_shape_class_symbol(), &args, 2)?;
    let items = decode_shape_list(cx, fields[0].clone(), "items")?;
    let rest = match fields[1].object().as_expr(cx)? {
        Expr::Nil => None,
        _ => Some(decode_shape_value(cx, fields[1].clone(), "rest")?),
    };
    list_shape_value(items, rest)
}

pub(super) fn table_shape_value(
    fields: Vec<TableFieldSpec>,
    extra: TableExtraPolicy,
) -> Result<Value> {
    Ok(build_shape_value(
        table_shape_class_symbol(),
        Arc::new(TableShape::new(fields.clone(), extra.clone())),
        vec![
            super::encode_table_fields(&fields)?,
            super::encode_extra(&extra)?,
        ],
    ))
}

pub(super) fn construct_table_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, table_shape_class_symbol(), &args, 2)?;
    table_shape_value(
        decode_table_fields(cx, fields[0].clone())?,
        decode_extra(cx, fields[1].clone())?,
    )
}

pub(super) fn or_shape_value(choices: Vec<Arc<dyn Shape>>, strategy: OrStrategy) -> Result<Value> {
    Ok(build_shape_value(
        or_shape_class_symbol(),
        Arc::new(OrShape::with_strategy(choices.clone(), strategy)),
        vec![
            super::encode_shape_list(&choices)?,
            Expr::Symbol(or_strategy_symbol(strategy)),
        ],
    ))
}

pub(super) fn construct_or_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, or_shape_class_symbol(), &args, 2)?;
    or_shape_value(
        decode_shape_list(cx, fields[0].clone(), "choices")?,
        super::codec::decode_or_strategy(cx, fields[1].clone())?,
    )
}

pub(super) fn and_shape_value(parts: Vec<Arc<dyn Shape>>) -> Result<Value> {
    Ok(build_shape_value(
        and_shape_class_symbol(),
        Arc::new(AndShape::new(parts.clone())),
        vec![super::encode_shape_list(&parts)?],
    ))
}

pub(super) fn construct_and_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, and_shape_class_symbol(), &args, 1)?;
    and_shape_value(decode_shape_list(cx, fields[0].clone(), "parts")?)
}

pub(super) fn not_shape_value(inner: Arc<dyn Shape>) -> Result<Value> {
    Ok(build_shape_value(
        not_shape_class_symbol(),
        Arc::new(NotShape::new(inner.clone())),
        vec![super::encode_shape_expr(inner.as_ref())?],
    ))
}

pub(super) fn construct_not_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, not_shape_class_symbol(), &args, 1)?;
    not_shape_value(decode_shape_value(cx, fields[0].clone(), "inner")?)
}

pub(super) fn repeat_shape_value(
    body: Arc<dyn Shape>,
    min: usize,
    max: Option<usize>,
) -> Result<Value> {
    Ok(build_shape_value(
        repeat_shape_class_symbol(),
        Arc::new(RepeatShape::with_bounds(body.clone(), min, max)),
        vec![
            super::encode_shape_expr(body.as_ref())?,
            min.encode_field(),
            max.encode_field(),
        ],
    ))
}

pub(super) fn construct_repeat_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, repeat_shape_class_symbol(), &args, 3)?;
    repeat_shape_value(
        decode_shape_value(cx, fields[0].clone(), "body")?,
        usize::decode_field_value(cx, fields[1].clone(), "min")?,
        Option::<usize>::decode_field_value(cx, fields[2].clone(), "max")?,
    )
}

pub(super) fn shape_defs_value(
    root: Arc<dyn Shape>,
    defs: Vec<(Symbol, Arc<dyn Shape>)>,
) -> Result<Value> {
    Ok(build_shape_value(
        shape_defs_class_symbol(),
        Arc::new(ShapeDefs::new(root.clone(), defs.clone())),
        vec![
            super::encode_shape_expr(root.as_ref())?,
            super::encode_shape_defs(&defs)?,
        ],
    ))
}

pub(super) fn construct_shape_defs(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, shape_defs_class_symbol(), &args, 2)?;
    shape_defs_value(
        decode_shape_value(cx, fields[0].clone(), "root")?,
        decode_shape_defs(cx, fields[1].clone())?,
    )
}

pub(super) fn shape_def_ref_value(name: Symbol) -> Value {
    build_shape_value(
        shape_def_ref_class_symbol(),
        Arc::new(ShapeDefRef::new(name.clone())),
        vec![Expr::Symbol(name)],
    )
}

pub(super) fn construct_shape_def_ref(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, shape_def_ref_class_symbol(), &args, 1)?;
    Ok(shape_def_ref_value(decode_symbol(
        cx,
        fields[0].clone(),
        "name",
    )?))
}

pub(super) fn hooked_shape_value(
    inner: Arc<dyn Shape>,
    hooks: Vec<Arc<dyn crate::MatchHook>>,
) -> Result<Value> {
    Ok(build_shape_value(
        hooked_shape_class_symbol(),
        Arc::new(HookedShape::new(inner.clone(), hooks.clone())),
        vec![
            super::encode_shape_expr(inner.as_ref())?,
            super::encode_hooks(&hooks)?,
        ],
    ))
}

pub(super) fn construct_hooked_shape(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, hooked_shape_class_symbol(), &args, 2)?;
    hooked_shape_value(
        decode_shape_value(cx, fields[0].clone(), "inner")?,
        decode_hooks(cx, fields[1].clone())?,
    )
}

pub(super) fn construct_trace_mark_hook(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    fields(cx, crate::trace_mark_hook_class_symbol(), &args, 0)?;
    Ok(hook_value(Arc::new(TraceMarkHook)))
}

pub(super) fn construct_score_floor_hook(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, crate::score_floor_hook_class_symbol(), &args, 1)?;
    Ok(hook_value(Arc::new(ScoreFloorHook::new(
        i32::decode_field_value(cx, fields[0].clone(), "floor")?,
    ))))
}

pub(super) fn construct_accept_on_no_diagnostics_hook(
    cx: &mut Cx,
    args: Vec<Value>,
) -> Result<Value> {
    fields(
        cx,
        crate::accept_on_no_diagnostics_hook_class_symbol(),
        &args,
        0,
    )?;
    Ok(hook_value(Arc::new(AcceptOnNoDiagnosticsHook)))
}

pub(super) fn construct_discard_on_diagnostic_prefix_hook(
    cx: &mut Cx,
    args: Vec<Value>,
) -> Result<Value> {
    let fields = fields(
        cx,
        crate::discard_on_diagnostic_prefix_hook_class_symbol(),
        &args,
        1,
    )?;
    Ok(hook_value(Arc::new(DiscardOnDiagnosticPrefixHook::new(
        String::decode_field_value(cx, fields[0].clone(), "prefix")?,
    ))))
}

pub(super) fn construct_venn_shape_set(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let fields = fields(cx, venn_shape_set_class_symbol(), &args, 1)?;
    let members = decode_venn_members(cx, fields[0].clone())?;
    cx.factory().opaque(Arc::new(VennShapeSet::new(members)))
}
