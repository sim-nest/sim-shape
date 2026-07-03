//! Atomic shapes: the leaf matchers `AnyShape`, `ExprKindShape`,
//! `NumberValueShape`, `ClassShape`, and `ExactExprShape`.

use crate::base::{ExprKind, MatchScore, Shape, ShapeDoc, ShapeMatch};
use crate::diagnostics::{expected_shape_diagnostic, expr_actual_label};
use crate::functions::shape_value;
use crate::primitives::object::ObjectExpr;
use crate::recursion::{DepthGuard, class_is_subclass_of_guarded, is_cyclic_parent_edge};
use sim_kernel::{Cx, Expr, Result, ShapeRef, Symbol, Value};

/// The total shape that accepts every value and expression.
///
/// This is the `core/Any` atomic shape: it matches anything with an exact
/// score and reports `is_total() == true`.
///
/// # Examples
///
/// ```rust
/// # use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// use sim_shape::{AnyShape, Shape};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let matched = AnyShape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();
/// assert!(matched.accepted);
/// assert!(AnyShape.is_total());
/// ```
pub struct AnyShape;

impl Shape for AnyShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("core", "Any"))
    }

    fn is_total(&self) -> bool {
        true
    }

    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(0)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(0)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("Any"))
    }
}

/// A shape that matches expressions of a single [`ExprKind`].
///
/// Accepts any expression whose syntactic kind equals the configured kind (for
/// example, every `String` expression); a subshape of the `core/Expr` shape.
pub struct ExprKindShape {
    kind: ExprKind,
}

impl ExprKindShape {
    /// Build a shape that matches expressions of the given kind.
    pub fn new(kind: ExprKind) -> Self {
        Self { kind }
    }

    /// The expression kind this shape matches.
    pub fn kind(&self) -> &ExprKind {
        &self.kind
    }
}

impl Shape for ExprKindShape {
    fn parents(&self, cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        Ok(cx
            .registry()
            .shape_by_symbol(&Symbol::qualified("core", "Expr"))
            .cloned()
            .into_iter()
            .collect())
    }

    fn is_subshape_of(&self, _cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        if let Some(parent) = parent.as_any().downcast_ref::<Self>() {
            return Ok(Some(self.kind == parent.kind));
        }
        if parent.as_any().is::<ExactExprShape>()
            || matches!(
                parent.symbol(),
                Some(symbol) if symbol == Symbol::qualified("core", "ExactExprShape")
            )
        {
            return Ok(Some(false));
        }
        Ok(matches!(
            parent.symbol(),
            Some(symbol) if symbol == Symbol::qualified("core", "Expr")
        )
        .then_some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        if self.kind.matches(expr) {
            Ok(ShapeMatch::accept(MatchScore::exact(10)))
        } else {
            Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic(
                    format!("{} expression", self.kind.name()),
                    expr_actual_label(expr),
                ),
            ))
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!("expr-kind {}", self.kind.name())))
    }
}

/// A shape that matches number-domain values and number expressions.
///
/// On values it accepts anything the active number backend recognizes as a
/// number; on expressions it accepts literal `Number` forms. Named
/// `core/Number`.
pub struct NumberValueShape;

impl Shape for NumberValueShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("core", "Number"))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if cx.number_value_ref(value)?.is_some() {
            Ok(ShapeMatch::accept(MatchScore::exact(20)))
        } else {
            Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic("number value", "non-number value"),
            ))
        }
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        if matches!(expr, Expr::Number(_)) {
            Ok(ShapeMatch::accept(MatchScore::exact(10)))
        } else {
            Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic("number expression", expr_actual_label(expr)),
            ))
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("number value"))
    }
}

/// A shape that matches values and expressions of a named class.
///
/// Accepts a value whose class is, or is a subclass of, the named class, and
/// accepts class-symbol or object expressions for that class. Subshape
/// relationships follow the class hierarchy in the registry.
pub struct ClassShape {
    symbol: Symbol,
}

impl ClassShape {
    /// Build a shape that matches the class named by `symbol`.
    pub fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }

    /// The class symbol this shape matches against.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
}

impl Shape for ClassShape {
    fn parents(&self, cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        let Some(class_value) = cx.registry().class_by_symbol(&self.symbol).cloned() else {
            return Ok(Vec::new());
        };
        let Some(class) = class_value.object().as_class() else {
            return Ok(Vec::new());
        };
        let child_id = class.id();
        let parent_classes = class.parents(cx)?;
        let mut out = Vec::new();
        for parent in parent_classes {
            let Some(parent_class) = parent.object().as_class() else {
                continue;
            };
            // Prune cyclic back-edges so the kernel subshape walk over the
            // reported parents terminates on an adversarial hierarchy.
            if is_cyclic_parent_edge(cx, child_id, parent_class)? {
                continue;
            }
            out.push(shape_value(
                Symbol::qualified("shape-class-parent", parent_class.symbol().to_string()),
                std::sync::Arc::new(ClassShape::new(parent_class.symbol())),
            ));
        }
        Ok(out)
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        if self.symbol == parent.symbol {
            return Ok(Some(true));
        }
        let Some(child_class) = cx.registry().class_by_symbol(&self.symbol).cloned() else {
            return Ok(Some(false));
        };
        let Some(child_class) = child_class.object().as_class() else {
            return Ok(Some(false));
        };
        let Some(parent_class) = cx.registry().class_by_symbol(&parent.symbol).cloned() else {
            return Ok(Some(false));
        };
        class_is_subclass_of_guarded(cx, child_class, parent_class).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if let Some(class) = value.object().as_class()
            && class_matches(cx, class, &self.symbol)?
        {
            return Ok(ShapeMatch::accept(MatchScore::exact(30)));
        }

        let class = value.object().class(cx)?;
        let Some(class) = class.object().as_class() else {
            return Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic("class-backed value", "value without class metadata"),
            ));
        };
        if class_matches(cx, class, &self.symbol)? {
            Ok(ShapeMatch::accept(MatchScore::exact(30)))
        } else {
            Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic(
                    format!("class {}", self.symbol),
                    format!("class {}", class.symbol()),
                ),
            ))
        }
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::Symbol(symbol) if class_symbol_matches(cx, symbol, &self.symbol)? => {
                Ok(ShapeMatch::accept(MatchScore::exact(20)))
            }
            _ => {
                let object = match ObjectExpr::parse(expr) {
                    Some(object) => object,
                    None => {
                        return Ok(ShapeMatch::reject_with_diagnostic(
                            expected_shape_diagnostic(
                                format!("class {}", self.symbol),
                                expr_actual_label(expr),
                            ),
                        ));
                    }
                };
                if !class_symbol_matches(cx, &object.class, &self.symbol)? {
                    return Ok(ShapeMatch::reject_with_diagnostic(
                        expected_shape_diagnostic(
                            format!("class {}", self.symbol),
                            format!("class {}", object.class),
                        ),
                    ));
                }
                if let Some(class_value) = cx.registry().class_by_symbol(&self.symbol).cloned()
                    && let Some(class) = class_value.object().as_class()
                {
                    let shape = class.instance_shape(cx)?;
                    if let Some(shape) = shape.object().as_shape() {
                        // The instance shape can resolve back to this class
                        // (self-referential metadata); bound the re-entry so a
                        // cycle rejects rather than overflowing the stack.
                        let Some(_guard) = DepthGuard::enter() else {
                            return Ok(ShapeMatch::reject_with_diagnostic(
                                expected_shape_diagnostic(
                                    format!("class {} within shape recursion budget", self.symbol),
                                    "shape recursion budget exceeded",
                                ),
                            ));
                        };
                        let matched = shape.check_expr(cx, expr)?;
                        if matched.accepted {
                            return Ok(ShapeMatch::accept(MatchScore::exact(30)));
                        }
                        return Ok(matched);
                    }
                }
                Ok(ShapeMatch::accept(MatchScore::exact(25)))
            }
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!("class {}", self.symbol)))
    }
}

/// A shape that matches one exact expression form.
///
/// Accepts only expressions canonically equal to the stored form; a subshape of
/// the [`ExprKindShape`] for that form's kind.
///
/// # Examples
///
/// ```rust
/// # use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// use sim_shape::{ExactExprShape, Shape};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = ExactExprShape::new(Expr::Bool(true));
/// assert!(shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap().accepted);
/// assert!(!shape.check_expr(&mut cx, &Expr::Bool(false)).unwrap().accepted);
/// ```
pub struct ExactExprShape {
    expected: Expr,
}

impl ExactExprShape {
    /// Build a shape that matches only the given exact expression.
    pub fn new(expected: Expr) -> Self {
        Self { expected }
    }

    /// The exact expression this shape matches.
    pub fn expected(&self) -> &Expr {
        &self.expected
    }
}

impl Shape for ExactExprShape {
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        Ok(vec![shape_value(
            Symbol::qualified("shape-exact-parent", expr_kind_of(&self.expected).name()),
            std::sync::Arc::new(ExprKindShape::new(expr_kind_of(&self.expected))),
        )])
    }

    fn is_subshape_of(&self, _cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        if let Some(parent) = parent.as_any().downcast_ref::<Self>() {
            return Ok(Some(self.expected.canonical_eq(parent.expected())));
        }
        if let Some(parent) = parent.as_any().downcast_ref::<ExprKindShape>() {
            return Ok(Some(parent.kind().matches(&self.expected)));
        }
        Ok(None)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        if self.expected.canonical_eq(expr) {
            Ok(ShapeMatch::accept(MatchScore::exact(20)))
        } else {
            Ok(ShapeMatch::reject_with_diagnostic(
                expected_shape_diagnostic("exact expression form", expr_actual_label(expr)),
            ))
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("exact expr").with_detail(format!("{:?}", self.expected)))
    }
}

fn class_matches(cx: &mut Cx, class: &dyn sim_kernel::Class, expected: &Symbol) -> Result<bool> {
    if class.symbol() == *expected {
        return Ok(true);
    }
    let Some(expected) = cx.registry().class_by_symbol(expected).cloned() else {
        return Ok(false);
    };
    class_is_subclass_of_guarded(cx, class, expected)
}

fn class_symbol_matches(cx: &mut Cx, actual: &Symbol, expected: &Symbol) -> Result<bool> {
    if actual == expected {
        return Ok(true);
    }
    let Some(actual) = cx.registry().class_by_symbol(actual).cloned() else {
        return Ok(false);
    };
    let Some(actual) = actual.object().as_class() else {
        return Ok(false);
    };
    class_matches(cx, actual, expected)
}

fn expr_kind_of(expr: &Expr) -> ExprKind {
    match expr {
        Expr::Nil => ExprKind::Nil,
        Expr::Bool(_) => ExprKind::Bool,
        Expr::Number(_) => ExprKind::Number,
        Expr::Symbol(_) => ExprKind::Symbol,
        Expr::Local(_) => ExprKind::Symbol,
        Expr::String(_) => ExprKind::String,
        Expr::Bytes(_) => ExprKind::Bytes,
        Expr::List(_) => ExprKind::List,
        Expr::Vector(_) => ExprKind::Vector,
        Expr::Map(_) => ExprKind::Map,
        Expr::Set(_) => ExprKind::Set,
        Expr::Call { .. } => ExprKind::Call,
        Expr::Infix { .. } => ExprKind::Infix,
        Expr::Prefix { .. } => ExprKind::Prefix,
        Expr::Postfix { .. } => ExprKind::Postfix,
        Expr::Block(_) => ExprKind::Block,
        Expr::Quote { .. } => ExprKind::Quote,
        Expr::Annotated { .. } => ExprKind::Annotated,
        Expr::Extension { .. } => ExprKind::Extension,
    }
}
