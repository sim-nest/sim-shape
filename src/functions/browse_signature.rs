//! Non-enforcing callable signature metadata.
//!
//! This adapter is deliberately observational: it delegates both evaluated
//! and raw-expression calls to the wrapped callable and supplies only the two
//! browse slots defined by the kernel's [`Callable`] protocol.

use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClaimPattern, ClaimSink, ClassRef, Cx, Datum, Error, Object, ObjectCompat,
    ObjectHeader, OpKey, RawArgs, Result, ShapeRef, Value,
};

/// A callable decorated with argument and result Shapes used only by browsers.
///
/// Construction fails closed when `callable` is not callable or either
/// metadata value does not expose the Shape protocol. Invocation never reads
/// the metadata and therefore cannot check, coerce, rank, or select calls.
#[derive(Clone)]
pub struct BrowseSignature {
    callable: Value,
    args: Option<ShapeRef>,
    result: Option<ShapeRef>,
}

impl BrowseSignature {
    /// Validate and build a browse-only signature around `callable`.
    pub fn new(callable: Value, args: Option<ShapeRef>, result: Option<ShapeRef>) -> Result<Self> {
        if callable.object().as_callable().is_none() {
            return Err(Error::HostError(
                "browse signature requires a callable value".to_owned(),
            ));
        }
        for (slot, shape) in [("arguments", args.as_ref()), ("result", result.as_ref())] {
            if shape.is_some_and(|value| value.object().as_shape().is_none()) {
                return Err(Error::HostError(format!(
                    "browse signature {slot} metadata must be a Shape value"
                )));
            }
        }
        Ok(Self {
            callable,
            args,
            result,
        })
    }

    fn inner(&self) -> &dyn Callable {
        self.callable
            .object()
            .as_callable()
            .expect("BrowseSignature construction validated the callable")
    }
}

/// Wrap a callable as a runtime value with non-enforcing browse metadata.
pub fn browse_signature(
    cx: &mut Cx,
    callable: Value,
    args: Option<ShapeRef>,
    result: Option<ShapeRef>,
) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(BrowseSignature::new(callable, args, result)?))
}

impl Object for BrowseSignature {
    fn header(&self) -> &ObjectHeader {
        self.callable.object().header()
    }

    fn op(&self, key: &OpKey) -> Option<&dyn sim_kernel::Op> {
        self.callable.object().op(key)
    }

    fn claims(&self, cx: &mut Cx, pattern: &ClaimPattern, sink: &mut dyn ClaimSink) -> Result<()> {
        self.callable.object().claims(cx, pattern, sink)
    }

    fn snapshot(&self, cx: &mut Cx) -> Result<Option<Datum>> {
        self.callable.object().snapshot(cx)
    }

    fn display(&self, cx: &mut Cx) -> Result<String> {
        self.callable.object().display(cx)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for BrowseSignature {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        self.callable.object().class(cx)
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_expr(&self, cx: &mut Cx) -> Result<sim_kernel::Expr> {
        self.callable.object().as_expr(cx)
    }

    fn truth(&self, cx: &mut Cx) -> Result<bool> {
        self.callable.object().truth(cx)
    }

    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        self.callable.object().as_table(cx)
    }
}

impl Callable for BrowseSignature {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.inner().call(cx, args)
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(self.args.clone())
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(self.result.clone())
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        self.inner().call_exprs(cx, args)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use sim_kernel::{DefaultFactory, Expr, HybridPolicy, Object, ObjectCompat, RawArgs, Symbol};

    use super::*;
    use crate::{
        AnyShape, ExactExprShape, ExprKindShape, ListShape, OneOfShape, Shape, shape_value,
    };

    struct Specimen {
        evaluated_calls: Arc<AtomicUsize>,
        raw_calls: Arc<AtomicUsize>,
    }

    impl Object for Specimen {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok("#<non-typescript-specimen>".to_owned())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl ObjectCompat for Specimen {
        fn as_callable(&self) -> Option<&dyn Callable> {
            Some(self)
        }
    }

    impl Callable for Specimen {
        fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
            self.evaluated_calls.fetch_add(1, Ordering::SeqCst);
            cx.factory().string("unchecked-result".to_owned())
        }

        fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
            self.raw_calls.fetch_add(1, Ordering::SeqCst);
            cx.factory().expr(Expr::List(args.into_exprs()))
        }
    }

    fn cx() -> Cx {
        Cx::new(Arc::new(HybridPolicy), Arc::new(DefaultFactory))
    }

    fn specimen(cx: &mut Cx) -> (Value, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let evaluated_calls = Arc::new(AtomicUsize::new(0));
        let raw_calls = Arc::new(AtomicUsize::new(0));
        let value = cx
            .factory()
            .opaque(Arc::new(Specimen {
                evaluated_calls: evaluated_calls.clone(),
                raw_calls: raw_calls.clone(),
            }))
            .unwrap();
        (value, evaluated_calls, raw_calls)
    }

    #[test]
    fn metadata_is_observational_for_evaluated_and_raw_calls() {
        let mut cx = cx();
        let (inner, evaluated_calls, raw_calls) = specimen(&mut cx);
        let impossible_args = shape_value(
            Symbol::new("literal-args"),
            Arc::new(ExactExprShape::new(Expr::List(vec![Expr::String(
                "never supplied".to_owned(),
            )]))),
        );
        let impossible_result = shape_value(
            Symbol::new("number-result"),
            Arc::new(ExprKindShape::new(sim_kernel::ExprKind::Number)),
        );
        let wrapped = browse_signature(
            &mut cx,
            inner,
            Some(impossible_args.clone()),
            Some(impossible_result.clone()),
        )
        .unwrap();
        let callable = wrapped.object().as_callable().unwrap();

        let result = callable.call(&mut cx, Args::new(Vec::new())).unwrap();
        assert_eq!(
            result.object().display(&mut cx).unwrap(),
            "unchecked-result"
        );
        assert_eq!(evaluated_calls.load(Ordering::SeqCst), 1);

        let raw = vec![Expr::Symbol(Symbol::new("unevaluated"))];
        let result = callable
            .call_exprs(&mut cx, RawArgs::new(raw.clone()))
            .unwrap();
        assert_eq!(result.object().as_expr(&mut cx).unwrap(), Expr::List(raw));
        assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
        assert_eq!(evaluated_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            callable.browse_args_shape(&mut cx).unwrap(),
            Some(impossible_args)
        );
        assert_eq!(
            callable.browse_result_shape(&mut cx).unwrap(),
            Some(impossible_result)
        );
    }

    #[test]
    fn faithful_shape_categories_are_retained_and_non_shapes_fail_closed() {
        let mut cx = cx();
        let categories: Vec<(&str, Arc<dyn Shape>)> = vec![
            (
                "primitive",
                Arc::new(ExprKindShape::new(sim_kernel::ExprKind::Bool)),
            ),
            ("literal", Arc::new(ExactExprShape::new(Expr::Bool(true)))),
            (
                "union",
                Arc::new(OneOfShape::new(vec![
                    Arc::new(ExprKindShape::new(sim_kernel::ExprKind::Bool)),
                    Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String)),
                ])),
            ),
            (
                "tuple-or-array",
                Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
            ),
            ("bounded-named", Arc::new(AnyShape)),
        ];

        for (category, shape) in categories {
            let (inner, _, _) = specimen(&mut cx);
            let metadata = shape_value(Symbol::qualified("law", category), shape);
            let wrapped = browse_signature(&mut cx, inner, Some(metadata.clone()), None).unwrap();
            assert_eq!(
                wrapped
                    .object()
                    .as_callable()
                    .unwrap()
                    .browse_args_shape(&mut cx)
                    .unwrap(),
                Some(metadata),
                "{category} metadata must be retained without projection"
            );
        }

        let (inner, _, _) = specimen(&mut cx);
        let not_a_shape = cx
            .factory()
            .string("conditional type widened to any".to_owned())
            .unwrap();
        let error = BrowseSignature::new(inner, Some(not_a_shape), None)
            .err()
            .expect("non-equivalent metadata must be rejected");
        assert!(error.to_string().contains("must be a Shape value"));
    }
}
