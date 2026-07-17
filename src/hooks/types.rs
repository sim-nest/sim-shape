//! Match-hook protocol and built-ins: the `MatchHook` trait, its context and
//! decision types, the hook object wrapper, and the standard hook
//! implementations (trace, score floor, accept/discard on diagnostics).

use std::sync::Arc;

use sim_citizen_derive::non_citizen;
use sim_kernel::{
    ClassRef, Cx, DefaultFactory, Error, Expr, Factory, NumberLiteral, Object, ObjectEncode,
    ObjectEncoding, Result, Symbol, Value,
};

use crate::{MatchScore, ShapeMatch};

/// Capability class for a match hook decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchHookKind {
    /// Observe and emit diagnostics without changing acceptance.
    Mark,
    /// Optionally turn a rejected match into an accepted one.
    Accept,
    /// Optionally turn an accepted match into a rejected one.
    Discard,
    /// Add annotations such as score deltas and diagnostics.
    Annotate,
}

impl MatchHookKind {
    /// Whether this hook kind leaves acceptance unchanged.
    pub fn preserves_acceptance(self) -> bool {
        matches!(self, Self::Mark | Self::Annotate)
    }
}

/// Match target observed by a hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchHookTargetKind {
    /// Runtime value check.
    Value,
    /// Expression check.
    Expr,
}

/// Point in the wrapper algorithm where a hook runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchHookPhase {
    /// Before the inner shape is checked.
    BeforeInner,
    /// After the inner shape has produced a match.
    AfterInner,
}

/// Context supplied to every hook invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchHookContext {
    /// Position of the hook in the wrapper registration order.
    pub hook_index: usize,
    /// Current execution phase.
    pub phase: MatchHookPhase,
    /// Whether the check is for a value or expression.
    pub target_kind: MatchHookTargetKind,
    /// Description name for the wrapped shape.
    pub shape_label: String,
}

/// Hook result interpreted by [`HookedShape`](crate::HookedShape).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchHookDecision {
    /// Do nothing.
    Pass,
    /// Emit a mark diagnostic.
    Mark {
        /// Mark text recorded as an info diagnostic.
        message: String,
    },
    /// Accept a currently rejected match.
    Accept {
        /// Explanation recorded for the repair.
        reason: String,
        /// Score assigned when the inner match left a reject score.
        score: MatchScore,
    },
    /// Reject a currently accepted match.
    Discard {
        /// Explanation recorded for the veto.
        reason: String,
    },
    /// Add a diagnostic and score delta.
    Annotate {
        /// Annotation text recorded as an info diagnostic.
        message: String,
        /// Amount added to the match score.
        score_delta: i32,
    },
}

/// Runtime hook contract for neutral shape match membranes.
pub trait MatchHook: Send + Sync {
    /// Stable symbol naming the hook.
    fn symbol(&self) -> Symbol;
    /// Decision class this hook may produce.
    fn kind(&self) -> MatchHookKind;
    /// Constructor encoding for pure, descriptor-backed built-in hooks.
    fn object_encoding(&self) -> Option<ObjectEncoding> {
        None
    }
    /// Run the hook for the supplied context and current match state.
    fn apply(
        &self,
        cx: &mut Cx,
        ctx: &MatchHookContext,
        current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision>;
}

/// Opaque runtime object that carries a shape hook.
#[non_citizen(
    reason = "may wrap custom live hook code; built-in pure hook descriptors use shape/*Hook citizens",
    kind = "function",
    descriptor = "shape/live-hook"
)]
#[derive(Clone)]
pub struct MatchHookObject {
    hook: Arc<dyn MatchHook>,
}

impl MatchHookObject {
    /// Wrap a hook as an opaque runtime object.
    pub fn new(hook: Arc<dyn MatchHook>) -> Self {
        Self { hook }
    }

    /// Clone out the wrapped hook handle.
    pub fn hook(&self) -> Arc<dyn MatchHook> {
        self.hook.clone()
    }
}

impl Object for MatchHookObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<shape-hook {} {}>",
            self.hook.symbol(),
            hook_kind_name(self.hook.kind())
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for MatchHookObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(ObjectEncoding::Constructor { class, .. }) = self.hook.object_encoding()
            && let Some(value) = cx.registry().class_by_symbol(&class)
        {
            return Ok(value.clone());
        }
        cx.factory().nil()
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        match self.object_encoding(_cx)? {
            ObjectEncoding::Constructor { class, args } => Ok(Expr::Call {
                operator: Box::new(Expr::Symbol(class)),
                args,
            }),
            _ => Err(Error::Eval(format!(
                "shape hook {} produced a non-constructor object encoding; only \
                 constructor encodings can render as an expression",
                self.hook.symbol()
            ))),
        }
    }

    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        self.hook.object_encoding().is_some().then_some(self)
    }
}

impl ObjectEncode for MatchHookObject {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        self.hook.object_encoding().ok_or_else(|| {
            Error::Eval(format!(
                "shape hook {} is not a pure descriptor citizen",
                self.hook.symbol()
            ))
        })
    }
}

/// Wrap a hook as an opaque runtime value.
pub fn hook_value(hook: Arc<dyn MatchHook>) -> Value {
    DefaultFactory
        .opaque(Arc::new(MatchHookObject::new(hook)))
        .expect("hook object should always be boxable")
}

/// Extract a hook from a runtime value produced by [`hook_value`].
pub fn hook_ref_arc(value: &Value) -> Result<Arc<dyn MatchHook>> {
    value
        .object()
        .downcast_ref::<MatchHookObject>()
        .map(MatchHookObject::hook)
        .ok_or(Error::TypeMismatch {
            expected: "shape-hook",
            found: "non-shape-hook",
        })
}

/// Mark hook that emits the wrapped shape label before and after matching.
#[derive(Clone, Default)]
pub struct TraceMarkHook;

impl MatchHook for TraceMarkHook {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("shape", "trace-mark")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Mark
    }

    fn object_encoding(&self) -> Option<ObjectEncoding> {
        Some(hook_encoding(trace_mark_hook_class_symbol(), Vec::new()))
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        ctx: &MatchHookContext,
        _current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        Ok(MatchHookDecision::Mark {
            message: ctx.shape_label.clone(),
        })
    }
}

/// Annotate hook that raises accepted match scores to a minimum floor.
#[derive(Clone)]
pub struct ScoreFloorHook {
    floor: i32,
}

impl ScoreFloorHook {
    /// Build a score-floor hook that lifts accepted scores to `floor`.
    pub fn new(floor: i32) -> Self {
        Self { floor }
    }

    /// The minimum score this hook enforces.
    pub fn floor(&self) -> i32 {
        self.floor
    }
}

impl MatchHook for ScoreFloorHook {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("shape", "score-floor")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Annotate
    }

    fn object_encoding(&self) -> Option<ObjectEncoding> {
        Some(hook_encoding(
            score_floor_hook_class_symbol(),
            vec![int_expr(self.floor)],
        ))
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        _ctx: &MatchHookContext,
        current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        let Some(current) = current else {
            return Ok(MatchHookDecision::Pass);
        };
        if current.accepted && current.score.value() < self.floor {
            return Ok(MatchHookDecision::Annotate {
                message: format!("score floor {}", self.floor),
                score_delta: self.floor - current.score.value(),
            });
        }
        Ok(MatchHookDecision::Pass)
    }
}

/// Accept hook that repairs quiet rejections with score 1.
#[derive(Clone, Default)]
pub struct AcceptOnNoDiagnosticsHook;

impl MatchHook for AcceptOnNoDiagnosticsHook {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("shape", "accept-on-no-diagnostics")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Accept
    }

    fn object_encoding(&self) -> Option<ObjectEncoding> {
        Some(hook_encoding(
            accept_on_no_diagnostics_hook_class_symbol(),
            Vec::new(),
        ))
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        _ctx: &MatchHookContext,
        current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        let Some(current) = current else {
            return Ok(MatchHookDecision::Pass);
        };
        if !current.accepted && current.diagnostics.is_empty() {
            return Ok(MatchHookDecision::Accept {
                reason: "no diagnostics".to_owned(),
                score: MatchScore::exact(1),
            });
        }
        Ok(MatchHookDecision::Pass)
    }
}

/// Discard hook that rejects accepted matches containing a diagnostic prefix.
#[derive(Clone)]
pub struct DiscardOnDiagnosticPrefixHook {
    prefix: String,
}

impl DiscardOnDiagnosticPrefixHook {
    /// Build a discard hook that vetoes matches carrying `prefix` diagnostics.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    /// The diagnostic-message prefix this hook watches for.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

impl MatchHook for DiscardOnDiagnosticPrefixHook {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("shape", "discard-on-diagnostic-prefix")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Discard
    }

    fn object_encoding(&self) -> Option<ObjectEncoding> {
        Some(hook_encoding(
            discard_on_diagnostic_prefix_hook_class_symbol(),
            vec![Expr::String(self.prefix.clone())],
        ))
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        _ctx: &MatchHookContext,
        current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        let Some(current) = current else {
            return Ok(MatchHookDecision::Pass);
        };
        if current.accepted
            && current
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.starts_with(&self.prefix))
        {
            return Ok(MatchHookDecision::Discard {
                reason: self.prefix.clone(),
            });
        }
        Ok(MatchHookDecision::Pass)
    }
}

pub(crate) fn hook_kind_name(kind: MatchHookKind) -> &'static str {
    match kind {
        MatchHookKind::Mark => "mark",
        MatchHookKind::Accept => "accept",
        MatchHookKind::Discard => "discard",
        MatchHookKind::Annotate => "annotate",
    }
}

/// Class symbol for the [`TraceMarkHook`] citizen (`shape/TraceMarkHook`).
pub fn trace_mark_hook_class_symbol() -> Symbol {
    Symbol::qualified("shape", "TraceMarkHook")
}

/// Class symbol for the [`ScoreFloorHook`] citizen (`shape/ScoreFloorHook`).
pub fn score_floor_hook_class_symbol() -> Symbol {
    Symbol::qualified("shape", "ScoreFloorHook")
}

/// Class symbol for the [`AcceptOnNoDiagnosticsHook`] citizen
/// (`shape/AcceptOnNoDiagnosticsHook`).
pub fn accept_on_no_diagnostics_hook_class_symbol() -> Symbol {
    Symbol::qualified("shape", "AcceptOnNoDiagnosticsHook")
}

/// Class symbol for the [`DiscardOnDiagnosticPrefixHook`] citizen
/// (`shape/DiscardOnDiagnosticPrefixHook`).
pub fn discard_on_diagnostic_prefix_hook_class_symbol() -> Symbol {
    Symbol::qualified("shape", "DiscardOnDiagnosticPrefixHook")
}

fn hook_encoding(class: Symbol, fields: Vec<Expr>) -> ObjectEncoding {
    let mut args = Vec::with_capacity(fields.len() + 1);
    args.push(Expr::Symbol(Symbol::new("v1")));
    args.extend(fields);
    ObjectEncoding::Constructor { class, args }
}

fn int_expr(value: i32) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("citizen", "int"),
        canonical: value.to_string(),
    })
}
