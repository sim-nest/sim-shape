//! Overload selection for shape-typed function cases.

use std::cmp::Ordering;

use sim_kernel::{Cx, Diagnostic, PreparedArgs, Result, shape_is_subshape_of};

use super::{FunctionCase, FunctionObject, SelectedCase};
use crate::diagnostics::{callable_mismatch_diagnostic, overload_selection_diagnostic};

impl FunctionObject {
    /// Pick the overload case that best matches the prepared arguments.
    ///
    /// Cases whose argument shape accepts the arguments are ordered by priority
    /// first, then by proven argument-shape specificity (the subshape lattice),
    /// and only then by additive match score: a case whose argument shape is a
    /// proven subshape of another's is strictly more specific and wins even at
    /// an equal or lower score. Returns [`Error::NoMatchingOverload`] when
    /// nothing matches and [`Error::AmbiguousOverload`] when two top cases
    /// remain unordered (equal priority, neither shape subsumes the other, and
    /// equal score).
    ///
    /// [`Error::NoMatchingOverload`]: sim_kernel::Error::NoMatchingOverload
    /// [`Error::AmbiguousOverload`]: sim_kernel::Error::AmbiguousOverload
    pub fn select_case<'a>(
        &'a self,
        cx: &mut Cx,
        prepared: &PreparedArgs,
    ) -> Result<SelectedCase<'a>> {
        let cases = self.cases.iter().collect::<Vec<_>>();
        let (matches, diagnostics) = self.collect_matches(cx, prepared, &cases)?;
        self.select_best_case(cx, matches, diagnostics)
    }

    pub(in crate::functions) fn collect_matches<'a>(
        &'a self,
        cx: &mut Cx,
        prepared: &PreparedArgs,
        cases: &[&'a FunctionCase],
    ) -> Result<(Vec<SelectedCase<'a>>, Vec<Diagnostic>)> {
        let mut matches = Vec::new();
        let mut diagnostics = Vec::new();
        let args = cx.new_list(prepared.values().to_vec())?;

        for case in cases {
            let matched = case.args.check_value(cx, args.clone())?;
            if matched.accepted {
                matches.push(SelectedCase {
                    case,
                    match_result: matched,
                });
            } else {
                diagnostics.extend(matched.diagnostics);
            }
        }

        Ok((matches, diagnostics))
    }

    pub(in crate::functions) fn select_best_case<'a>(
        &'a self,
        cx: &mut Cx,
        mut matches: Vec<SelectedCase<'a>>,
        mut diagnostics: Vec<Diagnostic>,
    ) -> Result<SelectedCase<'a>> {
        if matches.is_empty() {
            diagnostics.insert(
                0,
                overload_selection_diagnostic(&self.symbol, "no matching overload case"),
            );
            diagnostics.push(callable_mismatch_diagnostic(
                &self.symbol,
                "an applicable case",
                "rejected arguments",
            ));
            return Err(sim_kernel::Error::NoMatchingOverload {
                function: self.id,
                diagnostics,
            });
        }

        // Order by priority then additive score as a stable starting point,
        // then refine with the proven subshape lattice so a strictly more
        // specific overload is preferred over a tying or higher-scoring one.
        matches.sort_by(|left, right| {
            right
                .case
                .priority
                .cmp(&left.case.priority)
                .then_with(|| right.match_result.score.cmp(&left.match_result.score))
        });

        let mut best = 0usize;
        for index in 1..matches.len() {
            if self
                .compare_cases(cx, &matches[index], &matches[best])?
                .is_gt()
            {
                best = index;
            }
        }

        // The selection is ambiguous only when another accepted case cannot be
        // ordered against the best one: equal priority, neither argument shape
        // a proven subshape of the other, and an equal additive score.
        let mut candidates = vec![matches[best].case.id];
        for index in 0..matches.len() {
            if index != best
                && self
                    .compare_cases(cx, &matches[best], &matches[index])?
                    .is_eq()
            {
                candidates.push(matches[index].case.id);
            }
        }
        if candidates.len() > 1 {
            cx.push_diagnostic(overload_selection_diagnostic(
                &self.symbol,
                "ambiguous top-ranked overload cases",
            ));
            return Err(sim_kernel::Error::AmbiguousOverload {
                function: self.id,
                candidates,
            });
        }

        Ok(matches[best].clone())
    }

    /// Order two accepted cases: priority, then proven subshape specificity,
    /// then additive match score.
    ///
    /// `Greater` means `a` is preferred over `b`. A case whose argument shape
    /// is a proven subshape of the other's is strictly more specific and wins
    /// before the additive score is consulted; `Equal` means the runtime could
    /// not order the two and selection is ambiguous.
    fn compare_cases(
        &self,
        cx: &mut Cx,
        a: &SelectedCase<'_>,
        b: &SelectedCase<'_>,
    ) -> Result<Ordering> {
        match a.case.priority.cmp(&b.case.priority) {
            Ordering::Equal => {}
            other => return Ok(other),
        }

        let a_subshape = shape_is_subshape_of(cx, a.case.args.as_ref(), b.case.args.as_ref())?;
        let b_subshape = shape_is_subshape_of(cx, b.case.args.as_ref(), a.case.args.as_ref())?;
        match (a_subshape, b_subshape) {
            (true, false) => return Ok(Ordering::Greater),
            (false, true) => return Ok(Ordering::Less),
            _ => {}
        }

        Ok(a.match_result.score.cmp(&b.match_result.score))
    }
}
