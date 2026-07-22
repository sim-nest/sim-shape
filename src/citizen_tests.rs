use std::sync::Arc;

use sim_citizen::{CitizenLib, value_from_expr};
use sim_kernel::{
    CapabilitySet, Cx, DefaultFactory, Error, Expr, NoopEvalPolicy, ObjectEncoding, Shape,
    ShapeDoc, ShapeMatch, Symbol, Value, read_construct_capability,
};

use crate::{
    AcceptOnNoDiagnosticsHook, AndShape, AnyShape, ClassShape, DiscardOnDiagnosticPrefixHook,
    ExactExprShape, ExprKind, ExprKindShape, HookedShape, ListShape, MatchHook, MatchHookContext,
    MatchHookDecision, MatchHookKind, MatchScore, NotShape, OrShape, RepeatShape, ScoreFloorHook,
    TableExtraPolicy, TableFieldSpec, TableShape, TraceMarkHook, VennShapeSet,
    accept_on_no_diagnostics_hook_class_symbol, and_shape_class_symbol, any_shape_class_symbol,
    class_shape_class_symbol, discard_on_diagnostic_prefix_hook_class_symbol,
    exact_expr_shape_class_symbol, expr_kind_shape_class_symbol, hook_value,
    hooked_shape_class_symbol, list_shape_class_symbol, not_shape_class_symbol,
    or_shape_class_symbol, repeat_shape_class_symbol, score_floor_hook_class_symbol,
    table_shape_class_symbol, trace_mark_hook_class_symbol, venn_shape_set_class_symbol,
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

fn decoded_shape_accepts_expr(shape: &dyn Shape, cx: &mut Cx, expr: Expr) -> bool {
    shape.check_expr(cx, &expr).unwrap().accepted
}

fn assert_supported_shape_decodes_from_bare_and_constructor_surfaces<T>(
    shape: Arc<T>,
    accepted_expr: Expr,
) where
    T: Shape + 'static,
{
    let mut cx = cx();
    cx.grant(read_construct_capability());

    let expected = crate::citizen::encode_shape_expr(shape.as_ref()).unwrap();

    let bare_value = cx.factory().opaque(shape.clone()).unwrap();
    let decoded_bare = crate::citizen::decode_shape_value(&mut cx, bare_value, "shape").unwrap();
    assert_eq!(
        crate::citizen::encode_shape_expr(decoded_bare.as_ref()).unwrap(),
        expected
    );
    assert!(decoded_shape_accepts_expr(
        decoded_bare.as_ref(),
        &mut cx,
        accepted_expr.clone()
    ));

    let constructor_value = value_from_expr(&mut cx, &expected).unwrap();
    let decoded_constructor =
        crate::citizen::decode_shape_value(&mut cx, constructor_value, "shape").unwrap();
    assert_eq!(
        crate::citizen::encode_shape_expr(decoded_constructor.as_ref()).unwrap(),
        expected
    );
    assert!(decoded_shape_accepts_expr(
        decoded_constructor.as_ref(),
        &mut cx,
        accepted_expr
    ));
}

struct LiveShape;

impl Shape for LiveShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn describe(&self, _cx: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
        Ok(ShapeDoc::new("live shape"))
    }
}

struct LiveHook;

impl MatchHook for LiveHook {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("shape", "live-hook")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Mark
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        _ctx: &MatchHookContext,
        _current: Option<&ShapeMatch>,
    ) -> sim_kernel::Result<MatchHookDecision> {
        Ok(MatchHookDecision::Pass)
    }
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
fn supported_composite_shape_values_decode_from_bare_and_constructor_surfaces() {
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(TableShape::new(
            vec![TableFieldSpec {
                key: Symbol::new("ok"),
                required: true,
                shape: Arc::new(AnyShape),
            }],
            TableExtraPolicy::Reject,
        )),
        Expr::Map(vec![(Expr::Symbol(Symbol::new("ok")), Expr::Bool(true))]),
    );
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(OrShape::new(vec![
            Arc::new(ExactExprShape::new(Expr::Bool(false))),
            Arc::new(ExactExprShape::new(Expr::Bool(true))),
        ])),
        Expr::Bool(true),
    );
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(AndShape::new(vec![
            Arc::new(AnyShape),
            Arc::new(ExactExprShape::new(Expr::Bool(true))),
        ])),
        Expr::Bool(true),
    );
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(NotShape::new(Arc::new(ExactExprShape::new(Expr::Bool(
            false,
        ))))),
        Expr::Bool(true),
    );
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(RepeatShape::with_bounds(Arc::new(AnyShape), 1, Some(2))),
        Expr::Vector(vec![Expr::Bool(true)]),
    );
    assert_supported_shape_decodes_from_bare_and_constructor_surfaces(
        Arc::new(HookedShape::new(
            Arc::new(AnyShape),
            vec![Arc::new(TraceMarkHook)],
        )),
        Expr::String("hooked".to_owned()),
    );
}

#[test]
fn venn_member_shapes_decode_from_bare_and_constructor_surfaces() {
    let mut cx = cx();
    cx.grant(read_construct_capability());

    let table = Arc::new(TableShape::single(Symbol::new("ok"), Arc::new(AnyShape)));
    let hooked = Arc::new(HookedShape::new(
        Arc::new(AnyShape),
        vec![Arc::new(TraceMarkHook)],
    ));
    let table_bare_member = cx
        .factory()
        .list(vec![
            cx.factory().symbol(Symbol::new("table-bare")).unwrap(),
            cx.factory().opaque(table.clone()).unwrap(),
        ])
        .unwrap();
    let hooked_expr_value = value_from_expr(
        &mut cx,
        &crate::citizen::encode_shape_expr(hooked.as_ref()).unwrap(),
    )
    .unwrap();
    let hooked_expr_member = cx
        .factory()
        .list(vec![
            cx.factory().symbol(Symbol::new("hooked-expr")).unwrap(),
            hooked_expr_value,
        ])
        .unwrap();

    let members = cx
        .factory()
        .list(vec![table_bare_member, hooked_expr_member])
        .unwrap();

    let decoded = crate::citizen::decode_venn_members(&mut cx, members).unwrap();
    let decoded_venn = cx
        .factory()
        .opaque(Arc::new(VennShapeSet::new(decoded.clone())))
        .unwrap();
    let (class, args) = constructor(&decoded_venn, &mut cx);
    assert_eq!(class, venn_shape_set_class_symbol());
    assert_eq!(
        args,
        vec![
            Expr::Symbol(Symbol::new("v1")),
            Expr::List(vec![
                Expr::List(vec![
                    Expr::Symbol(Symbol::new("table-bare")),
                    crate::citizen::encode_shape_expr(table.as_ref()).unwrap(),
                ]),
                Expr::List(vec![
                    Expr::Symbol(Symbol::new("hooked-expr")),
                    crate::citizen::encode_shape_expr(hooked.as_ref()).unwrap(),
                ]),
            ]),
        ]
    );
    assert!(decoded_shape_accepts_expr(
        decoded[0].1.as_ref(),
        &mut cx,
        Expr::Map(vec![(Expr::Symbol(Symbol::new("ok")), Expr::Bool(true))]),
    ));
    assert!(decoded_shape_accepts_expr(
        decoded[1].1.as_ref(),
        &mut cx,
        Expr::String("hooked".to_owned()),
    ));
}

#[test]
fn unsupported_live_shape_values_still_fail_closed() {
    let mut cx = cx();
    cx.grant(read_construct_capability());
    let live_shape = cx.factory().opaque(Arc::new(LiveShape)).unwrap();

    let err = match crate::citizen::decode_shape_value(&mut cx, live_shape, "shape") {
        Ok(_) => panic!("unsupported live shapes must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        Error::Eval(message)
            if message
                == "citizen field shape: shape is not a citizen-supported pure descriptor"
    ));
}

#[test]
fn hooked_shape_bare_decode_requires_descriptor_backed_hooks() {
    let mut cx = cx();
    cx.grant(read_construct_capability());
    let live_hooked = cx
        .factory()
        .opaque(Arc::new(HookedShape::new(
            Arc::new(AnyShape),
            vec![Arc::new(LiveHook)],
        )))
        .unwrap();

    let err = match crate::citizen::decode_shape_value(&mut cx, live_hooked, "shape") {
        Ok(_) => panic!("live hooks must fail closed during bare shape decoding"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        Error::Eval(message)
            if message
                == "citizen field shape: shape hook shape/live-hook is not a pure descriptor citizen"
    ));
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
