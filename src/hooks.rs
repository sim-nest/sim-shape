//! Match-extension hooks: `HookedShape` wraps an inner shape with the
//! `MatchHook` protocol and the built-in hook implementations that adjust
//! acceptance, scores, and diagnostics.

mod hooked;
mod types;

#[cfg(test)]
mod tests;

pub use hooked::HookedShape;
pub use types::{
    AcceptOnNoDiagnosticsHook, DiscardOnDiagnosticPrefixHook, MatchHook, MatchHookContext,
    MatchHookDecision, MatchHookKind, MatchHookObject, MatchHookPhase, MatchHookTargetKind,
    ScoreFloorHook, TraceMarkHook, accept_on_no_diagnostics_hook_class_symbol,
    discard_on_diagnostic_prefix_hook_class_symbol, hook_ref_arc, hook_value,
    score_floor_hook_class_symbol, trace_mark_hook_class_symbol,
};
