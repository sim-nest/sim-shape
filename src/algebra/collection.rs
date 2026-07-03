//! Collection shapes: `TableShape` with its per-field specs and extra-field
//! policy, and `RepeatShape` for matching repeated occurrences of a shape.

use std::sync::Arc;

use sim_kernel::{
    Cx, Diagnostic, Expr, Result, Symbol, Table, Value, force_list_to_vec, shape_is_subshape_of,
};

use crate::{
    algebra::{capture_symbol, number_expr, number_value, symbol_list_expr, symbol_list_value},
    base::{Bindings, MatchScore, Shape, ShapeDoc, ShapeMatch},
};

/// Shape for table values or map expressions with named field constraints.
///
/// Required fields must be present and accepted by their field shapes. Extra
/// fields are controlled by [`TableExtraPolicy`].
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Symbol};
/// # use sim_shape::{ExprKind, ExprKindShape, Shape, TableShape};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = TableShape::single(
///     Symbol::new("ok"),
///     Arc::new(ExprKindShape::new(ExprKind::Bool)),
/// );
/// let expr = Expr::Map(vec![(Expr::Symbol(Symbol::new("ok")), Expr::Bool(true))]);
///
/// assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);
/// ```
#[derive(Clone)]
pub struct TableShape {
    fields: Vec<TableFieldSpec>,
    extra: TableExtraPolicy,
}

/// One field constraint inside a [`TableShape`].
#[derive(Clone)]
pub struct TableFieldSpec {
    /// Symbol key to look up in the table or map expression.
    pub key: Symbol,
    /// Shape that must accept the field value or expression.
    pub shape: Arc<dyn Shape>,
    /// Whether the field must be present.
    pub required: bool,
}

/// Policy for keys not listed in a [`TableShape`].
#[derive(Clone)]
pub enum TableExtraPolicy {
    /// Accept extra keys without checking their values.
    Allow,
    /// Reject any extra key.
    Reject,
    /// Check each extra value with the supplied shape.
    Shape(Arc<dyn Shape>),
}

impl TableShape {
    /// Build a table shape requiring a single named field, allowing extras.
    pub fn single(key: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self::new(
            vec![TableFieldSpec {
                key,
                shape,
                required: true,
            }],
            TableExtraPolicy::Allow,
        )
    }

    /// Build a table shape from explicit field specs and an extra-key policy.
    pub fn new(fields: Vec<TableFieldSpec>, extra: TableExtraPolicy) -> Self {
        Self { fields, extra }
    }

    /// Return the field constraints in declaration order.
    pub fn fields(&self) -> &[TableFieldSpec] {
        &self.fields
    }

    /// Return the policy applied to keys not listed in the field specs.
    pub fn extra(&self) -> &TableExtraPolicy {
        &self.extra
    }
}

impl Shape for TableShape {
    fn is_total(&self) -> bool {
        self.fields.is_empty() && matches!(self.extra, TableExtraPolicy::Allow)
    }

    fn is_effectful(&self) -> bool {
        self.fields.iter().any(|field| field.shape.is_effectful())
            || matches!(&self.extra, TableExtraPolicy::Shape(shape) if shape.is_effectful())
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };

        for parent_field in parent.fields() {
            if !parent_field.required {
                continue;
            }
            let Some(field) = self
                .fields
                .iter()
                .find(|candidate| candidate.key == parent_field.key)
            else {
                return Ok(None);
            };
            if !field.required {
                return Ok(None);
            }
            if !shape_is_subshape_of(cx, field.shape.as_ref(), parent_field.shape.as_ref())? {
                return Ok(None);
            }
        }

        if extra_policy_at_least_as_strict(cx, &self.extra, &parent.extra)? {
            Ok(Some(true))
        } else {
            Ok(None)
        }
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if let Some(table) = value.object().as_table_impl() {
            return self.check_table_value(cx, table);
        }

        let table_value = value.object().as_table(cx)?;
        let Some(table) = table_value.object().as_table_impl() else {
            return Ok(ShapeMatch::reject("shape-table: expected table"));
        };
        self.check_table_value(cx, table)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let Expr::Map(entries) = expr else {
            return Ok(ShapeMatch::reject("shape-table: expected map expression"));
        };

        let mut parsed = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let Expr::Symbol(key) = key else {
                return Ok(ShapeMatch::reject("shape-table: map key must be symbol"));
            };
            parsed.push((key.clone(), value.clone()));
        }
        self.check_map_expr(cx, &parsed)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = ShapeDoc::new("table shape");
        for field in &self.fields {
            doc = doc.with_detail(format!("{}: {}", field.key, field.shape.describe(cx)?.name));
        }
        Ok(doc)
    }
}

/// Shape for homogeneous list-like values and collection expressions.
///
/// `RepeatShape` checks every item with the body shape and can enforce minimum
/// and maximum item counts.
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
/// # use sim_shape::{ExprKind, ExprKindShape, RepeatShape, Shape};
/// # let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let shape = RepeatShape::with_bounds(
///     Arc::new(ExprKindShape::new(ExprKind::Bool)),
///     1,
///     Some(2),
/// );
///
/// assert!(shape
///     .check_expr(&mut cx, &Expr::Vector(vec![Expr::Bool(true)]))
///     .unwrap()
///     .accepted);
/// ```
pub struct RepeatShape {
    body: Arc<dyn Shape>,
    min: usize,
    max: Option<usize>,
}

impl RepeatShape {
    /// Build an unbounded repeat over the given body shape.
    pub fn new(body: Arc<dyn Shape>) -> Self {
        Self::with_bounds(body, 0, None)
    }

    /// Build a repeat with a minimum and optional maximum item count.
    pub fn with_bounds(body: Arc<dyn Shape>, min: usize, max: Option<usize>) -> Self {
        Self { body, min, max }
    }

    /// Return the shape applied to each item.
    pub fn body(&self) -> &Arc<dyn Shape> {
        &self.body
    }

    /// Return the minimum required item count.
    pub fn min(&self) -> usize {
        self.min
    }

    /// Return the maximum allowed item count, if bounded.
    pub fn max(&self) -> Option<usize> {
        self.max
    }
}

impl Shape for RepeatShape {
    fn is_effectful(&self) -> bool {
        self.body.is_effectful()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let Some(parent) = parent.as_any().downcast_ref::<Self>() else {
            return Ok(None);
        };
        if self.min < parent.min {
            return Ok(None);
        }
        if !max_at_most(self.max, parent.max) {
            return Ok(None);
        }
        shape_is_subshape_of(cx, self.body.as_ref(), parent.body.as_ref()).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let Some(list) = value.object().as_list() else {
            let expr = value.object().as_expr(cx)?;
            return self.check_expr(cx, &expr);
        };
        let items = force_list_to_vec(cx, list, "shape-repeat")?;
        self.check_values(cx, &items)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let items = match expr {
            Expr::List(items) | Expr::Vector(items) | Expr::Set(items) => items,
            _ => return Ok(ShapeMatch::reject("shape-repeat: expected list expression")),
        };
        let mut out = ShapeMatch::accept(MatchScore::exact(20));
        for item in items {
            let mut matched = self.body.check_expr(cx, item)?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-repeat: item rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        self.finish_expr(out, items.len())
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let max = self
            .max
            .map(|max| max.to_string())
            .unwrap_or_else(|| "unbounded".to_owned());
        Ok(ShapeDoc::new("repeat shape")
            .with_detail(self.body.describe(cx)?.name)
            .with_detail(format!("min {}", self.min))
            .with_detail(format!("max {max}")))
    }
}

impl TableShape {
    fn check_table_value(&self, cx: &mut Cx, table: &dyn Table) -> Result<ShapeMatch> {
        let entries = table.entries(cx)?;
        self.check_value_entries(cx, &entries)
    }

    fn check_value_entries(&self, cx: &mut Cx, entries: &[(Symbol, Value)]) -> Result<ShapeMatch> {
        let mut out = ShapeMatch::accept(MatchScore::exact(20));
        let mut matched_keys = Vec::new();
        let mut missing_keys = Vec::new();

        for field in &self.fields {
            let Some((_, value)) = entries.iter().find(|(key, _)| *key == field.key) else {
                if field.required {
                    missing_keys.push(field.key.clone());
                }
                continue;
            };
            let mut matched = field.shape.check_value(cx, value.clone())?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-table: field rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
            matched_keys.push(field.key.clone());
        }

        if !missing_keys.is_empty() {
            let mut captures = Bindings::new();
            captures.bind_value(
                capture_symbol("missing-keys"),
                symbol_list_value(cx, &missing_keys)?,
            );
            return Ok(ShapeMatch {
                accepted: false,
                captures,
                score: MatchScore::reject(),
                diagnostics: vec![Diagnostic::error("shape-table: missing keys")],
            });
        }

        let field_keys = self
            .fields
            .iter()
            .map(|field| field.key.clone())
            .collect::<Vec<_>>();
        for (key, value) in entries {
            if field_keys.contains(key) {
                continue;
            }
            match &self.extra {
                TableExtraPolicy::Allow => {}
                TableExtraPolicy::Reject => {
                    return Ok(ShapeMatch::reject(format!("shape-table: extra key {key}")));
                }
                TableExtraPolicy::Shape(shape) => {
                    let mut matched = shape.check_value(cx, value.clone())?;
                    if !matched.accepted {
                        matched
                            .diagnostics
                            .insert(0, Diagnostic::error("shape-table: extra value rejected"));
                        return Ok(matched);
                    }
                    out.captures.extend(matched.captures);
                    out.score += matched.score;
                }
            }
        }

        out.captures.bind_value(
            capture_symbol("matched-keys"),
            symbol_list_value(cx, &matched_keys)?,
        );
        Ok(out)
    }

    fn check_map_expr(&self, cx: &mut Cx, entries: &[(Symbol, Expr)]) -> Result<ShapeMatch> {
        let mut out = ShapeMatch::accept(MatchScore::exact(20));
        let mut matched_keys = Vec::new();
        let mut missing_keys = Vec::new();

        for field in &self.fields {
            let Some((_, value)) = entries.iter().find(|(key, _)| *key == field.key) else {
                if field.required {
                    missing_keys.push(field.key.clone());
                }
                continue;
            };
            let mut matched = field.shape.check_expr(cx, value)?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-table: field rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
            matched_keys.push(field.key.clone());
        }

        if !missing_keys.is_empty() {
            let mut captures = Bindings::new();
            captures.bind_expr(
                capture_symbol("missing-keys"),
                symbol_list_expr(&missing_keys),
            );
            return Ok(ShapeMatch {
                accepted: false,
                captures,
                score: MatchScore::reject(),
                diagnostics: vec![Diagnostic::error("shape-table: missing keys")],
            });
        }

        let field_keys = self
            .fields
            .iter()
            .map(|field| field.key.clone())
            .collect::<Vec<_>>();
        for (key, value) in entries {
            if field_keys.contains(key) {
                continue;
            }
            match &self.extra {
                TableExtraPolicy::Allow => {}
                TableExtraPolicy::Reject => {
                    return Ok(ShapeMatch::reject(format!("shape-table: extra key {key}")));
                }
                TableExtraPolicy::Shape(shape) => {
                    let mut matched = shape.check_expr(cx, value)?;
                    if !matched.accepted {
                        matched
                            .diagnostics
                            .insert(0, Diagnostic::error("shape-table: extra value rejected"));
                        return Ok(matched);
                    }
                    out.captures.extend(matched.captures);
                    out.score += matched.score;
                }
            }
        }

        out.captures.bind_expr(
            capture_symbol("matched-keys"),
            symbol_list_expr(&matched_keys),
        );
        Ok(out)
    }
}

impl RepeatShape {
    fn check_values(&self, cx: &mut Cx, items: &[Value]) -> Result<ShapeMatch> {
        let mut out = ShapeMatch::accept(MatchScore::exact(20));
        for item in items {
            let mut matched = self.body.check_value(cx, item.clone())?;
            if !matched.accepted {
                matched
                    .diagnostics
                    .insert(0, Diagnostic::error("shape-repeat: item rejected"));
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        self.finish_value(cx, out, items.len())
    }

    fn finish_value(&self, cx: &mut Cx, mut out: ShapeMatch, count: usize) -> Result<ShapeMatch> {
        if count < self.min {
            return Ok(ShapeMatch::reject("shape-repeat: too few items"));
        }
        if matches!(self.max, Some(max) if count > max) {
            return Ok(ShapeMatch::reject("shape-repeat: too many items"));
        }
        out.captures
            .bind_value(capture_symbol("repeat-count"), number_value(cx, count)?);
        Ok(out)
    }

    fn finish_expr(&self, mut out: ShapeMatch, count: usize) -> Result<ShapeMatch> {
        if count < self.min {
            return Ok(ShapeMatch::reject("shape-repeat: too few items"));
        }
        if matches!(self.max, Some(max) if count > max) {
            return Ok(ShapeMatch::reject("shape-repeat: too many items"));
        }
        out.captures
            .bind_expr(capture_symbol("repeat-count"), number_expr(count));
        Ok(out)
    }
}

fn extra_policy_at_least_as_strict(
    cx: &mut Cx,
    child: &TableExtraPolicy,
    parent: &TableExtraPolicy,
) -> Result<bool> {
    Ok(match (child, parent) {
        (_, TableExtraPolicy::Allow) => true,
        (TableExtraPolicy::Reject, TableExtraPolicy::Reject | TableExtraPolicy::Shape(_)) => true,
        (TableExtraPolicy::Shape(child), TableExtraPolicy::Shape(parent)) => {
            shape_is_subshape_of(cx, child.as_ref(), parent.as_ref())?
        }
        (TableExtraPolicy::Allow, TableExtraPolicy::Reject | TableExtraPolicy::Shape(_)) => false,
        (TableExtraPolicy::Shape(_), TableExtraPolicy::Reject) => false,
    })
}

fn max_at_most(child: Option<usize>, parent: Option<usize>) -> bool {
    match (child, parent) {
        (_, None) => true,
        (Some(child), Some(parent)) => child <= parent,
        (None, Some(_)) => false,
    }
}
