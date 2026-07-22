//! Reusable shape-query predicates built on relation analysis.

use sim_kernel::{Cx, Result};

use crate::base::Shape;
use crate::compare::{ShapeRelationKind, relate_shapes};

/// Direction used when matching a candidate Shape against a requested Shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeQueryRelation {
    /// The candidate accepts every value accepted by the requested Shape.
    Subsumes,
    /// The candidate is contained by the requested Shape.
    SubshapeOf,
    /// The candidate and requested Shapes have at least one known value in common.
    Overlaps,
}

/// Returns whether `candidate` satisfies the requested relation to `wanted`.
///
/// The relation is computed with the existing [`relate_shapes`] helper. Unknown
/// relations fail closed for query filtering.
pub fn shape_query_matches(
    cx: &mut Cx,
    candidate: &dyn Shape,
    wanted: &dyn Shape,
    relation: ShapeQueryRelation,
) -> Result<bool> {
    let relation_kind = relate_shapes(cx, candidate, wanted, &[])?.kind;
    Ok(match relation {
        ShapeQueryRelation::Subsumes => matches!(
            relation_kind,
            ShapeRelationKind::Equal | ShapeRelationKind::RightSubshape
        ),
        ShapeQueryRelation::SubshapeOf => matches!(
            relation_kind,
            ShapeRelationKind::Equal | ShapeRelationKind::LeftSubshape
        ),
        ShapeQueryRelation::Overlaps => matches!(
            relation_kind,
            ShapeRelationKind::Equal
                | ShapeRelationKind::LeftSubshape
                | ShapeRelationKind::RightSubshape
                | ShapeRelationKind::Overlap
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};

    use crate::{AnyShape, ExprKind, ExprKindShape, ShapeQueryRelation, shape_query_matches};

    fn bare_cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }

    #[test]
    fn directional_shape_queries_match_relation_semantics() {
        let mut cx = bare_cx();
        let any = AnyShape;
        let string = ExprKindShape::new(ExprKind::String);

        assert!(shape_query_matches(&mut cx, &any, &string, ShapeQueryRelation::Subsumes).unwrap());
        assert!(
            !shape_query_matches(&mut cx, &string, &any, ShapeQueryRelation::Subsumes).unwrap()
        );
        assert!(
            shape_query_matches(&mut cx, &string, &any, ShapeQueryRelation::SubshapeOf).unwrap()
        );
        assert!(shape_query_matches(&mut cx, &string, &any, ShapeQueryRelation::Overlaps).unwrap());
    }

    #[test]
    fn disjoint_shapes_do_not_overlap() {
        let mut cx = bare_cx();
        let string = ExprKindShape::new(ExprKind::String);
        let number = ExprKindShape::new(ExprKind::Number);

        assert!(
            !shape_query_matches(&mut cx, &string, &number, ShapeQueryRelation::Overlaps).unwrap()
        );
    }
}
