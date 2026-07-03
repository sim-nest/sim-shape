use std::sync::{Arc, Mutex};

use sim_kernel::{Cx, Diagnostic, Expr, NoopEvalPolicy, Result, Value};

use crate::{
    AcceptOnNoDiagnosticsHook, AnyShape, DiscardOnDiagnosticPrefixHook, HookedShape, MatchHook,
    MatchHookContext, MatchHookDecision, MatchHookKind, MatchHookPhase, MatchHookTargetKind,
    MatchScore, ScoreFloorHook, Shape, ShapeDoc, ShapeMatch, TraceMarkHook, hook_value,
};

fn cx() -> Cx {
    Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(sim_kernel::DefaultFactory),
    )
}

#[test]
fn hooked_shape_preserves_plain_inner_acceptance_when_hooks_pass() {
    let mut cx = cx();
    let shape = HookedShape::new(Arc::new(AnyShape), Vec::new());

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 0);
}

#[test]
fn mark_hooks_run_before_and_after_inner_in_deterministic_order() {
    let mut cx = cx();
    let log = Arc::new(Mutex::new(Vec::new()));
    let shape = HookedShape::new(
        Arc::new(RecordingShape::new(log.clone())),
        vec![
            Arc::new(RecordingMarkHook::new("first", log.clone())),
            Arc::new(RecordingMarkHook::new("second", log.clone())),
        ],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(matched.accepted);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "first:before",
            "second:before",
            "inner",
            "first:after",
            "second:after"
        ]
    );
    assert_eq!(
        diagnostics(&matched),
        vec![
            "shape-hook:mark first:BeforeInner",
            "shape-hook:mark second:BeforeInner",
            "shape-hook:mark first:AfterInner",
            "shape-hook:mark second:AfterInner",
        ]
    );
}

#[test]
fn value_checks_run_hooks_with_value_target() {
    let mut cx = cx();
    let targets = Arc::new(Mutex::new(Vec::new()));
    let shape = HookedShape::new(
        Arc::new(AnyShape),
        vec![Arc::new(TargetRecordingHook::new(targets.clone()))],
    );
    let value = cx.factory().bool(true).unwrap();

    let matched = shape.check_value(&mut cx, value).unwrap();

    assert!(matched.accepted);
    assert_eq!(
        targets.lock().unwrap().as_slice(),
        [MatchHookTargetKind::Value, MatchHookTargetKind::Value]
    );
}

#[test]
fn mark_hook_cannot_change_acceptance() {
    let mut cx = cx();
    let shape = HookedShape::new(
        Arc::new(RejectingShape::with_diagnostic()),
        vec![Arc::new(TraceMarkHook)],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert!(
        diagnostics(&matched)
            .iter()
            .any(|message| message.starts_with("shape-hook:mark"))
    );
}

#[test]
fn accept_hook_can_turn_rejection_into_acceptance() {
    let mut cx = cx();
    let shape = HookedShape::new(
        Arc::new(RejectingShape::quiet()),
        vec![Arc::new(AcceptOnNoDiagnosticsHook)],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 1);
    assert!(
        diagnostics(&matched)
            .iter()
            .any(|message| message.starts_with("shape-hook:accept"))
    );
}

#[test]
fn discard_hook_can_turn_acceptance_into_rejection() {
    let mut cx = cx();
    let shape = HookedShape::new(
        Arc::new(NoisyAcceptShape),
        vec![Arc::new(DiscardOnDiagnosticPrefixHook::new("inner:"))],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert_eq!(matched.score, MatchScore::reject());
    assert!(
        diagnostics(&matched)
            .iter()
            .any(|message| message.starts_with("shape-hook:discard"))
    );
}

#[test]
fn discard_hook_runs_after_accept_hook_and_can_veto_it() {
    let mut cx = cx();
    let shape = HookedShape::new(
        Arc::new(RejectingShape::quiet()),
        vec![
            Arc::new(AcceptOnNoDiagnosticsHook),
            Arc::new(DiscardOnDiagnosticPrefixHook::new("shape-hook:accept")),
        ],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
    assert!(
        diagnostics(&matched)
            .iter()
            .position(|message| message.starts_with("shape-hook:accept"))
            < diagnostics(&matched)
                .iter()
                .position(|message| message.starts_with("shape-hook:discard"))
    );
}

#[test]
fn annotate_hook_can_change_score_without_changing_acceptance() {
    let mut cx = cx();
    let shape = HookedShape::new(Arc::new(AnyShape), vec![Arc::new(ScoreFloorHook::new(50))]);

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(matched.accepted);
    assert_eq!(matched.score.value(), 50);
    assert!(
        diagnostics(&matched)
            .iter()
            .any(|message| message.starts_with("shape-hook:annotate"))
    );
}

#[test]
fn annotate_hook_cannot_change_acceptance() {
    let mut cx = cx();
    let shape = HookedShape::new(
        Arc::new(RejectingShape::with_diagnostic()),
        vec![Arc::new(ScoreFloorHook::new(50))],
    );

    let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();

    assert!(!matched.accepted);
}

#[test]
fn built_in_hook_values_have_stable_display() {
    let mut cx = cx();
    let value = hook_value(Arc::new(TraceMarkHook));

    let display = value.object().display(&mut cx).unwrap();

    assert_eq!(display, "#<shape-hook shape/trace-mark mark>");
}

fn diagnostics(matched: &ShapeMatch) -> Vec<&str> {
    matched
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

struct RecordingShape {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingShape {
    fn new(log: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self { log }
    }
}

impl Shape for RecordingShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        self.log.lock().unwrap().push("inner");
        Ok(ShapeMatch::accept(MatchScore::exact(3)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        self.log.lock().unwrap().push("inner");
        Ok(ShapeMatch::accept(MatchScore::exact(3)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("recording shape"))
    }
}

struct RecordingMarkHook {
    name: &'static str,
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingMarkHook {
    fn new(name: &'static str, log: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self { name, log }
    }
}

impl MatchHook for RecordingMarkHook {
    fn symbol(&self) -> sim_kernel::Symbol {
        sim_kernel::Symbol::qualified("test", self.name)
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Mark
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        ctx: &MatchHookContext,
        _current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        match ctx.phase {
            MatchHookPhase::BeforeInner => self.log.lock().unwrap().push(if self.name == "first" {
                "first:before"
            } else {
                "second:before"
            }),
            MatchHookPhase::AfterInner => self.log.lock().unwrap().push(if self.name == "first" {
                "first:after"
            } else {
                "second:after"
            }),
        }
        Ok(MatchHookDecision::Mark {
            message: format!("{}:{:?}", self.name, ctx.phase),
        })
    }
}

struct TargetRecordingHook {
    targets: Arc<Mutex<Vec<MatchHookTargetKind>>>,
}

impl TargetRecordingHook {
    fn new(targets: Arc<Mutex<Vec<MatchHookTargetKind>>>) -> Self {
        Self { targets }
    }
}

impl MatchHook for TargetRecordingHook {
    fn symbol(&self) -> sim_kernel::Symbol {
        sim_kernel::Symbol::qualified("test", "target-recording")
    }

    fn kind(&self) -> MatchHookKind {
        MatchHookKind::Mark
    }

    fn apply(
        &self,
        _cx: &mut Cx,
        ctx: &MatchHookContext,
        _current: Option<&ShapeMatch>,
    ) -> Result<MatchHookDecision> {
        self.targets.lock().unwrap().push(ctx.target_kind);
        Ok(MatchHookDecision::Mark {
            message: format!("{:?}", ctx.target_kind),
        })
    }
}

struct RejectingShape {
    diagnostics: bool,
}

impl RejectingShape {
    fn quiet() -> Self {
        Self { diagnostics: false }
    }

    fn with_diagnostic() -> Self {
        Self { diagnostics: true }
    }
}

impl Shape for RejectingShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(self.reject())
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(self.reject())
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("rejecting shape"))
    }
}

impl RejectingShape {
    fn reject(&self) -> ShapeMatch {
        ShapeMatch {
            accepted: false,
            captures: crate::Bindings::new(),
            score: MatchScore::reject(),
            diagnostics: self
                .diagnostics
                .then(|| Diagnostic::error("inner: rejected"))
                .into_iter()
                .collect(),
        }
    }
}

struct NoisyAcceptShape;

impl Shape for NoisyAcceptShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(self.accept())
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(self.accept())
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("noisy accept shape"))
    }
}

impl NoisyAcceptShape {
    fn accept(&self) -> ShapeMatch {
        let mut matched = ShapeMatch::accept(MatchScore::exact(7));
        matched
            .diagnostics
            .push(Diagnostic::info("inner: accepted with note"));
        matched
    }
}
