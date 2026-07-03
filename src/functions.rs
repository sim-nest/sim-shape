//! The callable shape object: function objects with shape-typed cases,
//! overload selection across those cases, and shape-as-value wrapping.

use std::cmp::Ordering;
use std::sync::Arc;

mod shape_object;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Demand, FunctionId, Object, PreparedArgs, RawArgs,
    ReadConstructor, Result, ShapeId, ShapeRef, Symbol, Value, shape_is_subshape_of,
};

use crate::base::{Bindings, Shape, ShapeMatch};
use crate::diagnostics::{callable_mismatch_diagnostic, overload_selection_diagnostic};
use crate::primitives::OneOfShape;
pub use shape_object::{ShapeObject, shape_value, shape_value_with_encoding};

/// Native implementation backing a single [`FunctionCase`].
///
/// Invoked with the forced/prepared arguments and the bindings captured while
/// the case's argument shape matched.
pub type NativeFunctionImpl = fn(&mut Cx, &PreparedArgs, Bindings) -> Result<Value>;

/// One overload case of a [`FunctionObject`]: a shape-typed signature paired
/// with the native code that runs when it is selected.
#[derive(Clone)]
pub struct FunctionCase {
    /// Stable identifier for this case within the function.
    pub id: sim_kernel::CaseId,
    /// Symbol naming the case.
    pub name: Symbol,
    /// Shape the argument list must match for this case to apply.
    pub args: Arc<dyn Shape>,
    /// Optional shape the result is checked against after the call.
    pub result: Option<Arc<dyn Shape>>,
    /// Per-argument evaluation demand (how far each argument is forced).
    pub demand: Vec<sim_kernel::Demand>,
    /// Tie-break priority; higher wins before match score is consulted.
    pub priority: i32,
    /// Native code run when this case is selected.
    pub implementation: NativeFunctionImpl,
}

/// A callable function object: a named set of shape-typed overload cases with
/// selection driven by case priority and argument match score.
#[derive(Clone)]
pub struct FunctionObject {
    /// Stable function identifier.
    pub id: FunctionId,
    /// Symbol naming the function.
    pub symbol: Symbol,
    /// Overload cases in registration order.
    pub cases: Vec<FunctionCase>,
}

/// The case chosen by overload selection together with its match result.
#[derive(Clone)]
pub struct SelectedCase<'a> {
    /// The selected overload case.
    pub case: &'a FunctionCase,
    /// The match (score, captures, diagnostics) that selected it.
    pub match_result: ShapeMatch,
}

impl FunctionObject {
    /// Build a function object from an id, symbol, and its overload cases.
    pub fn new(id: FunctionId, symbol: Symbol, cases: Vec<FunctionCase>) -> Self {
        Self { id, symbol, cases }
    }

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
        let mut matches = Vec::new();
        let mut diagnostics = Vec::new();
        let args = cx.new_list(prepared.values().to_vec())?;

        for case in &self.cases {
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

    /// Shape accepting any case's arguments: the lone case's shape, or a
    /// one-of over every case. `None` when the function has no cases.
    pub fn combined_args_shape(&self) -> Option<Arc<dyn Shape>> {
        match self.cases.as_slice() {
            [] => None,
            [one] => Some(one.args.clone()),
            many => Some(Arc::new(OneOfShape::new(
                many.iter().map(|case| case.args.clone()).collect(),
            ))),
        }
    }

    /// Shape covering every case's result, or `None` if any case omits a
    /// result shape. A single result is returned directly; many become a
    /// one-of.
    pub fn combined_result_shape(&self) -> Option<Arc<dyn Shape>> {
        let shapes = self
            .cases
            .iter()
            .map(|case| case.result.clone())
            .collect::<Option<Vec<_>>>()?;
        match shapes.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            many => Some(Arc::new(OneOfShape::new(many.to_vec()))),
        }
    }

    /// Evaluation demand declared for argument `index` across all cases.
    ///
    /// Returns the shared demand when every case agrees, [`Demand::Value`] when
    /// they disagree, and `None` when no case declares that position.
    pub fn declared_demand(&self, index: usize) -> Option<Demand> {
        let mut declared = None;
        for case in &self.cases {
            let case_demand = case.demand.get(index).copied().unwrap_or(Demand::Value);
            match declared {
                None => declared = Some(case_demand),
                Some(existing) if existing == case_demand => {}
                Some(_) => return Some(Demand::Value),
            }
        }
        declared
    }

    /// Per-position demands for the call, sized to the widest case and
    /// defaulting unspecified positions to [`Demand::Value`].
    pub fn declared_demands(&self) -> Vec<Demand> {
        let max_len = self
            .cases
            .iter()
            .map(|case| case.demand.len())
            .max()
            .unwrap_or(0);
        (0..max_len)
            .map(|index| self.declared_demand(index).unwrap_or(Demand::Value))
            .collect()
    }
}

impl Object for FunctionObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for FunctionObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let mut entries = vec![
            (
                Symbol::new("symbol"),
                cx.factory().string(self.symbol.to_string())?,
            ),
            (
                Symbol::new("case-count"),
                cx.factory().number_literal(
                    Symbol::qualified("numbers", "f64"),
                    self.cases.len().to_string(),
                )?,
            ),
        ];
        for (index, case) in self.cases.iter().enumerate() {
            entries.push((
                Symbol::qualified("case", case.name.name.clone()),
                cx.factory().string(case.name.to_string())?,
            ));
            let args_doc = case.args.describe(cx)?;
            entries.push((
                Symbol::qualified("case-args", index.to_string()),
                cx.factory().string(args_doc.name)?,
            ));
            if let Some(result) = &case.result {
                let result_doc = result.describe(cx)?;
                entries.push((
                    Symbol::qualified("case-result", index.to_string()),
                    cx.factory().string(result_doc.name)?,
                ));
            }
            if !case.demand.is_empty() {
                entries.push((
                    Symbol::qualified("case-demand", index.to_string()),
                    cx.factory().list(
                        case.demand
                            .iter()
                            .map(|demand| {
                                let name = match demand {
                                    Demand::Never => "never",
                                    Demand::Bool => "bool",
                                    Demand::Value => "value",
                                    Demand::Expr => "expr",
                                    Demand::Class(_) => "class",
                                    Demand::Shape(_) => "shape",
                                };
                                cx.factory().symbol(Symbol::new(name))
                            })
                            .collect::<Result<Vec<_>>>()?,
                    )?,
                ));
            }
        }
        cx.factory().table(entries)
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        Some(self)
    }
}

impl Callable for FunctionObject {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let prepared = PreparedArgs::new(args.into_vec());
        let selected = self.select_case(cx, &prepared)?;
        let prepared = refine_prepared_args(cx, &prepared, selected.case)?;
        let bindings = selected.match_result.captures;
        let env = bindings.clone().into_child_env(cx)?;
        let result = cx.with_env(env, |cx| {
            (selected.case.implementation)(cx, &prepared, bindings)
        })?;

        if let Some(shape) = &selected.case.result {
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

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(self
            .combined_args_shape()
            .map(|shape| shape_value(Symbol::qualified(self.symbol.to_string(), "args"), shape)))
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(self
            .combined_result_shape()
            .map(|shape| shape_value(Symbol::qualified(self.symbol.to_string(), "result"), shape)))
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let prepared =
            cx.eval_policy_ref()
                .prepare_call_args(cx, args, &self.declared_demands())?;
        let selected = self.select_case(cx, &prepared)?;
        let prepared = refine_prepared_args(cx, &prepared, selected.case)?;
        let bindings = selected.match_result.captures;
        let env = bindings.clone().into_child_env(cx)?;
        let result = cx.with_env(env, |cx| {
            (selected.case.implementation)(cx, &prepared, bindings)
        })?;

        if let Some(shape) = &selected.case.result {
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
}

fn refine_prepared_args(
    cx: &mut Cx,
    prepared: &PreparedArgs,
    case: &FunctionCase,
) -> Result<PreparedArgs> {
    let mut values = Vec::with_capacity(prepared.len());
    for index in 0..prepared.len() {
        let value = prepared
            .get(index)
            .cloned()
            .ok_or_else(|| sim_kernel::Error::Eval(format!("missing prepared arg {index}")))?;
        let demand = case.demand.get(index).copied().unwrap_or(Demand::Value);
        values.push(force_for_case_demand(cx, value, demand)?);
    }
    Ok(PreparedArgs::new(values))
}

fn force_for_case_demand(cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
    match demand {
        Demand::Shape(shape_id) => {
            let value = cx.force(value, Demand::Value)?;
            let shape_value = cx
                .registry()
                .shape_value(shape_id)
                .cloned()
                .ok_or_else(|| sim_kernel::Error::WrongShape {
                    expected: shape_id,
                    diagnostics: Vec::new(),
                })?;
            let shape = shape_value
                .object()
                .as_shape()
                .ok_or(sim_kernel::Error::TypeMismatch {
                    expected: "shape object",
                    found: "non-shape object",
                })?;
            let matched = shape.check_value(cx, value.clone())?;
            if matched.accepted {
                Ok(value)
            } else {
                Err(sim_kernel::Error::WrongShape {
                    expected: shape_id,
                    diagnostics: matched.diagnostics,
                })
            }
        }
        other => cx.force(value, other),
    }
}

impl ReadConstructor for FunctionObject {
    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn args_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        match self.combined_args_shape() {
            Some(shape) => Ok(shape_value(
                Symbol::qualified(self.symbol.to_string(), "args-shape"),
                shape,
            )),
            None => cx.factory().nil(),
        }
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        self.call(cx, Args::new(args))
    }
}

/// Merge several functions into one whose cases are the union of theirs.
///
/// The result is a fresh [`FunctionObject`] with a generated `overload:` symbol
/// and a new function id; selection then ranks across all combined cases.
pub fn overload(cx: &mut Cx, functions: Vec<FunctionObject>) -> Result<FunctionObject> {
    let mut cases = Vec::new();
    let mut names = Vec::new();

    for function in functions {
        names.push(function.symbol.to_string());
        cases.extend(function.cases);
    }

    let symbol = Symbol::new(format!("overload:{}", names.join("+")));
    Ok(FunctionObject {
        id: cx.registry_mut().fresh_function_id(),
        symbol,
        cases,
    })
}

/// Borrow the overload cases of a function object.
pub fn function_cases(function: &FunctionObject) -> &[FunctionCase] {
    &function.cases
}

/// Borrow the argument shape of a single case.
pub fn case_shape(case: &FunctionCase) -> &dyn Shape {
    case.args.as_ref()
}

/// Borrow the result shape of a case, if it declares one.
pub fn case_result_shape(case: &FunctionCase) -> Option<&dyn Shape> {
    case.result.as_deref()
}
