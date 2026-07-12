use std::sync::Arc;

use sim_kernel::{Cx, Expr, NumberLiteral, Symbol};

use crate::{
    AndShape, AnyShape, CaptureShape, ExactExprShape, ExprKind, ExprKindShape, NotShape,
    OptionFieldSpec, OrShape, OrStrategy, RepeatShape, Shape, TableExtraPolicy, TableFieldSpec,
    TableShape, check_option_map,
};

use sim_kernel::testing::bare_cx as cx;

fn number_expr(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: text.to_owned(),
    })
}

fn number_value(cx: &mut Cx, text: &str) -> sim_kernel::Value {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "f64"), text.to_owned())
        .unwrap()
}

#[test]
fn and_shape_accepts_when_all_children_accept() {
    let mut cx = cx();
    let shape = AndShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ExactExprShape::new(number_expr("1"))),
    ]);

    let matched = shape.check_expr(&mut cx, &number_expr("1")).unwrap();

    assert!(matched.accepted);
}

#[test]
fn and_shape_rejects_on_first_rejecting_child() {
    let mut cx = cx();
    let shape = AndShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ExprKindShape::new(ExprKind::String)),
        Arc::new(AnyShape),
    ]);

    let matched = shape.check_expr(&mut cx, &number_expr("1")).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-and:"));
}

#[test]
fn and_shape_merges_captures_from_accepted_children() {
    let mut cx = cx();
    let shape = AndShape::new(vec![
        Arc::new(CaptureShape::new(Symbol::new("a"), Arc::new(AnyShape))),
        Arc::new(CaptureShape::new(Symbol::new("b"), Arc::new(AnyShape))),
    ]);

    let matched = shape
        .check_expr(&mut cx, &Expr::String("ok".to_owned()))
        .unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 2);
}

#[test]
fn empty_and_shape_accepts() {
    let mut cx = cx();
    let shape = AndShape::new(Vec::new());

    let matched = shape
        .check_expr(&mut cx, &Expr::String("anything".to_owned()))
        .unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 0);
}

#[test]
fn or_shape_returns_leftmost_branch_by_default() {
    let mut cx = cx();
    let shape = OrShape::new(vec![
        Arc::new(AnyShape),
        Arc::new(ExactExprShape::new(number_expr("1"))),
    ]);

    let matched = shape.check_expr(&mut cx, &number_expr("1")).unwrap();

    assert!(matched.accepted);
    assert_eq!(
        matched.captures.exprs()[0],
        (
            Symbol::qualified("shape", "branch-index"),
            crate::algebra::number_expr(0)
        )
    );
}

#[test]
fn or_shape_can_pick_best_score() {
    let mut cx = cx();
    let shape = OrShape::with_strategy(
        vec![
            Arc::new(ExprKindShape::new(ExprKind::Number)),
            Arc::new(ExactExprShape::new(number_expr("1"))),
        ],
        OrStrategy::BestScore,
    );

    let matched = shape.check_expr(&mut cx, &number_expr("1")).unwrap();

    assert!(matched.accepted);
    assert_eq!(
        matched.captures.exprs()[0],
        (
            Symbol::qualified("shape", "branch-index"),
            crate::algebra::number_expr(1)
        )
    );
}

#[test]
fn or_shape_rejects_with_collected_diagnostics() {
    let mut cx = cx();
    let shape = OrShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    ]);

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-or:"));
    assert!(matched.diagnostics.len() >= 3);
}

#[test]
fn empty_or_shape_rejects() {
    let mut cx = cx();
    let shape = OrShape::new(Vec::new());

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-or:"));
}

#[test]
fn not_shape_accepts_when_inner_rejects() {
    let mut cx = cx();
    let shape = NotShape::new(Arc::new(ExprKindShape::new(ExprKind::Number)));

    let matched = shape
        .check_expr(&mut cx, &Expr::String("ok".to_owned()))
        .unwrap();

    assert!(matched.accepted);
    assert_eq!(
        matched.captures.exprs()[0],
        (Symbol::qualified("shape", "negated"), Expr::Bool(true))
    );
}

#[test]
fn not_shape_rejects_when_inner_accepts() {
    let mut cx = cx();
    let shape = NotShape::new(Arc::new(AnyShape));

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-not:"));
}

#[test]
fn not_shape_does_not_leak_inner_captures() {
    let mut cx = cx();
    let shape = NotShape::new(Arc::new(CaptureShape::new(
        Symbol::new("inner"),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
    )));

    let matched = shape
        .check_expr(&mut cx, &Expr::String("ok".to_owned()))
        .unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 1);
    assert_eq!(
        matched.captures.exprs()[0].0,
        Symbol::qualified("shape", "negated")
    );
}

#[test]
fn table_shape_accepts_required_keys() {
    let mut cx = cx();
    let shape = TableShape::single(
        Symbol::new("n"),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
    );
    let value = number_value(&mut cx, "1");
    let table = cx.factory().table(vec![(Symbol::new("n"), value)]).unwrap();

    let matched = shape.check_value(&mut cx, table).unwrap();

    assert!(matched.accepted);
}

#[test]
fn table_shape_rejects_missing_required_keys() {
    let mut cx = cx();
    let shape = TableShape::single(Symbol::new("n"), Arc::new(AnyShape));
    let table = cx.factory().table(Vec::new()).unwrap();

    let matched = shape.check_value(&mut cx, table).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-table:"));
}

#[test]
fn table_shape_rejects_extra_keys_under_closed_policy() {
    let mut cx = cx();
    let shape = TableShape::new(
        vec![TableFieldSpec {
            key: Symbol::new("n"),
            shape: Arc::new(AnyShape),
            required: true,
        }],
        TableExtraPolicy::Reject,
    );
    let required = number_value(&mut cx, "1");
    let extra = number_value(&mut cx, "2");
    let table = cx
        .factory()
        .table(vec![
            (Symbol::new("n"), required),
            (Symbol::new("extra"), extra),
        ])
        .unwrap();

    let matched = shape.check_value(&mut cx, table).unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-table:"));
}

#[test]
fn table_shape_checks_extra_values_under_shape_policy() {
    let mut cx = cx();
    let shape = TableShape::new(
        vec![TableFieldSpec {
            key: Symbol::new("n"),
            shape: Arc::new(AnyShape),
            required: true,
        }],
        TableExtraPolicy::Shape(Arc::new(ExprKindShape::new(ExprKind::Number))),
    );
    let required = number_value(&mut cx, "1");
    let extra = number_value(&mut cx, "2");
    let table = cx
        .factory()
        .table(vec![
            (Symbol::new("n"), required),
            (Symbol::new("extra"), extra),
        ])
        .unwrap();

    let matched = shape.check_value(&mut cx, table).unwrap();

    assert!(matched.accepted);
}

#[test]
fn option_map_check_reuses_table_shape_contract() {
    let mut cx = cx();
    let expr = Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("model")),
            Expr::String("local".to_owned()),
        ),
        (Expr::Symbol(Symbol::new("trace")), Expr::Bool(true)),
    ]);

    let matched = check_option_map(
        &mut cx,
        &expr,
        vec![
            OptionFieldSpec::required(
                Symbol::new("model"),
                Arc::new(ExprKindShape::new(ExprKind::String)),
            ),
            OptionFieldSpec::optional(
                Symbol::new("trace"),
                Arc::new(ExprKindShape::new(ExprKind::Bool)),
            ),
        ],
        TableExtraPolicy::Reject,
    )
    .unwrap();

    assert!(matched.accepted);
}

#[test]
fn option_map_check_reports_missing_and_extra_keys() {
    let mut cx = cx();
    let missing = check_option_map(
        &mut cx,
        &Expr::Map(Vec::new()),
        vec![OptionFieldSpec::required(
            Symbol::new("model"),
            Arc::new(AnyShape),
        )],
        TableExtraPolicy::Allow,
    )
    .unwrap();
    assert!(!missing.accepted);
    assert_eq!(missing.diagnostics[0].message, "shape-table: missing keys");

    let extra = check_option_map(
        &mut cx,
        &Expr::Map(vec![(
            Expr::Symbol(Symbol::new("unknown")),
            Expr::String("value".to_owned()),
        )]),
        Vec::new(),
        TableExtraPolicy::Reject,
    )
    .unwrap();
    assert!(!extra.accepted);
    assert_eq!(
        extra.diagnostics[0].message,
        "shape-table: extra key unknown"
    );
}

#[test]
fn repeat_shape_accepts_zero_items_when_min_is_zero() {
    let mut cx = cx();
    let shape = RepeatShape::new(Arc::new(AnyShape));

    let matched = shape.check_expr(&mut cx, &Expr::List(Vec::new())).unwrap();

    assert!(matched.accepted);
}

#[test]
fn repeat_shape_rejects_too_few_items() {
    let mut cx = cx();
    let shape = RepeatShape::with_bounds(Arc::new(AnyShape), 2, None);

    let matched = shape
        .check_expr(&mut cx, &Expr::List(vec![Expr::Bool(true)]))
        .unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-repeat:"));
}

#[test]
fn repeat_shape_rejects_too_many_items() {
    let mut cx = cx();
    let shape = RepeatShape::with_bounds(Arc::new(AnyShape), 0, Some(1));

    let matched = shape
        .check_expr(
            &mut cx,
            &Expr::List(vec![Expr::Bool(true), Expr::Bool(false)]),
        )
        .unwrap();

    assert!(!matched.accepted);
    assert!(matched.diagnostics[0].message.starts_with("shape-repeat:"));
}

#[test]
fn repeat_shape_binds_repeat_count() {
    let mut cx = cx();
    let shape = RepeatShape::new(Arc::new(AnyShape));

    let matched = shape
        .check_expr(
            &mut cx,
            &Expr::Vector(vec![Expr::Bool(true), Expr::Bool(false)]),
        )
        .unwrap();

    assert!(matched.accepted);
    assert_eq!(
        matched.captures.exprs()[0],
        (
            Symbol::qualified("shape", "repeat-count"),
            crate::algebra::number_expr(2)
        )
    );
}
