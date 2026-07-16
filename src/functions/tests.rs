use std::sync::Arc;

use sim_kernel::{
    CaseId, Cx, DefaultFactory, Diagnostic, Expr, FunctionId, NoopEvalPolicy, PreparedArgs, Result,
    Symbol, Value,
};

use crate::{
    AcceptOnNoDiagnosticsHook, Bindings, DiscardOnDiagnosticPrefixHook, ExprKind, ExprKindShape,
    FunctionCase, FunctionObject, HookedShape, ListShape, MatchScore, Shape, ShapeDoc, ShapeMatch,
};

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn general_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("general".to_owned())
}

fn specific_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("specific".to_owned())
}

#[test]
fn accept_hooked_case_does_not_beat_plain_case_by_inner_subshape_proof() {
    let mut cx = cx();
    let function = FunctionObject::new(
        FunctionId(90),
        Symbol::qualified("test", "hook-widened-dispatch"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "general"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                    ExprKind::Bool,
                ))])),
                result: None,
                demand: Vec::new(),
                priority: 0,
                implementation: general_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "specific"),
                args: Arc::new(ListShape::new(vec![Arc::new(HookedShape::new(
                    Arc::new(QuietTrueOnlyShape),
                    vec![Arc::new(AcceptOnNoDiagnosticsHook)],
                ))])),
                result: None,
                demand: Vec::new(),
                priority: 0,
                implementation: specific_case_impl,
            },
        ],
    );
    let prepared = PreparedArgs::new(vec![cx.factory().bool(false).unwrap()]);

    let selected = function
        .select_case(&mut cx, &prepared)
        .expect("plain bool case should outrank widened hooked case by score");

    assert_eq!(selected.case.id, CaseId(1));
}

#[test]
fn discard_hooked_case_does_not_create_false_specificity_proof() {
    let mut cx = cx();
    let function = FunctionObject::new(
        FunctionId(91),
        Symbol::qualified("test", "hook-narrowed-dispatch"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "general"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                    ExprKind::Bool,
                ))])),
                result: None,
                demand: Vec::new(),
                priority: 0,
                implementation: general_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "specific"),
                args: Arc::new(ListShape::new(vec![Arc::new(HookedShape::new(
                    Arc::new(DiagnosticBoolShape),
                    vec![Arc::new(DiscardOnDiagnosticPrefixHook::new("inner:"))],
                ))])),
                result: None,
                demand: Vec::new(),
                priority: 0,
                implementation: specific_case_impl,
            },
        ],
    );
    let prepared = PreparedArgs::new(vec![cx.factory().bool(false).unwrap()]);

    let err = match function.select_case(&mut cx, &prepared) {
        Ok(_) => panic!("hook-narrowed case should no longer win by inner-shape proof"),
        Err(err) => err,
    };
    let sim_kernel::Error::AmbiguousOverload { candidates, .. } = err else {
        panic!("expected ambiguous overload");
    };

    assert_eq!(candidates, vec![CaseId(1), CaseId(2)]);
}

struct QuietTrueOnlyShape;

impl Shape for QuietTrueOnlyShape {
    fn is_subshape_of(&self, _cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<ExprKindShape>() else {
            return Ok(None);
        };
        Ok((*parent.kind() == ExprKind::Bool).then_some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::Bool(true) => Ok(ShapeMatch::accept(MatchScore::exact(10))),
            _ => Ok(ShapeMatch {
                accepted: false,
                captures: Bindings::new(),
                score: MatchScore::reject(),
                diagnostics: Vec::new(),
            }),
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("quiet true only"))
    }
}

struct DiagnosticBoolShape;

impl Shape for DiagnosticBoolShape {
    fn is_subshape_of(&self, _cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<ExprKindShape>() else {
            return Ok(None);
        };
        Ok((*parent.kind() == ExprKind::Bool).then_some(true))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::Bool(flag) => {
                let mut matched = ShapeMatch::accept(MatchScore::exact(10));
                if *flag {
                    matched
                        .diagnostics
                        .push(Diagnostic::info("inner: accepted true"));
                }
                Ok(matched)
            }
            _ => Ok(ShapeMatch::reject_with_diagnostic(Diagnostic::error(
                "expected bool",
            ))),
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("diagnostic bool"))
    }
}
