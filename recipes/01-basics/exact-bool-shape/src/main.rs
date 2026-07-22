use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Symbol};
use sim_shape::{ExactExprShape, Shape, check_shape_on_expr, parse_shape_expr};

fn main() -> sim_kernel::Result<()> {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    let shape = ExactExprShape::new(Expr::Bool(true));

    let accepted = shape.check_expr(&mut cx, &Expr::Bool(true))?.accepted;
    let rejected = shape.check_expr(&mut cx, &Expr::Bool(false))?.accepted;

    assert!(accepted);
    assert!(!rejected);

    let parsed = parse_shape_expr(&Expr::List(vec![
        Expr::Symbol(Symbol::new("capture")),
        Expr::Symbol(Symbol::new("seen")),
        Expr::Symbol(Symbol::new("Bool")),
    ]))?;
    let captured = check_shape_on_expr(parsed.as_ref(), &mut cx, &Expr::Bool(true))?;
    assert!(captured.accepted);
    assert_eq!(captured.captures.exprs().len(), 1);
    assert_eq!(captured.captures.exprs()[0].0.name.as_ref(), "seen");
    assert_eq!(captured.captures.exprs()[0].1, Expr::Bool(true));

    println!("true accepted: {accepted}");
    println!("false accepted: {rejected}");
    println!(
        "captured {}: {:?}",
        captured.captures.exprs()[0].0,
        captured.captures.exprs()[0].1
    );
    Ok(())
}
