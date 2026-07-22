//! Shape relation analysis: classify how two shapes relate (subshape, equal,
//! disjoint, overlapping) via probes over their normalized forms.

use sim_kernel::{Cx, Diagnostic, Expr, Result, Value, shape_is_subshape_of};

use crate::{
    ExactExprShape, Shape, TableExtraPolicy, TableShape,
    compare::normal::{ShapeNormalForm, ShapeNormalKind, normalize_shape},
};

/// Conservative relation report between two shapes.
///
/// A relation is `proven` only when the runtime can establish it through
/// subshape checks or explicit conservative rules. Probe-only overlap remains
/// useful evidence but is not a proof.
#[derive(Clone, Debug)]
pub struct ShapeRelation {
    /// Normal form for the left input shape.
    pub left: ShapeNormalForm,
    /// Normal form for the right input shape.
    pub right: ShapeNormalForm,
    /// Best relation kind the runtime could determine.
    pub kind: ShapeRelationKind,
    /// Whether the relation kind is proven rather than probe-derived.
    pub proven: bool,
    /// Probe results collected while comparing the shapes.
    pub witnesses: Vec<ShapeWitness>,
    /// Diagnostic notes explaining conservative proof rules.
    pub diagnostics: Vec<Diagnostic>,
}

/// Relation categories reported by [`ShapeRelation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeRelationKind {
    /// Both shapes imply each other.
    Equal,
    /// The left shape is known to be a subshape of the right shape.
    LeftSubshape,
    /// The right shape is known to be a subshape of the left shape.
    RightSubshape,
    /// At least one probe is accepted by both shapes.
    Overlap,
    /// The shapes are conservatively known to have no common values.
    Disjoint,
    /// The runtime could not prove a stronger relation.
    Unknown,
}

/// Result of checking one probe against both compared shapes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeWitness {
    /// Stable label supplied by the caller.
    pub label: String,
    /// Whether the left shape accepted the probe.
    pub accepted_left: bool,
    /// Whether the right shape accepted the probe.
    pub accepted_right: bool,
    /// Short explanation such as `accepted by both`.
    pub note: String,
}

/// Example value or expression that gathers relation evidence.
#[derive(Clone, Debug)]
pub enum ShapeProbe {
    /// Runtime value probe.
    Value {
        /// Stable label recorded on the resulting witness.
        label: String,
        /// Value checked against both shapes.
        value: Value,
    },
    /// Expression probe.
    Expr {
        /// Stable label recorded on the resulting witness.
        label: String,
        /// Expression checked against both shapes.
        expr: Expr,
    },
}

/// Compare two shapes with conservative proof rules plus optional probes.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{
/// #     ExactExprShape, ExprKind, ExprKindShape, ShapeRelationKind, relate_shapes,
/// # };
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let exact_true = ExactExprShape::new(Expr::Bool(true));
/// let bool_expr = ExprKindShape::new(ExprKind::Bool);
/// let relation = relate_shapes(&mut cx, &exact_true, &bool_expr, &[]).unwrap();
///
/// assert_eq!(relation.kind, ShapeRelationKind::LeftSubshape);
/// assert!(relation.proven);
/// ```
pub fn relate_shapes(
    cx: &mut Cx,
    left: &dyn Shape,
    right: &dyn Shape,
    probes: &[ShapeProbe],
) -> Result<ShapeRelation> {
    let left_normal = normalize_shape(cx, left)?;
    let right_normal = normalize_shape(cx, right)?;

    let left_to_right = shape_is_subshape_of(cx, left, right)?;
    let right_to_left = shape_is_subshape_of(cx, right, left)?;
    if left_to_right && right_to_left {
        return Ok(relation(
            left_normal,
            right_normal,
            ShapeRelationKind::Equal,
            true,
            Vec::new(),
            Vec::new(),
        ));
    }
    if left_to_right {
        return Ok(relation(
            left_normal,
            right_normal,
            ShapeRelationKind::LeftSubshape,
            true,
            Vec::new(),
            Vec::new(),
        ));
    }
    if right_to_left {
        return Ok(relation(
            left_normal,
            right_normal,
            ShapeRelationKind::RightSubshape,
            true,
            Vec::new(),
            Vec::new(),
        ));
    }

    if let Some(message) = static_disjoint(cx, left, right, &left_normal, &right_normal)? {
        return Ok(relation(
            left_normal,
            right_normal,
            ShapeRelationKind::Disjoint,
            true,
            Vec::new(),
            vec![Diagnostic::info(message)],
        ));
    }

    let witnesses = probes
        .iter()
        .map(|probe| run_probe(cx, left, right, probe))
        .collect::<Result<Vec<_>>>()?;
    if witnesses
        .iter()
        .any(|witness| witness.accepted_left && witness.accepted_right)
    {
        return Ok(relation(
            left_normal,
            right_normal,
            ShapeRelationKind::Overlap,
            false,
            witnesses,
            Vec::new(),
        ));
    }

    Ok(relation(
        left_normal,
        right_normal,
        ShapeRelationKind::Unknown,
        false,
        witnesses,
        Vec::new(),
    ))
}

fn relation(
    left: ShapeNormalForm,
    right: ShapeNormalForm,
    kind: ShapeRelationKind,
    proven: bool,
    witnesses: Vec<ShapeWitness>,
    diagnostics: Vec<Diagnostic>,
) -> ShapeRelation {
    ShapeRelation {
        left,
        right,
        kind,
        proven,
        witnesses,
        diagnostics,
    }
}

fn run_probe(
    cx: &mut Cx,
    left: &dyn Shape,
    right: &dyn Shape,
    probe: &ShapeProbe,
) -> Result<ShapeWitness> {
    let (label, accepted_left, accepted_right) = match probe {
        ShapeProbe::Value { label, value } => (
            label.clone(),
            left.check_value(cx, value.clone())?.accepted,
            right.check_value(cx, value.clone())?.accepted,
        ),
        ShapeProbe::Expr { label, expr } => (
            label.clone(),
            left.check_expr(cx, expr)?.accepted,
            right.check_expr(cx, expr)?.accepted,
        ),
    };
    let note = match (accepted_left, accepted_right) {
        (true, true) => "accepted by both",
        (true, false) => "accepted by left only",
        (false, true) => "accepted by right only",
        (false, false) => "accepted by neither",
    }
    .to_owned();
    Ok(ShapeWitness {
        label,
        accepted_left,
        accepted_right,
        note,
    })
}

fn static_disjoint(
    cx: &mut Cx,
    left: &dyn Shape,
    right: &dyn Shape,
    left_normal: &ShapeNormalForm,
    right_normal: &ShapeNormalForm,
) -> Result<Option<String>> {
    if not_of(left_normal, right_normal) || not_of(right_normal, left_normal) {
        return Ok(Some(
            "shape-compare: negation excludes inner shape".to_owned(),
        ));
    }
    if exact_exprs_differ(left, right) {
        return Ok(Some(
            "shape-compare: exact expression shapes differ".to_owned(),
        ));
    }
    if fixed_list_lengths_differ(left_normal, right_normal) {
        return Ok(Some("shape-compare: fixed list lengths differ".to_owned()));
    }
    if closed_table_field_disjoint(cx, left, right)? {
        return Ok(Some(
            "shape-compare: closed tables require disjoint shared field".to_owned(),
        ));
    }
    Ok(None)
}

fn not_of(not_candidate: &ShapeNormalForm, other: &ShapeNormalForm) -> bool {
    matches!(&not_candidate.kind, ShapeNormalKind::Not(inner) if inner.as_ref() == other)
}

fn exact_exprs_differ(left: &dyn Shape, right: &dyn Shape) -> bool {
    let Some(left) = left.as_any().downcast_ref::<ExactExprShape>() else {
        return false;
    };
    let Some(right) = right.as_any().downcast_ref::<ExactExprShape>() else {
        return false;
    };
    !left.expected().canonical_eq(right.expected())
}

fn fixed_list_lengths_differ(left: &ShapeNormalForm, right: &ShapeNormalForm) -> bool {
    match (&left.kind, &right.kind) {
        (
            ShapeNormalKind::List {
                items: left,
                rest: None,
            },
            ShapeNormalKind::List {
                items: right,
                rest: None,
            },
        ) => left.len() != right.len(),
        _ => false,
    }
}

fn closed_table_field_disjoint(cx: &mut Cx, left: &dyn Shape, right: &dyn Shape) -> Result<bool> {
    let Some(left) = left.as_any().downcast_ref::<TableShape>() else {
        return Ok(false);
    };
    let Some(right) = right.as_any().downcast_ref::<TableShape>() else {
        return Ok(false);
    };
    if !matches!(left.extra(), TableExtraPolicy::Reject)
        || !matches!(right.extra(), TableExtraPolicy::Reject)
    {
        return Ok(false);
    }

    for left_field in left.fields().iter().filter(|field| field.required) {
        let Some(right_field) = right
            .fields()
            .iter()
            .find(|field| field.required && field.key == left_field.key)
        else {
            continue;
        };
        let relation = relate_shapes(
            cx,
            left_field.shape.as_ref(),
            right_field.shape.as_ref(),
            &[],
        )?;
        if relation.proven && relation.kind == ShapeRelationKind::Disjoint {
            return Ok(true);
        }
    }
    Ok(false)
}
