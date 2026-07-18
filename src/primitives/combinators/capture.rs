//! Capture combinator for binding matched shape inputs.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, ShapeRef, Symbol, Value, shape_is_subshape_of};

use crate::base::{Shape, ShapeDoc, ShapeMatch};
use crate::diagnostics::{binding_failure_diagnostic, expr_actual_label};

/// A shape that wraps an inner shape and binds the match under a name.
///
/// When the inner shape accepts, the matched expression (and value, where the
/// inner shape is not total) is recorded in the match captures under `name`,
/// feeding shape-driven binding.
pub struct CaptureShape {
    name: Symbol,
    inner: Arc<dyn Shape>,
}

impl CaptureShape {
    /// Build a capture that binds the inner shape's match under `name`.
    pub fn new(name: Symbol, inner: Arc<dyn Shape>) -> Self {
        Self { name, inner }
    }

    /// The capture name bound on a successful match.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// The inner shape whose match is captured.
    pub fn inner(&self) -> &Arc<dyn Shape> {
        &self.inner
    }
}

impl Shape for CaptureShape {
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        Ok(vec![crate::functions::shape_value(
            Symbol::qualified("shape-capture-parent", self.name.to_string()),
            self.inner.clone(),
        )])
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        shape_is_subshape_of(cx, self.inner.as_ref(), parent).map(Some)
    }

    fn is_total(&self) -> bool {
        self.inner.is_total()
    }

    fn is_effectful(&self) -> bool {
        self.inner.is_effectful()
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let mut matched = self.inner.check_value(cx, value.clone())?;
        if matched.accepted {
            matched.captures.bind_value(self.name.clone(), value);
            if !self.inner.is_total()
                && let Ok(expr) = matched
                    .captures
                    .values()
                    .last()
                    .expect("just bound value capture")
                    .1
                    .object()
                    .as_expr(cx)
            {
                matched.captures.bind_expr(self.name.clone(), expr);
            }
        } else {
            matched.diagnostics.insert(
                0,
                binding_failure_diagnostic(&self.name, "captured value", "rejected value"),
            );
        }
        Ok(matched)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let mut matched = self.inner.check_expr(cx, expr)?;
        if matched.accepted {
            matched.captures.bind_expr(self.name.clone(), expr.clone());
        } else {
            matched.diagnostics.insert(
                0,
                binding_failure_diagnostic(
                    &self.name,
                    "captured expression",
                    expr_actual_label(expr),
                ),
            );
        }
        Ok(matched)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let inner = self.inner.describe(cx)?;
        Ok(ShapeDoc::new(format!("capture {}", self.name)).with_detail(inner.name))
    }
}
