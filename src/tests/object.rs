use std::sync::Arc;

use sim_kernel::{ClassId, Expr, Symbol};

use crate::{ClassShape, ExprKind, ExprKindShape, FieldShape, FieldSpec, ObjectExpr, Shape};

use super::cx;

#[test]
fn class_shape_checks_runtime_class() {
    let mut cx = cx();
    let value = cx
        .factory()
        .class_stub(ClassId(77), Symbol::new("Point"))
        .unwrap();
    let shape = ClassShape::new(Symbol::new("Point"));
    assert!(shape.check_value(&mut cx, value).unwrap().accepted);
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::Map(Vec::new()))
            .unwrap()
            .accepted
    );
}

#[test]
fn object_expr_roundtrips() {
    let expr = ObjectExpr {
        class: Symbol::new("Point"),
        fields: vec![(Symbol::new("x"), Expr::Bool(true))],
    }
    .to_expr();
    let parsed = ObjectExpr::parse(&expr).unwrap();
    assert_eq!(parsed.class, Symbol::new("Point"));
    assert_eq!(parsed.field(&Symbol::new("x")), Some(&Expr::Bool(true)));
}

#[test]
fn field_shape_checks_required_fields() {
    let mut cx = cx();
    let shape = FieldShape::new(
        Symbol::new("Point"),
        vec![FieldSpec::required(
            Symbol::new("x"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )],
    );
    let expr = ObjectExpr {
        class: Symbol::new("Point"),
        fields: vec![(Symbol::new("y"), Expr::Bool(true))],
    }
    .to_expr();
    let matched = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(!matched.accepted);
}
