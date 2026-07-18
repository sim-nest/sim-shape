//! Alternation combinator over multiple candidate shapes.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, Value, shape_is_subshape_of};

use crate::base::{Bindings, MatchScore, Shape, ShapeDoc, ShapeMatch};

/// A shape that matches if any one of its choice shapes matches.
///
/// Choices are tried in order and the first accepting match wins; if none
/// accept, the rejection collects every choice's diagnostics. Total if any
/// choice is total.
pub struct OneOfShape {
    choices: Vec<Arc<dyn Shape>>,
}

impl OneOfShape {
    /// Build an alternation over the given choice shapes.
    pub fn new(choices: Vec<Arc<dyn Shape>>) -> Self {
        Self { choices }
    }

    /// The choice shapes tried in order.
    pub fn choices(&self) -> &[Arc<dyn Shape>] {
        &self.choices
    }
}

impl Shape for OneOfShape {
    fn is_effectful(&self) -> bool {
        self.choices.iter().any(|choice| choice.is_effectful())
    }

    fn is_total(&self) -> bool {
        self.choices.iter().any(|choice| choice.is_total())
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        for choice in &self.choices {
            if !shape_is_subshape_of(cx, choice.as_ref(), parent)? {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let mut diagnostics = Vec::new();
        for choice in &self.choices {
            let matched = choice.check_value(cx, value.clone())?;
            if matched.accepted {
                return Ok(matched);
            }
            diagnostics.extend(matched.diagnostics);
        }
        Ok(ShapeMatch {
            accepted: false,
            captures: Bindings::new(),
            score: MatchScore::reject(),
            diagnostics,
        })
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let mut diagnostics = Vec::new();
        for choice in &self.choices {
            let matched = choice.check_expr(cx, expr)?;
            if matched.accepted {
                return Ok(matched);
            }
            diagnostics.extend(matched.diagnostics);
        }
        Ok(ShapeMatch {
            accepted: false,
            captures: Bindings::new(),
            score: MatchScore::reject(),
            diagnostics,
        })
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("one-of shape");
        for choice in &self.choices {
            doc = doc.with_detail(choice.describe(cx)?.name);
        }
        Ok(doc)
    }
}
