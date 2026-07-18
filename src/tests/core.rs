use std::sync::Arc;

use sim_kernel::{
    CaseId, Expr, FunctionId, HintMetadata, NumberLiteral, PreparedArgs, Symbol,
    shape_is_subshape_of,
};

use crate::{
    AnyShape, Bindings, CaptureShape, EffectfulShape, ExactExprShape, ExprKind, ExprKindShape,
    FunctionCase, FunctionObject, ListShape, OneOfShape, PrattShape, Shape,
};

use super::{FakePrattParser, cx, test_case_impl};

#[test]
fn expr_kind_shape_checks_values_and_exprs() {
    let mut cx = cx();
    let shape = ExprKindShape::new(ExprKind::String);
    let expr = Expr::String("ok".to_owned());
    assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);

    let value = cx.factory().string("ok".to_owned()).unwrap();
    assert!(shape.check_value(&mut cx, value).unwrap().accepted);
}

#[test]
fn expr_kind_rejection_carries_shape_hint() {
    let mut cx = cx();
    let shape = ExprKindShape::new(ExprKind::Number);
    let matched = shape
        .check_expr(&mut cx, &Expr::String("text".to_owned()))
        .unwrap();

    assert!(!matched.accepted);
    let hints = HintMetadata::collect_from_diagnostic(&matched.diagnostics[0]);
    assert_eq!(hints[0].kind, Symbol::qualified("shape-hint", "expected"));
    assert!(hints[0].radar_text().contains("string expression"));
}

#[test]
fn capture_rejection_adds_binding_hint() {
    let mut cx = cx();
    let shape = CaptureShape::new(
        Symbol::new("n"),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
    );
    let matched = shape
        .check_expr(&mut cx, &Expr::String("text".to_owned()))
        .unwrap();

    assert!(!matched.accepted);
    let hints = HintMetadata::collect_from_diagnostic(&matched.diagnostics[0]);
    assert_eq!(hints[0].kind, Symbol::qualified("shape-hint", "binding"));
    assert!(hints[0].radar_text().contains("n"));
}

#[test]
fn callable_overload_rejection_reports_hints() {
    let mut cx = cx();
    let function = FunctionObject::new(
        FunctionId(7),
        Symbol::qualified("test", "only-number"),
        vec![FunctionCase {
            id: CaseId(1),
            name: Symbol::qualified("case", "number"),
            args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                ExprKind::Number,
            ))])),
            result: None,
            demand: Vec::new(),
            priority: 0,
            implementation: test_case_impl,
        }],
    );
    let prepared = PreparedArgs::new(vec![cx.factory().string("text".to_owned()).unwrap()]);

    let err = match function.select_case(&mut cx, &prepared) {
        Ok(_) => panic!("expected no matching overload"),
        Err(err) => err,
    };
    let sim_kernel::Error::NoMatchingOverload { diagnostics, .. } = err else {
        panic!("expected no matching overload");
    };
    let overload = HintMetadata::collect_from_diagnostic(&diagnostics[0]);
    let callable = HintMetadata::collect_from_diagnostic(&diagnostics[2]);

    assert_eq!(
        overload[0].kind,
        Symbol::qualified("shape-hint", "overload-selection")
    );
    assert_eq!(
        callable[0].kind,
        Symbol::qualified("shape-hint", "callable-mismatch")
    );
}

#[test]
fn bindings_can_populate_context_env() {
    let mut cx = cx();
    let mut bindings = Bindings::new();
    bindings.bind_expr(Symbol::new("s"), Expr::String("hello".to_owned()));
    bindings.bind_value(Symbol::new("n"), cx.factory().bool(true).unwrap());
    bindings.into_env(&mut cx).unwrap();

    assert!(cx.env().get(&Symbol::new("s")).is_some());
    assert!(cx.env().get(&Symbol::new("n")).is_some());
}

#[test]
fn pratt_shape_parses_string_surface_before_matching() {
    let mut cx = cx();
    let shape = PrattShape::new(
        Arc::new(FakePrattParser),
        Arc::new(ExprKindShape::new(ExprKind::Infix)),
    );

    let matched = shape
        .check_expr(&mut cx, &Expr::String("1 + 2 * 3".to_owned()))
        .unwrap();

    assert!(matched.accepted);
}

#[test]
fn shape_crate_has_no_concrete_codec_dependency() {
    let manifest = include_str!("../../Cargo.toml");

    for line in manifest.lines().map(str::trim) {
        assert!(
            !line.starts_with("sim-codec-"),
            "sim-shape must not depend on concrete codec crates: {line}"
        );
    }
}

#[test]
fn one_of_shape_reports_success_when_any_branch_matches() {
    let mut cx = cx();
    let shape = OneOfShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(AnyShape),
    ]);
    let matched = shape
        .check_expr(&mut cx, &Expr::String("fallback".to_owned()))
        .unwrap();
    assert!(matched.accepted);
}

#[test]
fn exact_expr_is_subshape_of_matching_expr_kind() {
    let mut cx = cx();
    let child = ExactExprShape::new(Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: "1".to_owned(),
    }));
    let number_parent = ExprKindShape::new(ExprKind::Number);
    let string_parent = ExprKindShape::new(ExprKind::String);

    assert!(shape_is_subshape_of(&mut cx, &child, &number_parent).unwrap());
    assert!(!shape_is_subshape_of(&mut cx, &child, &string_parent).unwrap());
}

#[test]
fn one_of_shape_is_subshape_only_when_every_branch_is_subshape() {
    let mut cx = cx();
    let number_parent = ExprKindShape::new(ExprKind::Number);
    let number_or_number = OneOfShape::new(vec![
        Arc::new(ExactExprShape::new(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        }))),
        Arc::new(ExactExprShape::new(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2".to_owned(),
        }))),
    ]);
    let number_or_string = OneOfShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    ]);

    assert!(shape_is_subshape_of(&mut cx, &number_or_number, &number_parent).unwrap());
    assert!(!shape_is_subshape_of(&mut cx, &number_or_string, &number_parent).unwrap());
}

#[test]
fn effectful_shape_is_not_subshape_of_inner_or_any() {
    let mut cx = cx();
    let effectful = EffectfulShape::new(Arc::new(AnyShape));

    assert!(!shape_is_subshape_of(&mut cx, &effectful, &AnyShape).unwrap());
}
