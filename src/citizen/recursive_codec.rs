//! Codec helpers for recursive shape definition lists.

use std::sync::Arc;

use sim_citizen::field_error;
use sim_kernel::{Cx, Expr, Result, Shape, Symbol, Value, force_list_to_vec};

use super::codec::{decode_shape_value, decode_symbol, encode_shape_expr};

pub(crate) fn encode_shape_defs(defs: &[(Symbol, Arc<dyn Shape>)]) -> Result<Expr> {
    Ok(Expr::List(
        defs.iter()
            .map(|(name, shape)| {
                Ok(Expr::List(vec![
                    Expr::Symbol(name.clone()),
                    encode_shape_expr(shape.as_ref())?,
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn decode_shape_defs(
    cx: &mut Cx,
    value: Value,
) -> Result<Vec<(Symbol, Arc<dyn Shape>)>> {
    let list = value
        .object()
        .as_list()
        .ok_or_else(|| field_error("defs", "expected shape definition list"))?;
    force_list_to_vec(cx, list, "defs")?
        .into_iter()
        .map(|entry| {
            let parts = entry
                .object()
                .as_list()
                .ok_or_else(|| field_error("defs", "shape definition must be a list"))?;
            let parts = force_list_to_vec(cx, parts, "defs")?;
            let [name, shape] = parts.as_slice() else {
                return Err(field_error(
                    "defs",
                    "shape definition must have name and shape",
                ));
            };
            Ok((
                decode_symbol(cx, name.clone(), "def-name")?,
                decode_shape_value(cx, shape.clone(), "def-shape")?,
            ))
        })
        .collect()
}
