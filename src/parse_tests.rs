use std::sync::Arc;

use sim_kernel::{DefaultFactory, Expr, NoopEvalPolicy, NumberLiteral, Symbol};

use crate::{Shape, parse_shape_expr};

fn cx() -> sim_kernel::Cx {
    sim_kernel::Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn number_expr(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: text.to_owned(),
    })
}

fn symbol_expr(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn shape_form(head: &str, args: Vec<Expr>) -> Expr {
    let mut items = vec![symbol_expr(head)];
    items.extend(args);
    Expr::List(items)
}

fn qualified_shape_form(head: &str, args: Vec<Expr>) -> Expr {
    let mut items = vec![Expr::Symbol(Symbol::qualified("shape", head))];
    items.extend(args);
    Expr::List(items)
}

fn parsed_shape(expr: Expr) -> Arc<dyn Shape> {
    parse_shape_expr(&expr).unwrap()
}

#[test]
fn parse_shape_expr_builds_and_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form(
        "and",
        vec![symbol_expr("Any"), symbol_expr("Number")],
    ));

    assert!(
        shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::String("not a number".to_owned()))
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_or_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form(
        "or",
        vec![symbol_expr("Number"), symbol_expr("String")],
    ));

    assert!(
        shape
            .check_expr(&mut cx, &Expr::String("accepted".to_owned()))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::Bool(true))
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_qualified_sdk_sequence_aliases() {
    let wrapped_number_and_any = Expr::List(vec![symbol_expr("Any"), symbol_expr("Number")]);
    let all_shape = parsed_shape(qualified_shape_form("all", vec![wrapped_number_and_any]));
    let mut cx = cx();
    assert!(
        all_shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        !all_shape
            .check_expr(&mut cx, &Expr::String("rejected".to_owned()))
            .unwrap()
            .accepted
    );

    let wrapped_number_or_string = Expr::List(vec![symbol_expr("Number"), symbol_expr("String")]);
    let any_shape = parsed_shape(qualified_shape_form("any", vec![wrapped_number_or_string]));
    assert!(
        any_shape
            .check_expr(&mut cx, &Expr::String("accepted".to_owned()))
            .unwrap()
            .accepted
    );
    assert!(
        !any_shape
            .check_expr(&mut cx, &Expr::Bool(true))
            .unwrap()
            .accepted
    );

    let none_shape = parsed_shape(qualified_shape_form("none", vec![symbol_expr("Number")]));
    assert!(
        none_shape
            .check_expr(&mut cx, &Expr::String("accepted".to_owned()))
            .unwrap()
            .accepted
    );
    assert!(
        !none_shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_not_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form("not", vec![symbol_expr("Number")]));

    assert!(
        shape
            .check_expr(&mut cx, &Expr::String("accepted".to_owned()))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_qualified_sdk_list_shape() {
    let mut cx = cx();
    let shape = parsed_shape(qualified_shape_form(
        "list",
        vec![Expr::List(vec![
            symbol_expr("String"),
            symbol_expr("Number"),
        ])],
    ));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::List(vec![Expr::String("head".to_owned()), number_expr("1")]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::List(vec![Expr::String("head".to_owned()), Expr::Bool(false)]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_list_rest_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form(
        "list-rest",
        vec![
            Expr::List(vec![symbol_expr("String")]),
            symbol_expr("Number"),
        ],
    ));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::List(vec![
                    Expr::String("head".to_owned()),
                    number_expr("1"),
                    number_expr("2"),
                ]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::List(vec![Expr::String("head".to_owned()), Expr::Bool(false)]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_repeat_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form("repeat", vec![symbol_expr("Number")]));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::Vector(vec![number_expr("1"), number_expr("2")])
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::Vector(vec![number_expr("1"), Expr::String("bad".to_owned())]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_repeat_bounds_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form(
        "repeat-bounds",
        vec![symbol_expr("Number"), number_expr("1"), number_expr("2")],
    ));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::Vector(vec![number_expr("1"), number_expr("2")])
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::Vector(vec![number_expr("1"), number_expr("2"), number_expr("3")]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_table_and_table_required_shapes() {
    let mut cx = cx();
    let table = parsed_shape(qualified_shape_form(
        "table",
        vec![symbol_expr("n"), symbol_expr("Number")],
    ));

    assert!(
        table
            .check_expr(
                &mut cx,
                &Expr::Map(vec![
                    (symbol_expr("n"), number_expr("1")),
                    (symbol_expr("extra"), Expr::Bool(true)),
                ]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !table
            .check_expr(
                &mut cx,
                &Expr::Map(vec![(symbol_expr("n"), Expr::Bool(true))]),
            )
            .unwrap()
            .accepted
    );

    let fields = Expr::List(vec![Expr::List(vec![
        symbol_expr("n"),
        symbol_expr("Number"),
    ])]);
    let table_required = parsed_shape(qualified_shape_form("table-required", vec![fields]));
    assert!(
        table_required
            .check_expr(
                &mut cx,
                &Expr::Map(vec![
                    (symbol_expr("n"), number_expr("1")),
                    (symbol_expr("extra"), Expr::Bool(true)),
                ]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !table_required
            .check_expr(
                &mut cx,
                &Expr::Map(vec![(symbol_expr("extra"), Expr::Bool(true))]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_open_table_shape() {
    let mut cx = cx();
    let shape = parsed_shape(shape_form(
        "table-open",
        vec![Expr::List(vec![symbol_expr(":n"), symbol_expr("Number")])],
    ));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::Map(vec![
                    (symbol_expr("n"), number_expr("1")),
                    (symbol_expr("extra"), Expr::Bool(true)),
                ]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::Map(vec![(symbol_expr("extra"), Expr::Bool(true))]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_closed_table_shape_from_wrapped_fields() {
    let mut cx = cx();
    let fields = Expr::List(vec![Expr::List(vec![
        symbol_expr("n"),
        symbol_expr("Number"),
    ])]);
    let shape = parsed_shape(shape_form("table-closed", vec![fields]));

    assert!(
        shape
            .check_expr(
                &mut cx,
                &Expr::Map(vec![(symbol_expr("n"), number_expr("1"))]),
            )
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(
                &mut cx,
                &Expr::Map(vec![
                    (symbol_expr("n"), number_expr("1")),
                    (symbol_expr("extra"), Expr::Bool(true)),
                ]),
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn parse_shape_expr_builds_without_and_difference_shapes() {
    for head in ["without", "difference"] {
        let mut cx = cx();
        let shape = parsed_shape(shape_form(
            head,
            vec![symbol_expr("Any"), symbol_expr("String")],
        ));

        assert!(
            shape
                .check_expr(&mut cx, &Expr::Bool(true))
                .unwrap()
                .accepted
        );
        assert!(
            !shape
                .check_expr(&mut cx, &Expr::String("rejected".to_owned()))
                .unwrap()
                .accepted
        );
    }
}
