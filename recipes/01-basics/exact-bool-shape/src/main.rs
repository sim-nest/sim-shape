use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
use sim_shape::{ExactExprShape, Shape};

fn main() -> sim_kernel::Result<()> {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    let shape = ExactExprShape::new(Expr::Bool(true));

    let accepted = shape.check_expr(&mut cx, &Expr::Bool(true))?.accepted;
    let rejected = shape.check_expr(&mut cx, &Expr::Bool(false))?.accepted;

    assert!(accepted);
    assert!(!rejected);

    println!("true accepted: {accepted}");
    println!("false accepted: {rejected}");
    Ok(())
}
