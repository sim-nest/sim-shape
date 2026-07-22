//! Effectful wrapper combinator.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, ShapeId, Value, shape_is_subshape_of};

use crate::base::{Shape, ShapeDoc, ShapeMatch};

/// A shape that marks its inner shape's matching as effectful.
///
/// Delegates checks to the inner shape but reports `is_effectful() == true` and
/// carries no shape id, signalling that parse-time validation through this
/// shape needs a trusted parser position.
pub struct EffectfulShape {
    inner: Arc<dyn Shape>,
}

impl EffectfulShape {
    /// Wrap an inner shape, marking its matching as effectful.
    pub fn new(inner: Arc<dyn Shape>) -> Self {
        Self { inner }
    }

    /// The wrapped inner shape.
    pub fn inner(&self) -> &Arc<dyn Shape> {
        &self.inner
    }
}

impl Shape for EffectfulShape {
    fn id(&self) -> Option<ShapeId> {
        None
    }

    fn is_effectful(&self) -> bool {
        true
    }

    fn is_total(&self) -> bool {
        self.inner.is_total()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(Some(false));
        };
        shape_is_subshape_of(cx, self.inner.as_ref(), parent.inner.as_ref()).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        self.inner.check_value(cx, value)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        self.inner.check_expr(cx, expr)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let inner = self.inner.describe(cx)?;
        Ok(
            ShapeDoc::new(format!("effectful {}", inner.name)).with_detail(
                "effectful parse-time validation requires a trusted parser position".to_owned(),
            ),
        )
    }
}
