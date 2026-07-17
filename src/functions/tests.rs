use std::sync::Arc;

use sim_kernel::{
    Callable, CaseId, Cx, DefaultFactory, Demand, Diagnostic, Expr, FunctionId, HybridPolicy,
    NoopEvalPolicy, NumberLiteral, PreparedArgs, RawArgs, Result, Symbol, Value,
};

use crate::{
    AcceptOnNoDiagnosticsHook, AnyShape, Bindings, DiscardOnDiagnosticPrefixHook, ExactExprShape,
    ExprKind, ExprKindShape, FunctionCase, FunctionObject, HookedShape, ListShape, MatchScore,
    Shape, ShapeDoc, ShapeMatch,
};

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn hybrid_cx() -> Cx {
    Cx::new(Arc::new(HybridPolicy), Arc::new(DefaultFactory))
}

fn number_expr(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: text.to_owned(),
    })
}

fn general_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("general".to_owned())
}

fn specific_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("specific".to_owned())
}

fn expect_demand_conflict(function: &FunctionObject, cx: &mut Cx, expr: Expr, cases: &[CaseId]) {
    let err = Callable::call_exprs(function, cx, RawArgs::new(vec![expr]))
        .expect_err("same-priority mixed demand groups should fail loudly");
    let sim_kernel::Error::Eval(message) = err else {
        panic!("expected explicit mixed-demand eval error");
    };
    let diagnostics = cx.take_diagnostics();

    assert!(message.contains("argument 0"));
    assert!(!diagnostics.is_empty());
    assert_eq!(
        diagnostics[0].code,
        Some(Symbol::qualified("shape", "overload-selection"))
    );
    assert!(diagnostics[0].message.contains("argument 0"));
    for case in cases {
        let case = format!("{case:?}");
        assert!(message.contains(&case));
        assert!(diagnostics[0].message.contains(&case));
    }
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

#[test]
fn mixed_demand_overload_avoids_lower_priority_value_forcing() {
    let mut cx = hybrid_cx();
    let function = FunctionObject::new(
        FunctionId(92),
        Symbol::qualified("test", "mixed-demand-priority"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "expr"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExactExprShape::new(
                    Expr::Symbol(Symbol::new("quoted")),
                ))])),
                result: None,
                demand: vec![Demand::Expr],
                priority: 10,
                implementation: specific_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "value"),
                args: Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                result: None,
                demand: vec![Demand::Value],
                priority: 0,
                implementation: general_case_impl,
            },
        ],
    );

    let result = Callable::call_exprs(
        &function,
        &mut cx,
        RawArgs::new(vec![Expr::Symbol(Symbol::new("quoted"))]),
    )
    .expect("higher-priority expr-demand case should win before lower value-demand forcing");

    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::String("specific".to_owned())
    );
}

#[test]
fn mixed_demand_overload_same_priority_reports_conflict() {
    let mut cx = hybrid_cx();
    let function = FunctionObject::new(
        FunctionId(93),
        Symbol::qualified("test", "mixed-demand-conflict"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "expr"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                    ExprKind::Number,
                ))])),
                result: None,
                demand: vec![Demand::Expr],
                priority: 0,
                implementation: specific_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "value"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                    ExprKind::Number,
                ))])),
                result: None,
                demand: vec![Demand::Value],
                priority: 0,
                implementation: general_case_impl,
            },
        ],
    );

    expect_demand_conflict(
        &function,
        &mut cx,
        number_expr("1"),
        &[CaseId(1), CaseId(2)],
    );
}

#[test]
fn mixed_demand_overload_conflict_does_not_force_value_group() {
    let mut cx = hybrid_cx();
    let function = FunctionObject::new(
        FunctionId(95),
        Symbol::qualified("test", "mixed-demand-no-force"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "expr"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExactExprShape::new(
                    Expr::Symbol(Symbol::new("quoted")),
                ))])),
                result: None,
                demand: vec![Demand::Expr],
                priority: 0,
                implementation: specific_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "value"),
                args: Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                result: None,
                demand: vec![Demand::Value],
                priority: 0,
                implementation: general_case_impl,
            },
        ],
    );

    expect_demand_conflict(
        &function,
        &mut cx,
        Expr::Symbol(Symbol::new("quoted")),
        &[CaseId(1), CaseId(2)],
    );
}

#[test]
fn mixed_demand_overload_groups_non_contiguous_equal_priorities() {
    let mut cx = hybrid_cx();
    let function = FunctionObject::new(
        FunctionId(96),
        Symbol::qualified("test", "mixed-demand-non-contiguous"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "expr"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExactExprShape::new(
                    Expr::Symbol(Symbol::new("quoted")),
                ))])),
                result: None,
                demand: vec![Demand::Expr],
                priority: 0,
                implementation: specific_case_impl,
            },
            FunctionCase {
                id: CaseId(9),
                name: Symbol::qualified("case", "higher-priority-miss"),
                args: Arc::new(ListShape::new(vec![Arc::new(ExactExprShape::new(
                    Expr::Symbol(Symbol::new("other")),
                ))])),
                result: None,
                demand: vec![Demand::Expr],
                priority: 10,
                implementation: general_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "value"),
                args: Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                result: None,
                demand: vec![Demand::Value],
                priority: 0,
                implementation: general_case_impl,
            },
        ],
    );

    expect_demand_conflict(
        &function,
        &mut cx,
        Expr::Symbol(Symbol::new("quoted")),
        &[CaseId(1), CaseId(2)],
    );
}

#[test]
fn mixed_demand_plan_normalizes_safe_value_family() {
    let function = FunctionObject::new(
        FunctionId(94),
        Symbol::qualified("test", "mixed-demand-plan"),
        vec![
            FunctionCase {
                id: CaseId(1),
                name: Symbol::qualified("case", "value"),
                args: Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                result: None,
                demand: vec![Demand::Value],
                priority: 0,
                implementation: general_case_impl,
            },
            FunctionCase {
                id: CaseId(2),
                name: Symbol::qualified("case", "bool"),
                args: Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                result: None,
                demand: vec![Demand::Bool],
                priority: 0,
                implementation: specific_case_impl,
            },
        ],
    );

    assert_eq!(function.demand_plan().unwrap(), vec![Demand::Value]);
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
