#![forbid(unsafe_code)]
#![allow(deprecated)]
#![deny(missing_docs)]
//! Shape algebra, comparison, and match-hook helpers.
//!
//! `sim-shape` supplies concrete `Shape` implementations while the kernel owns
//! only the open shape protocol.
//!
//! ```rust
//! use std::sync::Arc;
//!
//! use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
//! use sim_shape::{
//!     AnyShape, ExactExprShape, ExprKind, ExprKindShape, HookedShape, Shape,
//!     ShapeRelationKind, TraceMarkHook, relate_shapes,
//! };
//!
//! let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
//! let exact_true = ExactExprShape::new(Expr::Bool(true));
//! let bool_expr = ExprKindShape::new(ExprKind::Bool);
//! let relation = relate_shapes(&mut cx, &exact_true, &bool_expr, &[]).unwrap();
//!
//! assert_eq!(relation.kind, ShapeRelationKind::LeftSubshape);
//! assert!(relation.proven);
//!
//! let hooked = HookedShape::new(Arc::new(AnyShape), vec![Arc::new(TraceMarkHook)]);
//! let matched = hooked.check_expr(&mut cx, &Expr::String("trace me".to_owned())).unwrap();
//! assert!(matched.accepted);
//! assert!(matched.diagnostics.iter().any(|diagnostic| {
//!     diagnostic.message.starts_with("shape-hook:mark")
//! }));
//! ```
//!
//! # Surface
//!
//! The engine is organized into private module groups whose public items are
//! re-exported at the crate root (the modules themselves are not public):
//!
//! - `primitives` -- atomic shapes (`AnyShape`, `ExactExprShape`,
//!   `ExprKindShape`, `ClassShape`, `NumberValueShape`, `ListShape`,
//!   `FieldShape`) and the combinators and object-grammar parsers built on
//!   them.
//! - `algebra` -- boolean and collection shape algebra: `AndShape`,
//!   `OrShape`, `NotShape`, `RepeatShape`, and `TableShape` with its field and
//!   extra-field policies.
//! - `compare` -- shape comparison and subsumption: normalization
//!   (`normalize_shape`, `ShapeNormalForm`), relation analysis
//!   (`relate_shapes`, `ShapeRelation`), and `VennShapeSet` set reasoning.
//! - `citizen` -- citizen integration: class registration, codec, and
//!   construction for shape objects, plus the `*_class_symbol` accessors.
//! - `functions` -- the callable shape object: `FunctionObject`,
//!   `FunctionCase`, overload selection, and shape-as-value wrapping.
//! - `hooks` -- match-extension hooks: `HookedShape` and the `MatchHook`
//!   protocol with the built-in hook implementations.
//! - `grammar` -- codec-neutral grammar graph lowering plus the seed JSON
//!   Schema renderer used by existing model-runner contracts.
//! - `parse` -- the shape grammar parser that turns an `Expr` into a `Shape`
//!   and runs checks against expressions and values.
//! - `recursive` -- recursive shape descriptors with named definitions and
//!   bounded reference checks.
//! - `base` -- the base shape vocabulary re-exported from the kernel
//!   (`Shape`, `ShapeMatch`, `ShapeDoc`, `Bindings`, `ShapeReport`).

mod algebra;
mod base;
mod citizen;
#[cfg(test)]
mod citizen_tests;
mod compare;
mod diagnostics;
#[cfg(test)]
mod duplicate_key_tests;
mod duplicate_keys;
mod functions;
mod grammar;
mod hooks;
mod options;
mod parse;
#[cfg(test)]
mod parse_tests;
mod primitives;
mod recursion;
mod recursive;
#[cfg(test)]
mod tests;

pub use algebra::{
    AndShape, NotShape, OrShape, OrStrategy, RepeatShape, TableExtraPolicy, TableFieldSpec,
    TableShape,
};
pub use base::{
    Bindings, ExprKind, MatchScore, Shape, ShapeBindings, ShapeDoc, ShapeMatch, ShapeReport,
    check_value_report, insert_shape_satisfaction_claim, satisfies_shape_predicate,
    shape_report_from_match,
};
pub use citizen::{
    and_shape_class_symbol, any_shape_class_symbol, class_shape_class_symbol,
    exact_expr_shape_class_symbol, expr_kind_shape_class_symbol, hooked_shape_class_symbol,
    list_shape_class_symbol, not_shape_class_symbol, or_shape_class_symbol,
    repeat_shape_class_symbol, shape_def_ref_class_symbol, shape_defs_class_symbol,
    table_shape_class_symbol, venn_shape_set_class_symbol,
};
pub use compare::{
    ShapeNormalForm, ShapeNormalKind, ShapeProbe, ShapeRelation, ShapeRelationKind, ShapeWitness,
    VennShapeSet, normalize_shape, relate_shapes,
};
pub use diagnostics::{
    binding_failure_diagnostic, callable_mismatch_diagnostic, expected_shape_diagnostic,
    overload_selection_diagnostic,
};
pub use functions::{
    FunctionCase, FunctionObject, NativeFunctionImpl, SelectedCase, ShapeObject, case_result_shape,
    case_shape, function_cases, overload, shape_value, shape_value_with_encoding,
};
pub use grammar::{
    GrammarDialect, GrammarGraph, GrammarPosition, GrammarTarget, Production, ShapeGrammar,
    TerminalAtom, shape_grammar_graph, shape_json_schema,
};
pub use hooks::{
    AcceptOnNoDiagnosticsHook, DiscardOnDiagnosticPrefixHook, HookedShape, MatchHook,
    MatchHookContext, MatchHookDecision, MatchHookKind, MatchHookObject, MatchHookPhase,
    MatchHookTargetKind, ScoreFloorHook, TraceMarkHook, accept_on_no_diagnostics_hook_class_symbol,
    discard_on_diagnostic_prefix_hook_class_symbol, hook_ref_arc, hook_value,
    score_floor_hook_class_symbol, trace_mark_hook_class_symbol,
};
pub use options::{OptionFieldSpec, check_option_map};
pub use parse::{check_shape_on_expr, check_shape_on_value, parse_shape_expr, shape_error};
pub use primitives::{
    AnyShape, CaptureShape, ClassShape, EffectfulShape, ExactExprShape, ExprKindShape, FieldShape,
    FieldSpec, ListShape, NumberValueShape, ObjectExpr, OneOfShape, PrattShape, ShapeExprParser,
};
pub use recursive::{ShapeDefRef, ShapeDefs};
