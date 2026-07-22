use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Result, Symbol, shape_is_subshape_of};

use crate::{AnyShape, EffectfulShape, Shape};

use super::{
    CaptureShape, FieldShape, FieldSpec, ListShape, OneOfShape, PrattShape, ShapeExprParser,
};

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn effectful_child() -> Arc<dyn Shape> {
    Arc::new(EffectfulShape::new(Arc::new(AnyShape)))
}

struct PureParser;

impl ShapeExprParser for PureParser {
    fn label(&self) -> &str {
        "pure"
    }

    fn parse_expr(&self, _source: &str) -> Result<Expr> {
        Ok(Expr::Bool(true))
    }
}

struct EffectfulParser;

impl ShapeExprParser for EffectfulParser {
    fn label(&self) -> &str {
        "effectful"
    }

    fn is_effectful(&self) -> bool {
        true
    }

    fn parse_expr(&self, _source: &str) -> Result<Expr> {
        Ok(Expr::Bool(true))
    }
}

fn assert_effectful_not_subshape_of_any(cx: &mut Cx, shape: &dyn Shape) {
    assert!(shape.is_effectful());
    assert!(!shape_is_subshape_of(cx, shape, &AnyShape).unwrap());
}

#[test]
fn list_shape_propagates_effectful_items_and_rest() {
    let mut cx = cx();
    let effectful_item_list = ListShape::new(vec![effectful_child()]);
    let effectful_rest_list = ListShape::with_rest(Vec::new(), effectful_child());

    assert_effectful_not_subshape_of_any(&mut cx, &effectful_item_list);
    assert_effectful_not_subshape_of_any(&mut cx, &effectful_rest_list);
}

#[test]
fn capture_shape_propagates_effectful_inner() {
    let mut cx = cx();
    let shape = CaptureShape::new(Symbol::new("captured"), effectful_child());

    assert_effectful_not_subshape_of_any(&mut cx, &shape);
}

#[test]
fn one_of_shape_propagates_effectful_choice() {
    let mut cx = cx();
    let shape = OneOfShape::new(vec![Arc::new(AnyShape), effectful_child()]);

    assert_effectful_not_subshape_of_any(&mut cx, &shape);
}

#[test]
fn pratt_shape_propagates_effectful_inner() {
    let mut cx = cx();
    let shape = PrattShape::new(Arc::new(PureParser), effectful_child());

    assert_effectful_not_subshape_of_any(&mut cx, &shape);
}

#[test]
fn pratt_shape_is_effectful_when_parser_is_effectful() {
    let mut cx = cx();
    let shape = PrattShape::new(Arc::new(EffectfulParser), Arc::new(AnyShape));

    assert_effectful_not_subshape_of_any(&mut cx, &shape);
}

#[test]
fn field_shape_propagates_effectful_fields() {
    let mut cx = cx();
    let shape = FieldShape::anonymous(vec![FieldSpec::required(
        Symbol::new("x"),
        effectful_child(),
    )]);

    assert_effectful_not_subshape_of_any(&mut cx, &shape);
}
