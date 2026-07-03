//! Base shape vocabulary: re-exports the kernel `Shape` protocol types and the
//! shape-report helpers that the rest of the engine builds on.

pub use sim_kernel::shape_report::{
    ShapeReport, check_value_report, insert_shape_satisfaction_claim, satisfies_shape_predicate,
    shape_report_from_match,
};
pub use sim_kernel::{
    ExprKind, MatchScore, Shape, ShapeBindings as Bindings, ShapeBindings, ShapeDoc, ShapeMatch,
};
