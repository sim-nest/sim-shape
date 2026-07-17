use sim_kernel::{CaseId, Cx, Demand, PreparedArgs, RawArgs, Result, ShapeId, Value};

use super::{FunctionCase, FunctionObject, SelectedCase, refine_prepared_args};
use crate::diagnostics::{callable_mismatch_diagnostic, overload_selection_diagnostic};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DemandConflict {
    index: usize,
    cases: Vec<CaseId>,
}

#[derive(Clone)]
struct DemandPlanGroup<'a> {
    plan: Vec<Demand>,
    cases: Vec<&'a FunctionCase>,
}

#[derive(Clone)]
struct MatchedDemandGroup<'a> {
    plan: Vec<Demand>,
    prepared: PreparedArgs,
    selected: SelectedCase<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemandPrepKind {
    Syntax,
    Value,
}

impl FunctionObject {
    pub(super) fn call_exprs_with_demands(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let arity = args.exprs().len();
        let mut mismatch_diagnostics = Vec::new();
        let mut deferred_error = None;

        for priority_cases in self.priority_case_groups() {
            let groups = demand_plan_groups(&priority_cases, arity);
            if let Some(conflict) = incomparable_plan_conflict(&groups) {
                return self.demand_conflict_error(cx, conflict);
            }

            let mut matched_groups = Vec::new();
            let mut priority_error = None;

            for group in groups {
                if let Some(conflict) = matched_plan_conflict(&matched_groups, &group) {
                    return self.demand_conflict_error(cx, conflict);
                }

                let prepared =
                    match cx
                        .eval_policy_ref()
                        .prepare_call_args(cx, args.clone(), &group.plan)
                    {
                        Ok(prepared) => prepared,
                        Err(err) => {
                            priority_error.get_or_insert(err);
                            continue;
                        }
                    };
                let (matches, diagnostics) = self.collect_matches(cx, &prepared, &group.cases)?;
                if matches.is_empty() {
                    mismatch_diagnostics.extend(diagnostics);
                    continue;
                }
                let selected = self.select_best_case(cx, matches, diagnostics)?;
                matched_groups.push(MatchedDemandGroup {
                    plan: group.plan,
                    prepared,
                    selected,
                });
            }

            match matched_groups.as_slice() {
                [] => {
                    if deferred_error.is_none() {
                        deferred_error = priority_error;
                    }
                    continue;
                }
                [matched] => return self.call_matched_group(cx, matched),
                _ => return self.demand_conflict_error(cx, demand_conflict(&matched_groups)),
            }
        }

        if let Some(err) = deferred_error {
            return Err(err);
        }

        mismatch_diagnostics.insert(
            0,
            overload_selection_diagnostic(&self.symbol, "no matching overload case"),
        );
        mismatch_diagnostics.push(callable_mismatch_diagnostic(
            &self.symbol,
            "an applicable case",
            "rejected arguments",
        ));
        Err(sim_kernel::Error::NoMatchingOverload {
            function: self.id,
            diagnostics: mismatch_diagnostics,
        })
    }

    fn call_matched_group(&self, cx: &mut Cx, matched: &MatchedDemandGroup<'_>) -> Result<Value> {
        let prepared = refine_prepared_args(cx, &matched.prepared, matched.selected.case)?;
        let bindings = matched.selected.match_result.captures.clone();
        let env = bindings.clone().into_child_env(cx)?;
        let result = cx.with_env(env, |cx| {
            (matched.selected.case.implementation)(cx, &prepared, bindings)
        })?;

        if let Some(shape) = &matched.selected.case.result {
            let matched = shape.check_value(cx, result.clone())?;
            if !matched.accepted {
                return Err(sim_kernel::Error::WrongShape {
                    expected: shape.id().unwrap_or(ShapeId(0)),
                    diagnostics: matched.diagnostics,
                });
            }
        }

        Ok(result)
    }

    fn priority_case_groups(&self) -> Vec<Vec<&FunctionCase>> {
        let mut cases = self.cases.iter().collect::<Vec<_>>();
        cases.sort_by(|left, right| right.priority.cmp(&left.priority));

        let mut groups = Vec::<Vec<&FunctionCase>>::new();
        for case in cases {
            match groups.last_mut() {
                Some(group) if group[0].priority == case.priority => group.push(case),
                _ => groups.push(vec![case]),
            }
        }
        groups
    }

    fn demand_conflict_error<T>(&self, cx: &mut Cx, conflict: DemandConflict) -> Result<T> {
        let case_ids = conflict
            .cases
            .iter()
            .map(|case| format!("{case:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let reason = format!(
            "incompatible demand plans at argument {} across cases {}",
            conflict.index, case_ids
        );
        let diagnostic = overload_selection_diagnostic(&self.symbol, reason);
        let message = diagnostic.message.clone();
        cx.push_diagnostic(diagnostic);
        Err(sim_kernel::Error::Eval(message))
    }

    #[cfg(test)]
    pub(super) fn demand_plan(&self) -> std::result::Result<Vec<Demand>, DemandConflict> {
        let width = self
            .cases
            .iter()
            .map(|case| case.demand.len())
            .max()
            .unwrap_or(0);
        let cases = self.cases.iter().collect::<Vec<_>>();
        demand_plan_for_cases(&cases, width)
    }
}

fn demand_plan_groups<'a>(cases: &[&'a FunctionCase], arity: usize) -> Vec<DemandPlanGroup<'a>> {
    let mut groups = Vec::<DemandPlanGroup<'a>>::new();
    for case in cases {
        let plan = demand_plan_for_cases(&[*case], arity)
            .expect("single-case demand plans are compatible");
        if let Some(group) = groups.iter_mut().find(|group| group.plan == plan) {
            group.cases.push(case);
        } else {
            groups.push(DemandPlanGroup {
                plan,
                cases: vec![case],
            });
        }
    }
    groups.sort_by_key(|group| forcing_score(&group.plan));
    groups
}

fn demand_plan_for_cases(
    cases: &[&FunctionCase],
    width: usize,
) -> std::result::Result<Vec<Demand>, DemandConflict> {
    let mut plan = Vec::with_capacity(width);
    for index in 0..width {
        let mut kind = None;
        for case in cases {
            let case_kind = demand_prep_kind(case_demand(case, index));
            match kind {
                None => kind = Some(case_kind),
                Some(existing) if existing == case_kind => {}
                Some(_) => {
                    return Err(DemandConflict {
                        index,
                        cases: case_ids(cases.iter().copied()),
                    });
                }
            }
        }
        plan.push(kind.map(DemandPrepKind::plan).unwrap_or(Demand::Value));
    }
    Ok(plan)
}

fn incomparable_plan_conflict(groups: &[DemandPlanGroup<'_>]) -> Option<DemandConflict> {
    for (left_index, left) in groups.iter().enumerate() {
        for right in groups.iter().skip(left_index + 1) {
            if has_extra_forcing(&left.plan, &right.plan)
                && has_extra_forcing(&right.plan, &left.plan)
            {
                return Some(conflict_between_groups(left, right));
            }
        }
    }
    None
}

fn matched_plan_conflict(
    matched_groups: &[MatchedDemandGroup<'_>],
    group: &DemandPlanGroup<'_>,
) -> Option<DemandConflict> {
    matched_groups
        .iter()
        .find(|matched| has_extra_forcing(&group.plan, &matched.plan))
        .map(|matched| conflict_between_matched_and_group(matched, group))
}

fn has_extra_forcing(left: &[Demand], right: &[Demand]) -> bool {
    left.iter()
        .zip(right.iter())
        .any(|(left, right)| *left == Demand::Value && *right == Demand::Expr)
}

fn forcing_score(plan: &[Demand]) -> usize {
    plan.iter()
        .filter(|demand| **demand == Demand::Value)
        .count()
}

fn conflict_between_groups(
    left: &DemandPlanGroup<'_>,
    right: &DemandPlanGroup<'_>,
) -> DemandConflict {
    DemandConflict {
        index: differing_demand_index(&left.plan, &right.plan),
        cases: case_ids(left.cases.iter().chain(right.cases.iter()).copied()),
    }
}

fn conflict_between_matched_and_group(
    matched: &MatchedDemandGroup<'_>,
    group: &DemandPlanGroup<'_>,
) -> DemandConflict {
    let mut cases = vec![matched.selected.case.id];
    for case in &group.cases {
        push_unique_case(&mut cases, case.id);
    }
    DemandConflict {
        index: differing_demand_index(&matched.plan, &group.plan),
        cases,
    }
}

fn demand_conflict(groups: &[MatchedDemandGroup<'_>]) -> DemandConflict {
    let mut cases = Vec::new();
    for group in groups {
        push_unique_case(&mut cases, group.selected.case.id);
    }

    let baseline = &groups[0].plan;
    let index = groups
        .iter()
        .skip(1)
        .map(|group| differing_demand_index(baseline, &group.plan))
        .next()
        .unwrap_or(0);

    DemandConflict { index, cases }
}

fn differing_demand_index(left: &[Demand], right: &[Demand]) -> usize {
    left.iter()
        .zip(right.iter())
        .position(|(left, right)| left != right)
        .unwrap_or(0)
}

fn case_ids<'a>(cases: impl Iterator<Item = &'a FunctionCase>) -> Vec<CaseId> {
    let mut ids = Vec::new();
    for case in cases {
        push_unique_case(&mut ids, case.id);
    }
    ids
}

fn push_unique_case(cases: &mut Vec<CaseId>, case: CaseId) {
    if !cases.contains(&case) {
        cases.push(case);
    }
}

fn case_demand(case: &FunctionCase, index: usize) -> Demand {
    case.demand.get(index).copied().unwrap_or(Demand::Value)
}

fn demand_prep_kind(demand: Demand) -> DemandPrepKind {
    match demand {
        Demand::Never | Demand::Expr => DemandPrepKind::Syntax,
        Demand::Value | Demand::Bool | Demand::Class(_) | Demand::Shape(_) => DemandPrepKind::Value,
    }
}

impl DemandPrepKind {
    fn plan(self) -> Demand {
        match self {
            Self::Syntax => Demand::Expr,
            Self::Value => Demand::Value,
        }
    }
}
