use std::sync::Arc;

use sim_kernel::{Expr, NumberLiteral, Symbol};

use crate::{AnyShape, CaptureShape, ExactExprShape, ExprKind, ExprKindShape, ListShape, Shape};

use super::{EndlessNumberList, cx};

#[test]
fn list_shape_collects_captures() {
    let mut cx = cx();
    let shape = ListShape::new(vec![
        Arc::new(ExactExprShape::new(Expr::Symbol(Symbol::new("+")))),
        Arc::new(CaptureShape::new(
            Symbol::new("x"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )),
        Arc::new(CaptureShape::new(
            Symbol::new("y"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )),
    ]);

    let expr = Expr::List(vec![
        Expr::Symbol(Symbol::new("+")),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1.0".to_owned(),
        }),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2.0".to_owned(),
        }),
    ]);

    let matched = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 2);
}

#[test]
fn list_shape_with_rest_accepts_endless_value_list() {
    let mut cx = cx();
    // A total, non-binding rest accepts an unbounded tail without walking it
    // forever: the value path stops once a total rest binds nothing.
    let shape = ListShape::with_rest(
        vec![Arc::new(AnyShape), Arc::new(AnyShape)],
        Arc::new(AnyShape),
    );
    let endless = cx.factory().opaque(Arc::new(EndlessNumberList)).unwrap();
    let matched = shape.check_value(&mut cx, endless).unwrap();
    assert!(matched.accepted);
}

#[test]
fn list_shape_rest_captures_identically_for_value_and_expr() {
    let mut cx = cx();
    let shape = ListShape::with_rest(
        Vec::new(),
        Arc::new(CaptureShape::new(Symbol::new("rest"), Arc::new(AnyShape))),
    );

    let elements = vec![Expr::Bool(true), Expr::Bool(false), Expr::Bool(true)];
    let expr = Expr::List(elements.clone());
    let expr_match = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(expr_match.accepted);
    assert_eq!(expr_match.captures.exprs().len(), 3);

    let values = elements
        .iter()
        .map(|element| match element {
            Expr::Bool(flag) => cx.factory().bool(*flag).unwrap(),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    let list_value = cx.factory().list(values).unwrap();
    let value_match = shape.check_value(&mut cx, list_value).unwrap();
    assert!(value_match.accepted);

    // The value path captures the trailing rest identically to the expr path
    // instead of early-returning and binding nothing.
    assert_eq!(value_match.captures.values().len(), 3);
    assert_eq!(
        value_match.captures.values().len(),
        expr_match.captures.exprs().len()
    );
}
