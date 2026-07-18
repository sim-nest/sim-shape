//! Positional and variadic list-shape combinator.

use std::{cmp::Ordering, sync::Arc};

use sim_kernel::{Cx, Error, Expr, Result, Value, shape_is_subshape_of};

use crate::base::{MatchScore, Shape, ShapeDoc, ShapeMatch};
use crate::diagnostics::{expected_shape_diagnostic, expr_actual_label};

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

        // The expr path walks the tail to collect rest captures; the value path
        // mirrors it so equivalent list values capture like expression forms.
        // It still terminates on lazy tails once a total rest accepts an
        // element and binds nothing, because further total non-binding matches
        // cannot add captures.
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
    fn is_effectful(&self) -> bool {
        self.items.iter().any(|shape| shape.is_effectful())
            || self.rest.as_ref().is_some_and(|shape| shape.is_effectful())
    }

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
