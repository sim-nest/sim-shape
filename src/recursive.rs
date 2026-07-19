//! Recursive shape descriptors backed by named definitions.

use std::cell::RefCell;
use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, Shape, ShapeDoc, ShapeMatch, Symbol, Value};

use crate::recursion::DepthGuard;

type DefFrame = Vec<(Symbol, Arc<dyn Shape>)>;

thread_local! {
    static DEF_STACK: RefCell<Vec<DefFrame>> = RefCell::new(Vec::new());
}

/// A shape with a root descriptor and named definitions available to refs.
#[derive(Clone)]
pub struct ShapeDefs {
    /// The root shape checked by this descriptor.
    pub root: Arc<dyn Shape>,
    /// Named shapes that [`ShapeDefRef`] entries may resolve while checking.
    pub defs: Vec<(Symbol, Arc<dyn Shape>)>,
}

impl ShapeDefs {
    /// Build a recursive descriptor from a root shape and named definitions.
    pub fn new(root: Arc<dyn Shape>, defs: Vec<(Symbol, Arc<dyn Shape>)>) -> Self {
        Self { root, defs }
    }

    /// Root shape for this recursive descriptor.
    pub fn root(&self) -> &Arc<dyn Shape> {
        &self.root
    }

    /// Named definitions available while checking this descriptor.
    pub fn defs(&self) -> &[(Symbol, Arc<dyn Shape>)] {
        &self.defs
    }
}

/// A reference to a named shape inside the surrounding [`ShapeDefs`].
#[derive(Clone)]
pub struct ShapeDefRef {
    /// The definition name to resolve.
    pub name: Symbol,
}

impl ShapeDefRef {
    /// Build a definition reference.
    pub fn new(name: Symbol) -> Self {
        Self { name }
    }

    /// The definition name this reference resolves.
    pub fn name(&self) -> &Symbol {
        &self.name
    }
}

impl Shape for ShapeDefs {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("shape", "Defs"))
    }

    fn is_effectful(&self) -> bool {
        self.root.is_effectful() || self.defs.iter().any(|(_, shape)| shape.is_effectful())
    }

    fn is_total(&self) -> bool {
        self.root.is_total()
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let _scope = DefScope::push(&self.defs);
        self.root.check_value(cx, value)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let _scope = DefScope::push(&self.defs);
        self.root.check_expr(cx, expr)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("shape defs").with_detail(self.root.describe(cx)?.name);
        for (name, _) in &self.defs {
            doc = doc.with_detail(name.to_string());
        }
        Ok(doc)
    }
}

impl Shape for ShapeDefRef {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("shape", "Ref"))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        self.with_resolved_shape(|shape| shape.check_value(cx, value))
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        self.with_resolved_shape(|shape| shape.check_expr(cx, expr))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("shape ref").with_detail(self.name.to_string()))
    }
}

impl ShapeDefRef {
    fn with_resolved_shape(
        &self,
        check: impl FnOnce(Arc<dyn Shape>) -> Result<ShapeMatch>,
    ) -> Result<ShapeMatch> {
        let Some(_guard) = DepthGuard::enter() else {
            return Ok(ShapeMatch::reject(format!(
                "shape-ref: recursion budget exhausted while resolving {}",
                self.name
            )));
        };
        let Some(shape) = resolve_def(&self.name) else {
            return Ok(ShapeMatch::reject(format!(
                "shape-ref: undefined reference {}",
                self.name
            )));
        };
        check(shape)
    }
}

struct DefScope;

impl DefScope {
    fn push(defs: &[(Symbol, Arc<dyn Shape>)]) -> Self {
        DEF_STACK.with(|stack| stack.borrow_mut().push(defs.to_vec()));
        Self
    }
}

impl Drop for DefScope {
    fn drop(&mut self) {
        DEF_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn resolve_def(name: &Symbol) -> Option<Arc<dyn Shape>> {
    DEF_STACK.with(|stack| {
        stack.borrow().iter().rev().find_map(|frame| {
            frame
                .iter()
                .rev()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, shape)| shape.clone())
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_citizen::{CitizenLib, value_from_expr};
    use sim_kernel::{
        Cx, DefaultFactory, Expr, NoopEvalPolicy, NumberLiteral, Symbol, read_construct_capability,
    };

    use super::{ShapeDefRef, ShapeDefs};
    use crate::grammar::{Production, shape_grammar_graph};
    use crate::recursion::MAX_SHAPE_DEPTH;
    use crate::{AnyShape, ExprKind, ExprKindShape, ListShape, OneOfShape, Shape};

    fn number_expr(text: &str) -> Expr {
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: text.to_owned(),
        })
    }

    fn list_expr(depth: usize) -> Expr {
        let mut expr = Expr::Nil;
        for index in 0..depth {
            expr = Expr::List(vec![number_expr(&index.to_string()), expr]);
        }
        expr
    }

    fn node_shape() -> ShapeDefs {
        let node = Symbol::new("Node");
        ShapeDefs::new(
            Arc::new(ShapeDefRef::new(node.clone())),
            vec![(
                node.clone(),
                Arc::new(OneOfShape::new(vec![
                    Arc::new(ExprKindShape::new(ExprKind::Nil)),
                    Arc::new(ListShape::new(vec![
                        Arc::new(ExprKindShape::new(ExprKind::Number)),
                        Arc::new(ShapeDefRef::new(node)),
                    ])),
                ])),
            )],
        )
    }

    #[test]
    fn recursive_shape_accepts_exprs_and_values_within_depth_bound() {
        let shape = node_shape();
        let expr = list_expr(MAX_SHAPE_DEPTH - 1);
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

        assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);

        let value = value_from_expr(&mut cx, &expr).unwrap();
        assert!(shape.check_value(&mut cx, value).unwrap().accepted);
    }

    #[test]
    fn recursive_shape_rejects_when_depth_budget_is_spent() {
        let shape = node_shape();
        let expr = list_expr(MAX_SHAPE_DEPTH);
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let matched = shape.check_expr(&mut cx, &expr).unwrap();

        assert!(!matched.accepted);
        assert!(
            matched
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("recursion budget exhausted"))
        );
    }

    #[test]
    fn recursive_shape_rejects_undefined_refs() {
        let shape = ShapeDefs::new(
            Arc::new(ShapeDefRef::new(Symbol::new("Missing"))),
            Vec::new(),
        );
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let matched = shape.check_expr(&mut cx, &Expr::Nil).unwrap();

        assert!(!matched.accepted);
        assert!(
            matched
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("undefined reference Missing"))
        );
    }

    #[test]
    fn recursive_shape_lowers_to_ref_and_defs() {
        let graph = shape_grammar_graph(&node_shape()).unwrap();
        let node = Symbol::new("Node");

        assert_eq!(graph.root, Production::Ref(node.clone()));
        assert_eq!(graph.defs.len(), 1);
        assert_eq!(graph.defs[0].0, node.clone());
        assert!(contains_ref(&graph.defs[0].1, &node));
    }

    #[test]
    fn recursive_shape_roundtrips_through_read_construct() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        cx.load_lib(&CitizenLib::all()).unwrap();
        cx.grant(read_construct_capability());
        let node = Symbol::new("Node");
        let shape: Arc<dyn Shape> = Arc::new(ShapeDefs::new(
            Arc::new(ShapeDefRef::new(node.clone())),
            vec![(node, Arc::new(AnyShape))],
        ));
        let encoded = crate::citizen::encode_shape_expr(shape.as_ref()).unwrap();
        let Expr::Call { operator, args } = &encoded else {
            panic!("shape defs should encode as a constructor call");
        };
        let Expr::Symbol(class) = operator.as_ref() else {
            panic!("shape defs constructor operator should be a symbol");
        };
        let values = args
            .iter()
            .map(|arg| value_from_expr(&mut cx, arg))
            .collect::<sim_kernel::Result<Vec<_>>>()
            .unwrap();
        let decoded = cx.read_construct(class, values).unwrap();
        let decoded_shape = decoded
            .object()
            .as_shape()
            .expect("shape read-construct should return a shape value");

        assert!(
            decoded_shape
                .check_expr(&mut cx, &Expr::Bool(true))
                .unwrap()
                .accepted
        );
        assert_eq!(
            crate::citizen::encode_shape_expr(decoded_shape).unwrap(),
            encoded
        );
    }

    fn contains_ref(production: &Production, name: &Symbol) -> bool {
        match production {
            Production::Ref(candidate) => candidate == name,
            Production::Seq(items) | Production::Alt(items) => {
                items.iter().any(|item| contains_ref(item, name))
            }
            Production::Repeat { inner, .. } => contains_ref(inner, name),
            Production::Call { head, args } => {
                contains_ref(head, name) || args.iter().any(|arg| contains_ref(arg, name))
            }
            Production::Terminal(_) => false,
        }
    }
}
