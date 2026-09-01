use std::sync::Arc;

use sim_kernel::{
    Cx, Datum, Expr, MatchScore, Ref, RefResolver, ResolvedRef, Result, Shape, ShapeDoc,
    ShapeMatch, Symbol, TemporaryRefResolver, Value, value_from_datum,
};
use sim_relation_core::{DomainCatalog, DomainId};

use crate::RecordKind;

/// Resolves a domain's declared Shape through the standard context machinery.
///
/// The function only reads the registry/content stores. In particular, it does
/// not alter or temporarily widen `Cx` capabilities.
pub fn resolve_domain_shape(cx: &mut Cx, reference: &Ref) -> Result<Value> {
    let value = match TemporaryRefResolver::new().resolve_ref(cx, reference)? {
        ResolvedRef::Symbol(symbol) => cx.resolve_shape(&symbol)?,
        ResolvedRef::Datum(datum) => value_from_datum(cx, datum)?,
        ResolvedRef::Value(value) => value,
        ResolvedRef::Coordinate(_) | ResolvedRef::Missing(_) => {
            return Err(sim_kernel::Error::UnresolvedShapeRef {
                reference: Box::new(reference.clone()),
            });
        }
    };
    if value.object().as_shape().is_none() {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "shape",
            found: "non-shape",
        });
    }
    Ok(value)
}

fn datum_from_value(cx: &mut Cx, value: &Value) -> Result<Option<Datum>> {
    value.object().snapshot(cx)
}

fn datum_from_expr(expr: &Expr) -> Option<Datum> {
    Datum::try_from(expr.clone()).ok()
}

fn node_fields<'a>(datum: &'a Datum, tag: &Symbol) -> Option<&'a [(Symbol, Datum)]> {
    match datum {
        Datum::Node {
            tag: actual,
            fields,
        } if actual == tag => Some(fields),
        _ => None,
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(key, _)| *key == Symbol::new(name))
        .map(|(_, value)| value)
}

/// Shape for a typed relational cell.
pub struct CellShape {
    domains: Arc<DomainCatalog>,
    binding: Option<Symbol>,
}

impl CellShape {
    /// Creates a cell matcher backed by an immutable logical-domain catalog.
    pub fn new(domains: Arc<DomainCatalog>) -> Self {
        Self {
            domains,
            binding: None,
        }
    }

    /// Captures the accepted cell value under `name` for normal Shape binding.
    pub fn binding(mut self, name: Symbol) -> Self {
        self.binding = Some(name);
        self
    }

    fn check_datum(&self, cx: &mut Cx, datum: &Datum) -> Result<ShapeMatch> {
        let Some(fields) = node_fields(datum, &RecordKind::Cell.tag()) else {
            return Ok(ShapeMatch::reject("expected relation/cell record"));
        };
        let Some(Datum::Symbol(domain_symbol)) = field(fields, "domain") else {
            return Ok(ShapeMatch::reject("relation cell has no domain symbol"));
        };
        let domain = DomainId::new(domain_symbol.clone())
            .map_err(|error| sim_kernel::Error::Eval(error.to_string()))?;
        let Some(spec) = self.domains.get(&domain) else {
            return Ok(ShapeMatch::reject(format!(
                "unknown relation domain {domain_symbol}"
            )));
        };
        let Some(value) = field(fields, "value") else {
            return Ok(ShapeMatch::reject("relation cell has no value field"));
        };
        if matches!(value, Datum::Nil) {
            return Ok(ShapeMatch::accept(MatchScore::exact(80)));
        }
        let shape_value = resolve_domain_shape(cx, spec.shape())?;
        let shape = shape_value.object().as_shape().expect("validated shape");
        let runtime_value = value_from_datum(cx, value.clone())?;
        let mut matched = shape.check_value(cx, runtime_value.clone())?;
        if matched.accepted {
            matched.score += MatchScore::exact(20);
            if let Some(name) = &self.binding {
                matched.captures.bind_value(name.clone(), runtime_value);
            }
        }
        Ok(matched)
    }
}

impl Shape for CellShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("relation", "CellShape"))
    }
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let Some(datum) = datum_from_value(cx, &value)? else {
            return Ok(ShapeMatch::reject("relation cell must be pure data"));
        };
        self.check_datum(cx, &datum)
    }
    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let Some(datum) = datum_from_expr(expr) else {
            return Ok(ShapeMatch::reject(
                "relation cell expression must be pure data",
            ));
        };
        self.check_datum(cx, &datum)
    }
    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("relational cell")
            .with_detail("domain Shape resolved through Cx")
            .with_detail("capability-free pure match"))
    }
}

/// Shape for a canonical row whose cells are checked by their domain Shapes.
pub struct RowShape {
    cell: CellShape,
}

impl RowShape {
    /// Creates a row matcher backed by the supplied domain catalog.
    pub fn new(domains: Arc<DomainCatalog>) -> Self {
        Self {
            cell: CellShape::new(domains),
        }
    }
    fn check_datum(&self, cx: &mut Cx, datum: &Datum) -> Result<ShapeMatch> {
        let Some(fields) = node_fields(datum, &RecordKind::Row.tag()) else {
            return Ok(ShapeMatch::reject("expected relation/row record"));
        };
        let Some(Datum::Vector(cells)) = field(fields, "cells") else {
            return Ok(ShapeMatch::reject("relation row has no cell vector"));
        };
        let mut result = ShapeMatch::accept(MatchScore::exact(100));
        for (index, cell) in cells.iter().enumerate() {
            let matched = self.cell.check_datum(cx, cell)?;
            if !matched.accepted {
                return Ok(ShapeMatch::reject(format!(
                    "relation row cell {index} failed domain Shape: {}",
                    matched
                        .diagnostics
                        .first()
                        .map_or("shape mismatch", |diagnostic| diagnostic.message.as_str())
                )));
            }
            result.captures.extend(matched.captures);
        }
        Ok(result)
    }
}

impl Shape for RowShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("relation", "RowShape"))
    }
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let Some(datum) = datum_from_value(cx, &value)? else {
            return Ok(ShapeMatch::reject("relation row must be pure data"));
        };
        self.check_datum(cx, &datum)
    }
    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let Some(datum) = datum_from_expr(expr) else {
            return Ok(ShapeMatch::reject(
                "relation row expression must be pure data",
            ));
        };
        self.check_datum(cx, &datum)
    }
    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("relational row")
            .with_detail("every cell uses its installed domain Shape"))
    }
}

/// Open-metadata Shape for one relational record category.
pub struct RecordShape {
    kind: RecordKind,
}

impl RecordShape {
    /// Creates a record Shape. New categories require no kernel enum change.
    pub const fn new(kind: RecordKind) -> Self {
        Self { kind }
    }
    /// Returns the projected record category.
    pub const fn kind(&self) -> RecordKind {
        self.kind
    }
    fn check_datum(&self, datum: &Datum) -> ShapeMatch {
        if node_fields(datum, &self.kind.tag()).is_some() {
            ShapeMatch::accept(MatchScore::exact(100))
        } else {
            ShapeMatch::reject(format!("expected {} record", self.kind.tag()))
        }
    }
}

impl Shape for RecordShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.kind.shape_symbol())
    }
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        Ok(datum_from_value(cx, &value)?.as_ref().map_or_else(
            || ShapeMatch::reject("record must be inspectable data"),
            |v| self.check_datum(v),
        ))
    }
    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        Ok(datum_from_expr(expr).as_ref().map_or_else(
            || ShapeMatch::reject("record expression must be data"),
            |v| self.check_datum(v),
        ))
    }
    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(self.kind.label())
            .with_detail(format!("record tag: {}", self.kind.tag()))
            .with_detail(if self.kind.is_reconstructible() {
                "pure and read-constructible"
            } else {
                "summary-only; seal or live identity is not constructible"
            }))
    }
}
