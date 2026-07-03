//! Boolean shape combinators: `AndShape`, `OrShape`, and `NotShape`, plus the
//! `OrStrategy` that picks between first-match and best-match disjunction.

use std::sync::Arc;

use sim_kernel::{Cx, Diagnostic, Expr, Result, Value, shape_is_subshape_of};

use crate::{
    algebra::{capture_symbol, number_expr, number_value},
    base::{Bindings, MatchScore, Shape, ShapeDoc, ShapeMatch},
};

/// Shape that accepts only when every child shape accepts.
///
/// `AndShape` short-circuits on the first rejecting child. On success it
/// combines child captures and adds their scores to the combiner base score.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{AndShape, AnyShape, ExprKind, ExprKindShape, Shape};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = AndShape::new(vec![
///     Arc::new(AnyShape),
///     Arc::new(ExprKindShape::new(ExprKind::Bool)),
/// ]);
///
/// assert!(shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap().accepted);
/// assert!(!shape
///     .check_expr(&mut cx, &Expr::String("not bool".to_owned()))
///     .unwrap()
///     .accepted);
/// ```
pub struct AndShape {
    parts: Vec<Arc<dyn Shape>>,
}

impl AndShape {
    /// Build a conjunction over the given child shapes.
    pub fn new(parts: Vec<Arc<dyn Shape>>) -> Self {
        Self { parts }
    }

    /// Return the child shapes in conjunction order.
    pub fn parts(&self) -> &[Arc<dyn Shape>] {
        &self.parts
    }
}

impl Shape for AndShape {
    fn is_total(&self) -> bool {
        self.parts.iter().all(|part| part.is_total())
    }

    fn is_effectful(&self) -> bool {
        self.parts.iter().any(|part| part.is_effectful())
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        for part in &self.parts {
            if shape_is_subshape_of(cx, part.as_ref(), parent)? {
                return Ok(Some(true));
            }
        }

        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        for parent_part in parent.parts() {
            let mut covered = false;
            for part in &self.parts {
                if shape_is_subshape_of(cx, part.as_ref(), parent_part.as_ref())? {
                    covered = true;
                    break;
                }
            }
            if !covered {
                return Ok(None);
            }
        }
        Ok(Some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if self.parts.is_empty() {
            return Ok(ShapeMatch::accept(MatchScore::exact(0)));
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(10));
        for part in &self.parts {
            let mut matched = part.check_value(cx, value.clone())?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-and: child rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        Ok(out)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        if self.parts.is_empty() {
            return Ok(ShapeMatch::accept(MatchScore::exact(0)));
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(10));
        for part in &self.parts {
            let mut matched = part.check_expr(cx, expr)?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-and: child rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        Ok(out)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("and shape");
        for part in &self.parts {
            doc = doc.with_detail(part.describe(cx)?.name);
        }
        Ok(doc)
    }
}

/// Branch selection policy for [`OrShape`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrStrategy {
    /// Return the first accepted branch in registration order.
    FirstMatch,
    /// Evaluate every branch and return the accepted branch with best score.
    BestScore,
}

/// Shape that accepts when at least one child shape accepts.
///
/// By default, `OrShape` returns the leftmost accepted branch. Use
/// [`OrShape::with_strategy`] with [`OrStrategy::BestScore`] when callers need
/// all branches evaluated for score-based choice.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{ExprKind, ExprKindShape, OrShape, Shape};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = OrShape::new(vec![
///     Arc::new(ExprKindShape::new(ExprKind::Bool)),
///     Arc::new(ExprKindShape::new(ExprKind::String)),
/// ]);
///
/// assert!(shape
///     .check_expr(&mut cx, &Expr::String("accepted".to_owned()))
///     .unwrap()
///     .accepted);
/// ```
pub struct OrShape {
    choices: Vec<Arc<dyn Shape>>,
    strategy: OrStrategy,
}

impl OrShape {
    /// Build a disjunction that returns the first accepted branch.
    pub fn new(choices: Vec<Arc<dyn Shape>>) -> Self {
        Self::with_strategy(choices, OrStrategy::FirstMatch)
    }

    /// Build a disjunction with an explicit branch-selection strategy.
    pub fn with_strategy(choices: Vec<Arc<dyn Shape>>, strategy: OrStrategy) -> Self {
        Self { choices, strategy }
    }

    /// Return the candidate branches in registration order.
    pub fn choices(&self) -> &[Arc<dyn Shape>] {
        &self.choices
    }

    /// Return the configured branch-selection strategy.
    pub fn strategy(&self) -> OrStrategy {
        self.strategy
    }
}

impl Shape for OrShape {
    fn is_total(&self) -> bool {
        self.choices.iter().any(|choice| choice.is_total())
    }

    fn is_effectful(&self) -> bool {
        self.choices.iter().any(|choice| choice.is_effectful())
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        for choice in &self.choices {
            if !shape_is_subshape_of(cx, choice.as_ref(), parent)? {
                return Ok(None);
            }
        }
        Ok(Some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        match self.strategy {
            OrStrategy::FirstMatch => check_value_first(cx, &self.choices, value),
            OrStrategy::BestScore => check_value_best(cx, &self.choices, value),
        }
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        match self.strategy {
            OrStrategy::FirstMatch => check_expr_first(cx, &self.choices, expr),
            OrStrategy::BestScore => check_expr_best(cx, &self.choices, expr),
        }
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("or shape");
        for choice in &self.choices {
            doc = doc.with_detail(choice.describe(cx)?.name);
        }
        Ok(doc)
    }
}

/// Shape that accepts when the inner shape rejects.
///
/// `NotShape` is a predicate complement. It records the `shape/negated`
/// capture on accepted matches and does not leak captures from the rejected
/// inner match.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{ExprKind, ExprKindShape, NotShape, Shape};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = NotShape::new(Arc::new(ExprKindShape::new(ExprKind::Bool)));
///
/// assert!(shape
///     .check_expr(&mut cx, &Expr::String("not bool".to_owned()))
///     .unwrap()
///     .accepted);
/// ```
pub struct NotShape {
    inner: Arc<dyn Shape>,
}

impl NotShape {
    /// Build the complement of the given inner shape.
    pub fn new(inner: Arc<dyn Shape>) -> Self {
        Self { inner }
    }

    /// Return the negated inner shape.
    pub fn inner(&self) -> &Arc<dyn Shape> {
        &self.inner
    }
}

impl Shape for NotShape {
    fn is_effectful(&self) -> bool {
        self.inner.is_effectful()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        shape_is_subshape_of(cx, parent.inner.as_ref(), self.inner.as_ref()).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let matched = self.inner.check_value(cx, value)?;
        if matched.accepted {
            return Ok(ShapeMatch::reject("shape-not: inner accepted"));
        }
        let mut out = ShapeMatch::accept(MatchScore::exact(10));
        out.captures
            .bind_value(capture_symbol("negated"), cx.factory().bool(true)?);
        Ok(out)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let matched = self.inner.check_expr(cx, expr)?;
        if matched.accepted {
            return Ok(ShapeMatch::reject("shape-not: inner accepted"));
        }
        let mut out = ShapeMatch::accept(MatchScore::exact(10));
        out.captures
            .bind_expr(capture_symbol("negated"), Expr::Bool(true));
        Ok(out)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("not shape").with_detail(self.inner.describe(cx)?.name))
    }
}

fn check_value_first(cx: &mut Cx, choices: &[Arc<dyn Shape>], value: Value) -> Result<ShapeMatch> {
    let mut diagnostics = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let mut matched = choice.check_value(cx, value.clone())?;
        if matched.accepted {
            matched
                .captures
                .bind_value(capture_symbol("branch-index"), number_value(cx, index)?);
            return Ok(matched);
        }
        diagnostics.extend(matched.diagnostics);
    }
    reject_or(diagnostics)
}

fn check_value_best(cx: &mut Cx, choices: &[Arc<dyn Shape>], value: Value) -> Result<ShapeMatch> {
    let mut diagnostics = Vec::new();
    let mut best = None::<(usize, ShapeMatch)>;
    for (index, choice) in choices.iter().enumerate() {
        let matched = choice.check_value(cx, value.clone())?;
        if matched.accepted {
            let replace = best
                .as_ref()
                .map(|(_, current)| matched.score > current.score)
                .unwrap_or(true);
            if replace {
                best = Some((index, matched));
            }
        } else {
            diagnostics.extend(matched.diagnostics);
        }
    }
    match best {
        Some((index, mut matched)) => {
            matched
                .captures
                .bind_value(capture_symbol("branch-index"), number_value(cx, index)?);
            Ok(matched)
        }
        None => reject_or(diagnostics),
    }
}

fn check_expr_first(cx: &mut Cx, choices: &[Arc<dyn Shape>], expr: &Expr) -> Result<ShapeMatch> {
    let mut diagnostics = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let mut matched = choice.check_expr(cx, expr)?;
        if matched.accepted {
            matched
                .captures
                .bind_expr(capture_symbol("branch-index"), number_expr(index));
            return Ok(matched);
        }
        diagnostics.extend(matched.diagnostics);
    }
    reject_or(diagnostics)
}

fn check_expr_best(cx: &mut Cx, choices: &[Arc<dyn Shape>], expr: &Expr) -> Result<ShapeMatch> {
    let mut diagnostics = Vec::new();
    let mut best = None::<(usize, ShapeMatch)>;
    for (index, choice) in choices.iter().enumerate() {
        let matched = choice.check_expr(cx, expr)?;
        if matched.accepted {
            let replace = best
                .as_ref()
                .map(|(_, current)| matched.score > current.score)
                .unwrap_or(true);
            if replace {
                best = Some((index, matched));
            }
        } else {
            diagnostics.extend(matched.diagnostics);
        }
    }
    match best {
        Some((index, mut matched)) => {
            matched
                .captures
                .bind_expr(capture_symbol("branch-index"), number_expr(index));
            Ok(matched)
        }
        None => reject_or(diagnostics),
    }
}

fn reject_or(mut diagnostics: Vec<Diagnostic>) -> Result<ShapeMatch> {
    diagnostics.insert(0, Diagnostic::error("shape-or: no branch accepted"));
    Ok(ShapeMatch {
        accepted: false,
        captures: Bindings::new(),
        score: MatchScore::reject(),
        diagnostics,
    })
}
