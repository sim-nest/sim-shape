use std::sync::Arc;

use sim_kernel::testing::bare_cx as cx;
use sim_kernel::{Expr, Symbol};

use crate::{
    ExprKind, ExprKindShape, FieldShape, FieldSpec, Shape, TableExtraPolicy, TableFieldSpec,
    TableShape,
};

fn point_shape() -> FieldShape {
    FieldShape::new(
        Symbol::new("Point"),
        vec![FieldSpec::required(
            Symbol::new("x"),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        )],
    )
}

fn object_expr(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::Extension {
        tag: Symbol::qualified("expr", "object"),
        payload: Box::new(Expr::Map(entries)),
    }
}

fn bool_field_map() -> Expr {
    Expr::Map(vec![(Expr::Symbol(Symbol::new("x")), Expr::Bool(true))])
}

#[test]
fn field_shape_rejects_duplicate_class_keys() {
    let mut cx = cx();
    let expr = object_expr(vec![
        (
            Expr::Symbol(Symbol::new("class")),
            Expr::Symbol(Symbol::new("Point")),
        ),
        (
            Expr::Symbol(Symbol::new("class")),
            Expr::Symbol(Symbol::new("Point")),
        ),
        (Expr::Symbol(Symbol::new("fields")), bool_field_map()),
    ]);

    let matched = point_shape().check_expr(&mut cx, &expr).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-object: duplicate key class"
    );
}

#[test]
fn field_shape_rejects_duplicate_fields_keys() {
    let mut cx = cx();
    let expr = object_expr(vec![
        (
            Expr::Symbol(Symbol::new("class")),
            Expr::Symbol(Symbol::new("Point")),
        ),
        (Expr::Symbol(Symbol::new("fields")), bool_field_map()),
        (Expr::Symbol(Symbol::new("fields")), bool_field_map()),
    ]);

    let matched = point_shape().check_expr(&mut cx, &expr).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-object: duplicate key fields"
    );
}

#[test]
fn field_shape_rejects_duplicate_object_field_keys() {
    let mut cx = cx();
    let expr = object_expr(vec![
        (
            Expr::Symbol(Symbol::new("class")),
            Expr::Symbol(Symbol::new("Point")),
        ),
        (
            Expr::Symbol(Symbol::new("fields")),
            Expr::Map(vec![
                (Expr::Symbol(Symbol::new("x")), Expr::Bool(true)),
                (
                    Expr::Symbol(Symbol::new("x")),
                    Expr::String("not-a-bool".to_owned()),
                ),
            ]),
        ),
    ]);

    let matched = point_shape().check_expr(&mut cx, &expr).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-object fields: duplicate key x"
    );
}

#[test]
fn anonymous_field_shape_rejects_duplicate_map_keys() {
    let mut cx = cx();
    let expr = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("x")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("x")),
            Expr::String("not-a-bool".to_owned()),
        ),
    ]);
    let shape = FieldShape::anonymous(vec![FieldSpec::required(
        Symbol::new("x"),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    )]);

    let matched = shape.check_expr(&mut cx, &expr).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-fields: duplicate key x"
    );
}

#[test]
fn table_shape_rejects_duplicate_required_map_keys() {
    let mut cx = cx();
    let shape = TableShape::single(
        Symbol::new("n"),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );
    let expr = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("n")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("n")),
            Expr::String("not-a-bool".to_owned()),
        ),
    ]);

    let matched = shape.check_expr(&mut cx, &expr).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-table: duplicate key n"
    );
}

#[test]
fn table_shape_rejects_duplicate_optional_table_keys() {
    let mut cx = cx();
    let shape = TableShape::new(
        vec![TableFieldSpec {
            key: Symbol::new("opt"),
            shape: Arc::new(ExprKindShape::new(ExprKind::Bool)),
            required: false,
        }],
        TableExtraPolicy::Allow,
    );
    let table = cx
        .factory()
        .table(vec![
            (Symbol::new("opt"), cx.factory().bool(true).unwrap()),
            (
                Symbol::new("opt"),
                cx.factory().string("not-a-bool".to_owned()).unwrap(),
            ),
        ])
        .unwrap();

    let matched = shape.check_value(&mut cx, table).unwrap();

    assert!(!matched.accepted);
    assert_eq!(
        matched.diagnostics[0].message,
        "shape-table: duplicate key opt"
    );
}
