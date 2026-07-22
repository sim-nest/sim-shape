use std::sync::Arc;

use sim_kernel::{Diagnostic, Expr, NumberLiteral, Result, Symbol, Value};

use crate::{
    AcceptOnNoDiagnosticsHook, AndShape, AnyShape, Bindings, DiscardOnDiagnosticPrefixHook,
    ExactExprShape, ExprKind, ExprKindShape, HookedShape, ListShape, MatchScore, NotShape,
    OneOfShape, OrShape, Shape, ShapeDoc, ShapeMatch, ShapeNormalKind, ShapeProbe,
    ShapeRelationKind, TableExtraPolicy, TableFieldSpec, TableShape, VennShapeSet, normalize_shape,
    relate_shapes,
};

use sim_kernel::testing::bare_cx as cx;

fn number_expr(text: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: text.to_owned(),
    })
}

#[test]
fn normalization_flattens_nested_and() {
    let mut cx = cx();
    let shape = AndShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(AndShape::new(vec![
            Arc::new(ExprKindShape::new(ExprKind::String)),
            Arc::new(AnyShape),
        ])),
    ]);

    let normalized = normalize_shape(&mut cx, &shape).unwrap();

    let ShapeNormalKind::And(parts) = normalized.kind else {
        panic!("expected and normal form");
    };
    assert_eq!(parts.len(), 3);
}

#[test]
fn normalization_flattens_nested_or_and_one_of() {
    let mut cx = cx();
    let shape = OrShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(OneOfShape::new(vec![
            Arc::new(ExprKindShape::new(ExprKind::String)),
            Arc::new(OrShape::new(vec![Arc::new(ExprKindShape::new(
                ExprKind::Bool,
            ))])),
        ])),
    ]);

    let normalized = normalize_shape(&mut cx, &shape).unwrap();

    let ShapeNormalKind::Or(parts) = normalized.kind else {
        panic!("expected or normal form");
    };
    assert_eq!(parts.len(), 3);
}

#[test]
fn compare_reports_equal_when_both_subshape_directions_are_true() {
    let mut cx = cx();
    let left = ExactExprShape::new(number_expr("1"));
    let right = ExactExprShape::new(number_expr("1"));

    let relation = relate_shapes(&mut cx, &left, &right, &[]).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Equal);
    assert!(relation.proven);
}

#[test]
fn compare_reports_left_subshape_when_only_left_implies_right() {
    let mut cx = cx();
    let left = ExactExprShape::new(number_expr("1"));
    let right = ExprKindShape::new(ExprKind::Number);

    let relation = relate_shapes(&mut cx, &left, &right, &[]).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::LeftSubshape);
    assert!(relation.proven);
}

#[test]
fn compare_reports_right_subshape_when_only_right_implies_left() {
    let mut cx = cx();
    let left = ExprKindShape::new(ExprKind::Number);
    let right = ExactExprShape::new(number_expr("1"));

    let relation = relate_shapes(&mut cx, &left, &right, &[]).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::RightSubshape);
    assert!(relation.proven);
}

#[test]
fn compare_reports_overlap_with_both_accepted_witness() {
    let mut cx = cx();
    let left = OrShape::new(vec![Arc::new(ExprKindShape::new(ExprKind::Number))]);
    let right = OrShape::new(vec![
        Arc::new(ExactExprShape::new(number_expr("1"))),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    ]);
    let probes = vec![ShapeProbe::Expr {
        label: "one".to_owned(),
        expr: number_expr("1"),
    }];

    let relation = relate_shapes(&mut cx, &left, &right, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Overlap);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses.len(), 1);
}

#[test]
fn compare_hook_widened_shape_stays_unproven() {
    let mut cx = cx();
    let hooked = HookedShape::new(
        Arc::new(QuietTrueOnlyShape),
        vec![Arc::new(AcceptOnNoDiagnosticsHook)],
    );
    let bool_expr = ExprKindShape::new(ExprKind::Bool);
    let probes = vec![ShapeProbe::Expr {
        label: "string".to_owned(),
        expr: Expr::String("surprise".to_owned()),
    }];

    let relation = relate_shapes(&mut cx, &hooked, &bool_expr, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Unknown);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses[0].note, "accepted by left only");
}

#[test]
fn compare_hook_narrowed_shape_stays_unproven() {
    let mut cx = cx();
    let hooked = HookedShape::new(
        Arc::new(DiagnosticBoolShape),
        vec![Arc::new(DiscardOnDiagnosticPrefixHook::new("inner:"))],
    );
    let bool_expr = ExprKindShape::new(ExprKind::Bool);
    let probes = vec![ShapeProbe::Expr {
        label: "true".to_owned(),
        expr: Expr::Bool(true),
    }];

    let relation = relate_shapes(&mut cx, &hooked, &bool_expr, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Unknown);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses[0].note, "accepted by right only");
}

#[test]
fn compare_closed_child_extra_key_stays_unproven() {
    let mut cx = cx();
    let left = TableShape::new(
        vec![TableFieldSpec {
            key: Symbol::new("y"),
            shape: Arc::new(ExprKindShape::new(ExprKind::Number)),
            required: true,
        }],
        TableExtraPolicy::Reject,
    );
    let right = TableShape::new(Vec::new(), TableExtraPolicy::Reject);
    let probes = vec![ShapeProbe::Expr {
        label: "child-only y".to_owned(),
        expr: Expr::Map(vec![(Expr::Symbol(Symbol::new("y")), number_expr("1"))]),
    }];

    let relation = relate_shapes(&mut cx, &left, &right, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Unknown);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses[0].note, "accepted by left only");
}

#[test]
fn compare_optional_parent_field_constraint_stays_unproven() {
    let mut cx = cx();
    let left = TableShape::new(
        vec![TableFieldSpec {
            key: Symbol::new("x"),
            shape: Arc::new(ExprKindShape::new(ExprKind::Number)),
            required: true,
        }],
        TableExtraPolicy::Shape(Arc::new(ExprKindShape::new(ExprKind::String))),
    );
    let right = TableShape::new(
        vec![
            TableFieldSpec {
                key: Symbol::new("x"),
                shape: Arc::new(ExprKindShape::new(ExprKind::Number)),
                required: true,
            },
            TableFieldSpec {
                key: Symbol::new("y"),
                shape: Arc::new(ExprKindShape::new(ExprKind::Number)),
                required: false,
            },
        ],
        TableExtraPolicy::Reject,
    );
    let probes = vec![ShapeProbe::Expr {
        label: "bad optional y".to_owned(),
        expr: Expr::Map(vec![
            (Expr::Symbol(Symbol::new("x")), number_expr("1")),
            (
                Expr::Symbol(Symbol::new("y")),
                Expr::String("bad".to_owned()),
            ),
        ]),
    }];

    let relation = relate_shapes(&mut cx, &left, &right, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Unknown);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses[0].note, "accepted by left only");
}

#[test]
fn compare_does_not_claim_disjoint_from_probe_absence() {
    let mut cx = cx();
    let left = ExprKindShape::new(ExprKind::Number);
    let right = ExprKindShape::new(ExprKind::String);
    let probes = vec![ShapeProbe::Expr {
        label: "bool".to_owned(),
        expr: Expr::Bool(true),
    }];

    let relation = relate_shapes(&mut cx, &left, &right, &probes).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Unknown);
    assert!(!relation.proven);
    assert_eq!(relation.witnesses[0].note, "accepted by neither");
}

#[test]
fn not_shape_and_inner_compare_as_disjoint() {
    let mut cx = cx();
    let inner = Arc::new(ExprKindShape::new(ExprKind::Number));
    let left = NotShape::new(inner.clone());

    let relation = relate_shapes(&mut cx, &left, inner.as_ref(), &[]).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Disjoint);
    assert!(relation.proven);
}

#[test]
fn fixed_length_list_mismatch_can_be_disjoint() {
    let mut cx = cx();
    let left = ListShape::new(vec![Arc::new(AnyShape)]);
    let right = ListShape::new(vec![Arc::new(AnyShape), Arc::new(AnyShape)]);

    let relation = relate_shapes(&mut cx, &left, &right, &[]).unwrap();

    assert_eq!(relation.kind, ShapeRelationKind::Disjoint);
    assert!(relation.proven);
}

#[test]
fn venn_union_accepts_value_accepted_by_any_member() {
    let mut cx = cx();
    let venn = VennShapeSet::new(vec![
        (
            Symbol::new("number"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        ),
        (
            Symbol::new("string"),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        ),
    ]);

    assert!(
        venn.union()
            .check_expr(&mut cx, &Expr::String("ok".to_owned()))
            .unwrap()
            .accepted
    );
}

#[test]
fn venn_intersection_accepts_only_values_accepted_by_all_members() {
    let mut cx = cx();
    let venn = VennShapeSet::new(vec![
        (
            Symbol::new("number"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        ),
        (
            Symbol::new("one"),
            Arc::new(ExactExprShape::new(number_expr("1"))),
        ),
    ]);
    let shape = venn.intersection();

    assert!(
        shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &number_expr("2"))
            .unwrap()
            .accepted
    );
}

#[test]
fn venn_only_excludes_sibling_shapes() {
    let mut cx = cx();
    let venn = VennShapeSet::new(vec![
        (
            Symbol::new("number"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        ),
        (
            Symbol::new("string"),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        ),
    ]);
    let shape = venn.only(&Symbol::new("number")).unwrap();

    assert!(
        shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::String("ok".to_owned()))
            .unwrap()
            .accepted
    );
}

#[test]
fn venn_outside_rejects_values_in_the_union() {
    let mut cx = cx();
    let venn = VennShapeSet::new(vec![(
        Symbol::new("number"),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
    )]);
    let shape = venn.outside_all();

    assert!(
        !shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        shape
            .check_expr(&mut cx, &Expr::String("ok".to_owned()))
            .unwrap()
            .accepted
    );
}

struct QuietTrueOnlyShape;

impl Shape for QuietTrueOnlyShape {
    fn is_subshape_of(&self, _cx: &mut sim_kernel::Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<ExprKindShape>() else {
            return Ok(None);
        };
        Ok((*parent.kind() == ExprKind::Bool).then_some(true))
    }

    fn check_value(&self, cx: &mut sim_kernel::Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut sim_kernel::Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::Bool(true) => Ok(ShapeMatch::accept(MatchScore::exact(10))),
            _ => Ok(ShapeMatch {
                accepted: false,
                captures: Bindings::new(),
                score: MatchScore::reject(),
                diagnostics: Vec::new(),
            }),
        }
    }

    fn describe(&self, _cx: &mut sim_kernel::Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("quiet true only"))
    }
}

struct DiagnosticBoolShape;

impl Shape for DiagnosticBoolShape {
    fn is_subshape_of(&self, _cx: &mut sim_kernel::Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<ExprKindShape>() else {
            return Ok(None);
        };
        Ok((*parent.kind() == ExprKind::Bool).then_some(true))
    }

    fn check_value(&self, cx: &mut sim_kernel::Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut sim_kernel::Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::Bool(flag) => {
                let mut matched = ShapeMatch::accept(MatchScore::exact(10));
                if *flag {
                    matched
                        .diagnostics
                        .push(Diagnostic::info("inner: accepted true"));
                }
                Ok(matched)
            }
            _ => Ok(ShapeMatch::reject_with_diagnostic(Diagnostic::error(
                "expected bool",
            ))),
        }
    }

    fn describe(&self, _cx: &mut sim_kernel::Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("diagnostic bool"))
    }
}

#[test]
fn venn_exactly_includes_selected_and_excludes_unselected_shapes() {
    let mut cx = cx();
    let venn = VennShapeSet::new(vec![
        (
            Symbol::new("number"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        ),
        (
            Symbol::new("one"),
            Arc::new(ExactExprShape::new(number_expr("1"))),
        ),
        (
            Symbol::new("string"),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        ),
    ]);
    let shape = venn
        .exactly(&[Symbol::new("number"), Symbol::new("one")])
        .unwrap();

    assert!(
        shape
            .check_expr(&mut cx, &number_expr("1"))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &number_expr("2"))
            .unwrap()
            .accepted
    );
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::String("ok".to_owned()))
            .unwrap()
            .accepted
    );
}
