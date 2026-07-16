//! `HookedShape`: a shape that wraps an inner shape and runs an ordered list of
//! match hooks around each check to adjust acceptance, score, and diagnostics.

use std::sync::Arc;

use sim_kernel::{Cx, Diagnostic, Expr, Result, ShapeRef, Value, shape_is_subshape_of};

use crate::{
    MatchScore, Shape, ShapeDoc, ShapeMatch,
    hooks::types::{
        MatchHook, MatchHookContext, MatchHookDecision, MatchHookKind, MatchHookPhase,
        MatchHookTargetKind,
    },
};

/// Shape wrapper that runs neutral match hooks around an inner shape.
///
/// `HookedShape` keeps the kernel `Shape` trait unchanged. Mark hooks observe,
/// accept hooks can repair rejections, discard hooks can veto acceptances, and
/// annotate hooks can adjust score and diagnostics without changing acceptance.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{AnyShape, HookedShape, Shape, TraceMarkHook};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = HookedShape::new(Arc::new(AnyShape), vec![Arc::new(TraceMarkHook)]);
/// let matched = shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap();
///
/// assert!(matched.accepted);
/// assert!(matched
///     .diagnostics
///     .iter()
///     .any(|diagnostic| diagnostic.message.starts_with("shape-hook:mark")));
/// ```
pub struct HookedShape {
    inner: Arc<dyn Shape>,
    hooks: Vec<Arc<dyn MatchHook>>,
}

impl HookedShape {
    /// Wrap an inner shape with an ordered list of match hooks.
    pub fn new(inner: Arc<dyn Shape>, hooks: Vec<Arc<dyn MatchHook>>) -> Self {
        Self { inner, hooks }
    }

    /// Borrow the wrapped inner shape.
    pub fn inner(&self) -> &Arc<dyn Shape> {
        &self.inner
    }

    /// Borrow the hooks run around the inner shape, in registration order.
    pub fn hooks(&self) -> &[Arc<dyn MatchHook>] {
        &self.hooks
    }

    fn acceptance_transparent(&self) -> bool {
        self.hooks
            .iter()
            .all(|hook| hook.kind().preserves_acceptance())
    }

    fn matches_transparent_hook_stack(&self, other: &Self) -> bool {
        self.acceptance_transparent()
            && other.acceptance_transparent()
            && self.hooks.len() == other.hooks.len()
            && self
                .hooks
                .iter()
                .zip(other.hooks.iter())
                .all(|(left, right)| {
                    left.kind() == right.kind()
                        && left.kind().preserves_acceptance()
                        && left.symbol() == right.symbol()
                        && left.object_encoding() == right.object_encoding()
                })
    }
}

impl Shape for HookedShape {
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        // Hook wrappers must not leak the inner shape's parent lattice into
        // the generic walk; conservative subshape proofs go through the
        // wrapper's own override instead.
        Ok(Vec::new())
    }

    fn is_effectful(&self) -> bool {
        self.inner.is_effectful()
    }

    fn is_total(&self) -> bool {
        self.acceptance_transparent() && self.inner.is_total()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        if !self.matches_transparent_hook_stack(parent) {
            return Ok(None);
        }
        shape_is_subshape_of(cx, self.inner.as_ref(), parent.inner.as_ref()).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let label = self.inner.describe(cx)?.name;
        let before = self.run_marks(
            cx,
            MatchHookTargetKind::Value,
            MatchHookPhase::BeforeInner,
            &label,
            None,
        )?;
        let matched = self.inner.check_value(cx, value)?;
        self.finish_match(cx, MatchHookTargetKind::Value, label, matched, before)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let label = self.inner.describe(cx)?.name;
        let before = self.run_marks(
            cx,
            MatchHookTargetKind::Expr,
            MatchHookPhase::BeforeInner,
            &label,
            None,
        )?;
        let matched = self.inner.check_expr(cx, expr)?;
        self.finish_match(cx, MatchHookTargetKind::Expr, label, matched, before)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("hooked shape").with_detail(self.inner.describe(cx)?.name);
        for hook in &self.hooks {
            doc = doc.with_detail(hook.symbol().to_string());
        }
        Ok(doc)
    }
}

impl HookedShape {
    fn finish_match(
        &self,
        cx: &mut Cx,
        target_kind: MatchHookTargetKind,
        label: String,
        mut matched: ShapeMatch,
        before_marks: Vec<Diagnostic>,
    ) -> Result<ShapeMatch> {
        matched.diagnostics.extend(before_marks);
        let after_marks = self.run_marks(
            cx,
            target_kind,
            MatchHookPhase::AfterInner,
            &label,
            Some(&matched),
        )?;
        matched.diagnostics.extend(after_marks);

        if !matched.accepted {
            matched = self.run_accept_hooks(cx, target_kind, &label, matched)?;
        }
        if matched.accepted {
            matched = self.run_discard_hooks(cx, target_kind, &label, matched)?;
        }
        self.run_annotate_hooks(cx, target_kind, &label, matched)
    }

    fn run_marks(
        &self,
        cx: &mut Cx,
        target_kind: MatchHookTargetKind,
        phase: MatchHookPhase,
        shape_label: &str,
        current: Option<&ShapeMatch>,
    ) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        for (hook_index, hook) in self.hooks.iter().enumerate() {
            if hook.kind() != MatchHookKind::Mark {
                continue;
            }
            let ctx = MatchHookContext {
                hook_index,
                phase,
                target_kind,
                shape_label: shape_label.to_owned(),
            };
            if let MatchHookDecision::Mark { message } = hook.apply(cx, &ctx, current)? {
                diagnostics.push(Diagnostic::info(format!("shape-hook:mark {message}")));
            }
        }
        Ok(diagnostics)
    }

    fn run_accept_hooks(
        &self,
        cx: &mut Cx,
        target_kind: MatchHookTargetKind,
        shape_label: &str,
        mut matched: ShapeMatch,
    ) -> Result<ShapeMatch> {
        for (hook_index, hook) in self.hooks.iter().enumerate() {
            if hook.kind() != MatchHookKind::Accept {
                continue;
            }
            let ctx = MatchHookContext {
                hook_index,
                phase: MatchHookPhase::AfterInner,
                target_kind,
                shape_label: shape_label.to_owned(),
            };
            if let MatchHookDecision::Accept { reason, score } =
                hook.apply(cx, &ctx, Some(&matched))?
            {
                matched.accepted = true;
                if matched.score == MatchScore::reject() {
                    matched.score = score;
                }
                matched
                    .diagnostics
                    .push(Diagnostic::info(format!("shape-hook:accept {reason}")));
            }
        }
        Ok(matched)
    }

    fn run_discard_hooks(
        &self,
        cx: &mut Cx,
        target_kind: MatchHookTargetKind,
        shape_label: &str,
        mut matched: ShapeMatch,
    ) -> Result<ShapeMatch> {
        for (hook_index, hook) in self.hooks.iter().enumerate() {
            if hook.kind() != MatchHookKind::Discard {
                continue;
            }
            let ctx = MatchHookContext {
                hook_index,
                phase: MatchHookPhase::AfterInner,
                target_kind,
                shape_label: shape_label.to_owned(),
            };
            if let MatchHookDecision::Discard { reason } = hook.apply(cx, &ctx, Some(&matched))? {
                matched.accepted = false;
                matched.score = MatchScore::reject();
                matched
                    .diagnostics
                    .push(Diagnostic::error(format!("shape-hook:discard {reason}")));
                break;
            }
        }
        Ok(matched)
    }

    fn run_annotate_hooks(
        &self,
        cx: &mut Cx,
        target_kind: MatchHookTargetKind,
        shape_label: &str,
        mut matched: ShapeMatch,
    ) -> Result<ShapeMatch> {
        for (hook_index, hook) in self.hooks.iter().enumerate() {
            if hook.kind() != MatchHookKind::Annotate {
                continue;
            }
            let ctx = MatchHookContext {
                hook_index,
                phase: MatchHookPhase::AfterInner,
                target_kind,
                shape_label: shape_label.to_owned(),
            };
            if let MatchHookDecision::Annotate {
                message,
                score_delta,
            } = hook.apply(cx, &ctx, Some(&matched))?
            {
                matched.score += MatchScore::exact(score_delta);
                matched
                    .diagnostics
                    .push(Diagnostic::info(format!("shape-hook:annotate {message}")));
            }
        }
        Ok(matched)
    }
}
