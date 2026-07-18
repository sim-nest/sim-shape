//! Parser-backed shape combinator.

use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, Value};

use crate::base::{Shape, ShapeDoc, ShapeMatch};

/// A parser that turns shape source text into an [`Expr`] for [`PrattShape`].
///
/// Implementations plug a concrete grammar (the codec layer, not the kernel)
/// into shape matching; `parse_expr` should report a parse failure as
/// `Error::Eval` so the shape rejects rather than aborts.
pub trait ShapeExprParser: Send + Sync {
    /// A short human-readable name for this parser, shown in shape descriptions.
    fn label(&self) -> &str;

    /// Whether parsing may run effects; `false` by default.
    fn is_effectful(&self) -> bool {
        false
    }

    /// Parse source text into an expression to match against the inner shape.
    fn parse_expr(&self, source: &str) -> Result<Expr>;
}

/// A shape that parses a string expression with a [`ShapeExprParser`] before
/// matching.
///
/// Accepts only `String` expressions: the text is parsed and the resulting
/// expression is checked against the inner shape. Non-string inputs and parse
/// failures reject.
pub struct PrattShape {
    parser: Arc<dyn ShapeExprParser>,
    inner: Arc<dyn Shape>,
}

impl PrattShape {
    /// Build a shape that parses string input with `parser`, then checks `inner`.
    pub fn new(parser: Arc<dyn ShapeExprParser>, inner: Arc<dyn Shape>) -> Self {
        Self { parser, inner }
    }

    /// The parser applied to string input.
    pub fn parser(&self) -> &Arc<dyn ShapeExprParser> {
        &self.parser
    }

    /// The inner shape checked against the parsed expression.
    pub fn inner(&self) -> &Arc<dyn Shape> {
        &self.inner
    }
}

impl Shape for PrattShape {
    fn is_effectful(&self) -> bool {
        self.parser.is_effectful() || self.inner.is_effectful()
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let source = match expr {
            Expr::String(text) => text.as_str(),
            _ => {
                return Ok(ShapeMatch::reject(
                    "expected string expression for Pratt parse",
                ));
            }
        };
        let parsed = match self.parser.parse_expr(source) {
            Ok(parsed) => parsed,
            Err(Error::Eval(message)) => return Ok(ShapeMatch::reject(message)),
            Err(other) => return Err(other),
        };
        self.inner.check_expr(cx, &parsed)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let inner = self.inner.describe(cx)?;
        Ok(ShapeDoc::new("pratt shape")
            .with_detail(self.parser.label().to_owned())
            .with_detail(inner.name))
    }
}
