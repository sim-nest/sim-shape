//! Primitive combinators: `ListShape`, `CaptureShape`, `OneOfShape`, the
//! `ShapeExprParser`-driven `PrattShape`, and `EffectfulShape`.

use std::{cmp::Ordering, sync::Arc};

use sim_kernel::{Cx, Error, Expr, Result, ShapeId, ShapeRef, Value, shape_is_subshape_of};

use crate::base::{Bindings, MatchScore, Shape, ShapeDoc, ShapeMatch};
use crate::diagnostics::{
    binding_failure_diagnostic, expected_shape_diagnostic, expr_actual_label,
};

/// A shape that matches a list positionally, with an optional rest shape.
///
/// The leading `items` shapes must match the list elements in order. Without a
/// rest shape the list length must match exactly (a tuple); with one, every
/// trailing element must match the rest shape (a variadic list).
///
/// # Examples
///
/// ```rust
/// # use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// use sim_shape::{ExprKind, ExprKindShape, ListShape, Shape};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = ListShape::new(vec![Arc::new(ExprKindShape::new(ExprKind::String))]);
/// let expr = Expr::List(vec![Expr::String("hi".to_owned())]);
/// assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);
/// // an empty list rejects: the single positional item shape is unmatched.
/// assert!(!shape.check_expr(&mut cx, &Expr::List(vec![])).unwrap().accepted);
/// ```
pub struct ListShape {
    items: Vec<Arc<dyn Shape>>,
    rest: Option<Arc<dyn Shape>>,
}

impl ListShape {
    /// Build a fixed-length list shape over the given positional item shapes.
    pub fn new(items: Vec<Arc<dyn Shape>>) -> Self {
        Self { items, rest: None }
    }

    /// Build a fixed-length tuple shape; an alias for [`ListShape::new`].
    pub fn tuple(items: Vec<Arc<dyn Shape>>) -> Self {
        Self::new(items)
    }

    /// Build a list shape with leading items and a rest shape for the tail.
    pub fn with_rest(items: Vec<Arc<dyn Shape>>, rest: Arc<dyn Shape>) -> Self {
        Self {
            items,
            rest: Some(rest),
        }
    }

    /// Build a variadic list shape; an alias for [`ListShape::with_rest`].
    pub fn variadic(prefix: Vec<Arc<dyn Shape>>, rest: Arc<dyn Shape>) -> Self {
        Self::with_rest(prefix, rest)
    }

    /// The positional item shapes.
    pub fn items(&self) -> &[Arc<dyn Shape>] {
        &self.items
    }

    /// The rest shape applied to trailing elements, if this list is variadic.
    pub fn rest(&self) -> Option<&Arc<dyn Shape>> {
        self.rest.as_ref()
    }

    fn check_list_value(&self, cx: &mut Cx, head: Value) -> Result<ShapeMatch> {
        let min_len = self.items.len();
        {
            let Some(list) = head.object().as_list() else {
                return Err(Error::Eval("expected a list value".to_owned()));
            };
            let len_cmp = list.len_cmp(cx, min_len)?;
            if len_cmp == Ordering::Less {
                return Ok(ShapeMatch::reject_with_diagnostic(
                    expected_shape_diagnostic(
                        format!("at least {min_len} list items"),
                        "fewer list items",
                    ),
                ));
            }
            if self.rest.is_none() && len_cmp != Ordering::Equal {
                return Ok(ShapeMatch::reject_with_diagnostic(
                    expected_shape_diagnostic(format!("{min_len} list items"), "more list items"),
                ));
            }
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(20));
        // Walk the cdr chain from the head as runtime values, so the rest walk
        // starts from the correct node even when there are no leading items.
        let mut current = Some(head);

        for shape in &self.items {
            let Some(node_value) = current.clone() else {
                return Ok(ShapeMatch::reject_with_diagnostic(
                    expected_shape_diagnostic(
                        format!("at least {min_len} list items"),
                        "fewer list items",
                    ),
                ));
            };
            let Some(node) = node_value.object().as_list() else {
                return Err(Error::Eval("list cdr did not yield a list".to_owned()));
            };
            let Some(item) = node.car(cx)? else {
                return Ok(ShapeMatch::reject_with_diagnostic(
                    expected_shape_diagnostic(
                        format!("at least {min_len} list items"),
                        "fewer list items",
                    ),
                ));
            };
            let next = node.cdr(cx)?;
            let matched = shape.check_value(cx, item)?;
            if !matched.accepted {
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
            current = next;
        }

        let Some(rest) = &self.rest else {
            return Ok(out);
        };

        // The expr path always walks the tail to collect rest captures; the
        // value path must do the same, so an equivalent list value captures
        // identically to its expression form. The old `is_total()` shortcut
        // returned here and dropped those bindings. We instead walk, but stop
        // once a total rest accepts an element while binding nothing: further
        // total, non-binding matches add no captures, so this keeps an
        // unbounded (lazy or endless) tail terminating without losing fidelity.
        while let Some(node_value) = current.clone() {
            let Some(node) = node_value.object().as_list() else {
                break;
            };
            if node.is_empty(cx)? {
                break;
            }
            let Some(item) = node.car(cx)? else {
                break;
            };
            let next = node.cdr(cx)?;
            let matched = rest.check_value(cx, item)?;
            if !matched.accepted {
                return Ok(matched);
            }
            let bound_nothing =
                matched.captures.values().is_empty() && matched.captures.exprs().is_empty();
            out.captures.extend(matched.captures);
            out.score += matched.score;
            current = next;
            if rest.is_total() && bound_nothing {
                break;
            }
        }

        Ok(out)
    }
}

impl Shape for ListShape {
    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        // Conservative structural covariance: prove the subshape relation only
        // when both lists share their fixed arity and rest presence and every
        // corresponding item (and the rest) is itself a proven subshape. Any
        // uncertainty stays `None` so the engine never asserts a false
        // relation; this is what lets dispatch prefer a structurally more
        // specific list overload over a general one.
        if self.items.len() != parent.items.len() {
            return Ok(None);
        }
        match (&self.rest, &parent.rest) {
            (None, None) => {}
            (Some(child_rest), Some(parent_rest)) => {
                if !shape_is_subshape_of(cx, child_rest.as_ref(), parent_rest.as_ref())? {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        for (child_item, parent_item) in self.items.iter().zip(parent.items.iter()) {
            if !shape_is_subshape_of(cx, child_item.as_ref(), parent_item.as_ref())? {
                return Ok(None);
            }
        }
        Ok(Some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if value.object().as_list().is_none() {
            let expr = value.object().as_expr(cx)?;
            return self.check_expr(cx, &expr);
        }
        self.check_list_value(cx, value)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let Expr::List(items) = expr else {
            return Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic("list expression", expr_actual_label(expr)),
            ));
        };

        if items.len() < self.items.len() {
            return Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic(
                    format!("at least {} list items", self.items.len()),
                    format!("{} list items", items.len()),
                ),
            ));
        }

        if self.rest.is_none() && items.len() != self.items.len() {
            return Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic(
                    format!("{} list items", self.items.len()),
                    format!("{} list items", items.len()),
                ),
            ));
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(20));

        for (shape, item) in self.items.iter().zip(items.iter()) {
            let matched = shape.check_expr(cx, item)?;
            if !matched.accepted {
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }

        if let Some(rest) = &self.rest {
            for item in items.iter().skip(self.items.len()) {
                let matched = rest.check_expr(cx, item)?;
                if !matched.accepted {
                    return Ok(matched);
                }
                out.captures.extend(matched.captures);
                out.score += matched.score;
            }
        }

        Ok(out)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("list shape");
        for item in &self.items {
            doc = doc.with_detail(item.describe(cx)?.name);
        }
        if self.rest.is_some() {
            doc = doc.with_detail("rest".to_owned());
        }
        Ok(doc)
    }
}

/// A shape that wraps an inner shape and binds the match under a name.
///
/// When the inner shape accepts, the matched expression (and value, where the
/// inner shape is not total) is recorded in the match captures under `name`,
/// feeding shape-driven binding.
pub struct CaptureShape {
    name: sim_kernel::Symbol,
    inner: Arc<dyn Shape>,
}

impl CaptureShape {
    /// Build a capture that binds the inner shape's match under `name`.
    pub fn new(name: sim_kernel::Symbol, inner: Arc<dyn Shape>) -> Self {
        Self { name, inner }
    }

    /// The capture name bound on a successful match.
    pub fn name(&self) -> &sim_kernel::Symbol {
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
            sim_kernel::Symbol::qualified("shape-capture-parent", self.name.to_string()),
            self.inner.clone(),
        )])
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        shape_is_subshape_of(cx, self.inner.as_ref(), parent).map(Some)
    }

    fn is_total(&self) -> bool {
        self.inner.is_total()
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

/// A parser that turns shape source text into an [`Expr`] for [`PrattShape`].
///
/// Implementations plug a concrete grammar (the codec layer, not the kernel)
/// into shape matching; `parse_expr` should report a parse failure as
/// `Error::Eval` so the shape rejects rather than aborts.
pub trait ShapeExprParser: Send + Sync {
    /// A short human-readable name for this parser, used in shape descriptions.
    fn label(&self) -> &str;

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
