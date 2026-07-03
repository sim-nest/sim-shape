use std::sync::Arc;

use sim_citizen::{CitizenLib, value_from_expr};
use sim_kernel::{
    CapabilitySet, Cx, DefaultFactory, Error, Expr, NoopEvalPolicy, ObjectEncoding, Symbol, Value,
    read_construct_capability,
};

use crate::{
    AcceptOnNoDiagnosticsHook, AndShape, AnyShape, ClassShape, DiscardOnDiagnosticPrefixHook,
    ExactExprShape, ExprKind, ExprKindShape, HookedShape, ListShape, NotShape, OrShape,
    RepeatShape, ScoreFloorHook, TableExtraPolicy, TableFieldSpec, TableShape, TraceMarkHook,
    VennShapeSet, accept_on_no_diagnostics_hook_class_symbol, and_shape_class_symbol,
    any_shape_class_symbol, class_shape_class_symbol,
    discard_on_diagnostic_prefix_hook_class_symbol, exact_expr_shape_class_symbol,
    expr_kind_shape_class_symbol, hook_value, hooked_shape_class_symbol, list_shape_class_symbol,
    not_shape_class_symbol, or_shape_class_symbol, repeat_shape_class_symbol,
    score_floor_hook_class_symbol, table_shape_class_symbol, trace_mark_hook_class_symbol,
    venn_shape_set_class_symbol,
};

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.load_lib(&CitizenLib::all()).unwrap();
    cx
}

fn constructor(value: &Value, cx: &mut Cx) -> (Symbol, Vec<Expr>) {
    let encoding = value
        .object()
        .as_object_encoder()
        .expect("shape citizen must expose object encoder")
        .object_encoding(cx)
        .unwrap();
    let ObjectEncoding::Constructor { class, args } = encoding else {
        panic!("shape citizen must use constructor encoding");
    };
    (class, args)
}

fn read_construct_value(cx: &mut Cx, class: Symbol, args: &[Expr]) -> Value {
    let values = args
        .iter()
        .map(|arg| value_from_expr(cx, arg))
        .collect::<sim_kernel::Result<Vec<_>>>()
        .unwrap();
    cx.grant(read_construct_capability());
    cx.read_construct(&class, values).unwrap()
}

fn shape_accepts_expr(value: &Value, cx: &mut Cx, expr: Expr) -> bool {
    value
        .object()
        .as_shape()
        .expect("value must remain a callable shape")
        .check_expr(cx, &expr)
        .unwrap()
        .accepted
}

fn shape_fixtures() -> Vec<(Symbol, Value, Expr)> {
    vec![
        (
            any_shape_class_symbol(),
            crate::shape_value_with_encoding(
                any_shape_class_symbol(),
                Arc::new(AnyShape),
                ObjectEncoding::Constructor {
                    class: any_shape_class_symbol(),
                    args: vec![Expr::Symbol(Symbol::new("v1"))],
                },
            ),
            Expr::String("anything".to_owned()),
        ),
        (
            exact_expr_shape_class_symbol(),
            crate::shape_value_with_encoding(
                exact_expr_shape_class_symbol(),
                Arc::new(ExactExprShape::new(Expr::Bool(true))),
                ObjectEncoding::Constructor {
                    class: exact_expr_shape_class_symbol(),
                    args: vec![Expr::Symbol(Symbol::new("v1")), Expr::Bool(true)],
                },
            ),
            Expr::Bool(true),
        ),
        (
            expr_kind_shape_class_symbol(),
            crate::shape_value_with_encoding(
                expr_kind_shape_class_symbol(),
                Arc::new(ExprKindShape::new(ExprKind::Number)),
                ObjectEncoding::Constructor {
                    class: expr_kind_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::Symbol(Symbol::new("number")),
                    ],
                },
            ),
            Expr::Number(sim_kernel::NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            }),
        ),
        (
            class_shape_class_symbol(),
            crate::shape_value_with_encoding(
                class_shape_class_symbol(),
                Arc::new(ClassShape::new(any_shape_class_symbol())),
                ObjectEncoding::Constructor {
                    class: class_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::Symbol(any_shape_class_symbol()),
                    ],
                },
            ),
            Expr::Symbol(any_shape_class_symbol()),
        ),
        (
            list_shape_class_symbol(),
            crate::shape_value_with_encoding(
                list_shape_class_symbol(),
                Arc::new(ListShape::new(vec![Arc::new(AnyShape)])),
                ObjectEncoding::Constructor {
                    class: list_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::List(vec![Expr::Call {
                            operator: Box::new(Expr::Symbol(any_shape_class_symbol())),
                            args: vec![Expr::Symbol(Symbol::new("v1"))],
                        }]),
                        Expr::Nil,
                    ],
                },
            ),
            Expr::List(vec![Expr::Bool(true)]),
        ),
        (
            table_shape_class_symbol(),
            crate::shape_value_with_encoding(
                table_shape_class_symbol(),
                Arc::new(TableShape::new(
                    vec![TableFieldSpec {
                        key: Symbol::new("ok"),
                        required: true,
                        shape: Arc::new(AnyShape),
                    }],
                    TableExtraPolicy::Reject,
                )),
                ObjectEncoding::Constructor {
                    class: table_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::List(vec![Expr::List(vec![
                            Expr::Symbol(Symbol::new("ok")),
                            Expr::Bool(true),
                            Expr::Call {
                                operator: Box::new(Expr::Symbol(any_shape_class_symbol())),
                                args: vec![Expr::Symbol(Symbol::new("v1"))],
                            },
                        ])]),
                        Expr::Symbol(Symbol::new("reject")),
                    ],
                },
            ),
            Expr::Map(vec![(Expr::Symbol(Symbol::new("ok")), Expr::Bool(true))]),
        ),
        (
            or_shape_class_symbol(),
            crate::shape_value_with_encoding(
                or_shape_class_symbol(),
                Arc::new(OrShape::new(vec![
                    Arc::new(ExactExprShape::new(Expr::Bool(false))),
                    Arc::new(ExactExprShape::new(Expr::Bool(true))),
                ])),
                ObjectEncoding::Constructor {
                    class: or_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::List(vec![
                            Expr::Call {
                                operator: Box::new(Expr::Symbol(exact_expr_shape_class_symbol())),
                                args: vec![Expr::Symbol(Symbol::new("v1")), Expr::Bool(false)],
                            },
                            Expr::Call {
                                operator: Box::new(Expr::Symbol(exact_expr_shape_class_symbol())),
                                args: vec![Expr::Symbol(Symbol::new("v1")), Expr::Bool(true)],
                            },
                        ]),
                        Expr::Symbol(Symbol::new("first-match")),
                    ],
                },
            ),
            Expr::Bool(true),
        ),
        (
            and_shape_class_symbol(),
            crate::shape_value_with_encoding(
                and_shape_class_symbol(),
                Arc::new(AndShape::new(vec![
                    Arc::new(AnyShape),
                    Arc::new(ExactExprShape::new(Expr::Bool(true))),
                ])),
                ObjectEncoding::Constructor {
                    class: and_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::List(vec![
                            Expr::Call {
                                operator: Box::new(Expr::Symbol(any_shape_class_symbol())),
                                args: vec![Expr::Symbol(Symbol::new("v1"))],
                            },
                            Expr::Call {
                                operator: Box::new(Expr::Symbol(exact_expr_shape_class_symbol())),
                                args: vec![Expr::Symbol(Symbol::new("v1")), Expr::Bool(true)],
                            },
                        ]),
                    ],
                },
            ),
            Expr::Bool(true),
        ),
        (
            not_shape_class_symbol(),
            crate::shape_value_with_encoding(
                not_shape_class_symbol(),
                Arc::new(NotShape::new(Arc::new(ExactExprShape::new(Expr::Bool(
                    false,
                ))))),
                ObjectEncoding::Constructor {
                    class: not_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::Call {
                            operator: Box::new(Expr::Symbol(exact_expr_shape_class_symbol())),
                            args: vec![Expr::Symbol(Symbol::new("v1")), Expr::Bool(false)],
                        },
                    ],
                },
            ),
            Expr::Bool(true),
        ),
        (
            repeat_shape_class_symbol(),
            crate::shape_value_with_encoding(
                repeat_shape_class_symbol(),
                Arc::new(RepeatShape::with_bounds(Arc::new(AnyShape), 1, Some(2))),
                ObjectEncoding::Constructor {
                    class: repeat_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::Call {
                            operator: Box::new(Expr::Symbol(any_shape_class_symbol())),
                            args: vec![Expr::Symbol(Symbol::new("v1"))],
                        },
                        Expr::Number(sim_kernel::NumberLiteral {
                            domain: Symbol::qualified("citizen", "int"),
                            canonical: "1".to_owned(),
                        }),
                        Expr::Number(sim_kernel::NumberLiteral {
                            domain: Symbol::qualified("citizen", "int"),
                            canonical: "2".to_owned(),
                        }),
                    ],
                },
            ),
            Expr::Vector(vec![Expr::Bool(true)]),
        ),
        (
            hooked_shape_class_symbol(),
            crate::shape_value_with_encoding(
                hooked_shape_class_symbol(),
                Arc::new(HookedShape::new(
                    Arc::new(AnyShape),
                    vec![Arc::new(TraceMarkHook)],
                )),
                ObjectEncoding::Constructor {
                    class: hooked_shape_class_symbol(),
                    args: vec![
                        Expr::Symbol(Symbol::new("v1")),
                        Expr::Call {
                            operator: Box::new(Expr::Symbol(any_shape_class_symbol())),
                            args: vec![Expr::Symbol(Symbol::new("v1"))],
                        },
                        Expr::List(vec![Expr::Call {
                            operator: Box::new(Expr::Symbol(trace_mark_hook_class_symbol())),
                            args: vec![Expr::Symbol(Symbol::new("v1"))],
                        }]),
                    ],
                },
            ),
            Expr::String("hooked".to_owned()),
        ),
    ]
}

#[test]
fn built_in_shape_citizens_use_shape_namespace_and_v1() {
    let mut cx = cx();
    for (expected, value, _) in shape_fixtures() {
        let (class, args) = constructor(&value, &mut cx);
        assert_eq!(class, expected);
        assert_eq!(args.first(), Some(&Expr::Symbol(Symbol::new("v1"))));
    }
}

#[test]
fn shape_citizens_preserve_match_behavior_after_roundtrip() {
    let mut cx = cx();
    for (_expected, value, accepted_expr) in shape_fixtures() {
        let (class, args) = constructor(&value, &mut cx);
        let decoded = read_construct_value(&mut cx, class, &args);
        assert!(decoded.object().as_callable().is_some());
        assert!(shape_accepts_expr(&value, &mut cx, accepted_expr.clone()));
        assert!(shape_accepts_expr(&decoded, &mut cx, accepted_expr));
    }
}

#[test]
fn hook_and_venn_citizens_use_constructor_forms() {
    let mut cx = cx();
    let hooks = vec![
        (
            trace_mark_hook_class_symbol(),
            hook_value(Arc::new(TraceMarkHook)),
        ),
        (
            score_floor_hook_class_symbol(),
            hook_value(Arc::new(ScoreFloorHook::new(10))),
        ),
        (
            accept_on_no_diagnostics_hook_class_symbol(),
            hook_value(Arc::new(AcceptOnNoDiagnosticsHook)),
        ),
        (
            discard_on_diagnostic_prefix_hook_class_symbol(),
            hook_value(Arc::new(DiscardOnDiagnosticPrefixHook::new("shape:"))),
        ),
    ];
    for (expected, value) in hooks {
        let (class, args) = constructor(&value, &mut cx);
        assert_eq!(class, expected);
        assert_eq!(args.first(), Some(&Expr::Symbol(Symbol::new("v1"))));
    }

    let venn = cx
        .factory()
        .opaque(Arc::new(VennShapeSet::new(vec![(
            Symbol::new("any"),
            Arc::new(AnyShape),
        )])))
        .unwrap();
    let (class, args) = constructor(&venn, &mut cx);
    assert_eq!(class, venn_shape_set_class_symbol());
    assert_eq!(args.first(), Some(&Expr::Symbol(Symbol::new("v1"))));
}

#[test]
fn shape_read_construct_is_capability_gated() {
    let mut cx = cx();
    let version = cx.factory().symbol(Symbol::new("v1")).unwrap();
    let err = cx
        .with_capabilities(CapabilitySet::default(), |cx| {
            cx.read_construct(&any_shape_class_symbol(), vec![version])
        })
        .expect_err("shape read-construct must require read-construct capability");
    assert!(
        matches!(err, Error::CapabilityDenied { capability } if capability == read_construct_capability())
    );
}

#[test]
fn shape_citizens_pass_universal_conformance() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    sim_citizen::run_registered_conformance(&mut cx).unwrap();
}
