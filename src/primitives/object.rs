//! Object-grammar shapes: `ObjectExpr`, `FieldSpec`, and `FieldShape` for
//! matching structured object expressions field by field.

use std::sync::Arc;

use crate::base::{MatchScore, Shape, ShapeDoc, ShapeMatch};
use sim_kernel::{Cx, Expr, Result, Symbol, Value};

/// The decoded form of an object expression: a class symbol and its fields.
///
/// A view over the `expr/object` extension form, letting object-grammar shapes
/// read a structured object's class and named field expressions without
/// re-walking the raw `Expr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectExpr {
    /// The class this object form is tagged with.
    pub class: Symbol,
    /// The object's named fields, in source order.
    pub fields: Vec<(Symbol, Expr)>,
}

impl ObjectExpr {
    /// Encode this object back into its `expr/object` extension expression.
    pub fn to_expr(self) -> Expr {
        Expr::Extension {
            tag: Symbol::qualified("expr", "object"),
            payload: Box::new(Expr::Map(vec![
                (Expr::Symbol(Symbol::new("class")), Expr::Symbol(self.class)),
                (
                    Expr::Symbol(Symbol::new("fields")),
                    Expr::Map(
                        self.fields
                            .into_iter()
                            .map(|(key, value)| (Expr::Symbol(key), value))
                            .collect(),
                    ),
                ),
            ])),
        }
    }

    /// Decode an `expr/object` extension expression, or `None` if it is not one.
    pub fn parse(expr: &Expr) -> Option<Self> {
        let Expr::Extension { tag, payload } = expr else {
            return None;
        };
        if *tag != Symbol::qualified("expr", "object") {
            return None;
        }
        let Expr::Map(entries) = payload.as_ref() else {
            return None;
        };
        let mut class = None;
        let mut fields = None;
        for (key, value) in entries {
            let Expr::Symbol(key) = key else {
                continue;
            };
            if *key == Symbol::new("class") {
                if let Expr::Symbol(symbol) = value {
                    class = Some(symbol.clone());
                }
            } else if *key == Symbol::new("fields")
                && let Expr::Map(entries) = value
            {
                let parsed = entries
                    .iter()
                    .map(|(field, value)| match field {
                        Expr::Symbol(symbol) => Some((symbol.clone(), value.clone())),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                fields = parsed;
            }
        }
        Some(Self {
            class: class?,
            fields: fields?,
        })
    }

    /// The expression bound to the named field, if present.
    pub fn field(&self, name: &Symbol) -> Option<&Expr> {
        self.fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value))
    }
}

/// A single field requirement within a [`FieldShape`]: a name, a shape, and
/// whether the field must be present.
pub struct FieldSpec {
    pub(crate) name: Symbol,
    pub(crate) shape: Arc<dyn Shape>,
    pub(crate) required: bool,
}

impl FieldSpec {
    /// Build a required field spec binding `name` to `shape`.
    pub fn required(name: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self {
            name,
            shape,
            required: true,
        }
    }

    /// The field name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// The shape the field's value must match.
    pub fn shape(&self) -> &Arc<dyn Shape> {
        &self.shape
    }
}

/// A shape that matches an object form field by field.
///
/// Each [`FieldSpec`] checks the matching field's value; required fields must
/// be present. When bound to a class the object's class must match; when
/// anonymous, plain `Map` expressions match as well.
pub struct FieldShape {
    class: Option<Symbol>,
    fields: Vec<FieldSpec>,
}

impl FieldShape {
    /// Build a field shape that requires the object's class to be `class`.
    pub fn new(class: Symbol, fields: Vec<FieldSpec>) -> Self {
        Self {
            class: Some(class),
            fields,
        }
    }

    /// Build a class-free field shape that also accepts plain map expressions.
    pub fn anonymous(fields: Vec<FieldSpec>) -> Self {
        Self {
            class: None,
            fields,
        }
    }

    /// The required class symbol, or `None` for an anonymous field shape.
    pub fn class_symbol(&self) -> Option<&Symbol> {
        self.class.as_ref()
    }

    /// The field specs this shape checks.
    pub fn fields(&self) -> &[FieldSpec] {
        &self.fields
    }

    fn match_entries(
        &self,
        cx: &mut Cx,
        class: Option<&Symbol>,
        entries: &[(Symbol, Expr)],
    ) -> Result<ShapeMatch> {
        if let Some(expected) = &self.class
            && class != Some(expected)
        {
            return Ok(ShapeMatch::reject(format!("expected class {}", expected)));
        }

        let mut matched = ShapeMatch::accept(MatchScore::exact(20));
        for spec in &self.fields {
            let Some(value) = entries
                .iter()
                .find_map(|(name, value)| (name == &spec.name).then_some(value))
            else {
                if spec.required {
                    return Ok(ShapeMatch::reject(format!("missing field {}", spec.name)));
                }
                continue;
            };
            let field_match = spec.shape.check_expr(cx, value)?;
            if !field_match.accepted {
                return Ok(field_match);
            }
            matched.captures.extend(field_match.captures);
            matched.score += field_match.score;
        }
        Ok(matched)
    }
}

impl Shape for FieldShape {
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        if let Some(object) = ObjectExpr::parse(expr) {
            return self.match_entries(cx, Some(&object.class), &object.fields);
        }
        if self.class.is_none()
            && let Expr::Map(entries) = expr
        {
            let entries = entries
                .iter()
                .map(|(key, value)| match key {
                    Expr::Symbol(symbol) => Some((symbol.clone(), value.clone())),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(entries) = entries {
                return self.match_entries(cx, None, &entries);
            }
        }
        Ok(ShapeMatch::reject("expected object fields"))
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        let mut doc = match &self.class {
            Some(class) => ShapeDoc::new(format!("fields {}", class)),
            None => ShapeDoc::new("fields"),
        };
        for spec in &self.fields {
            let detail = spec.shape.describe(cx)?;
            doc = doc.with_detail(format!("{}: {}", spec.name, detail.name));
        }
        Ok(doc)
    }
}
