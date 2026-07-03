//! Shape normalization: reduce a shape to a `ShapeNormalForm` classified by
//! `ShapeNormalKind` so comparison and relation analysis can reason uniformly.

use sim_kernel::{Cx, Result, Symbol};

use crate::{
    AndShape, AnyShape, ListShape, NotShape, OneOfShape, OrShape, RepeatShape, Shape,
    TableExtraPolicy, TableShape,
};

/// Structural summary used by conservative shape comparison.
///
/// Normal forms expose enough algebraic structure for relation checks without
/// adding a closed kind enum to the kernel `Shape` trait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeNormalForm {
    /// Normalized structural kind.
    pub kind: ShapeNormalKind,
    /// Human-readable label from the source shape description.
    pub label: String,
}

/// Normalized shape structure understood by comparison helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeNormalKind {
    /// The total `AnyShape`.
    Any,
    /// A symbol-bearing atomic shape.
    Atom(Symbol),
    /// Flattened conjunction.
    And(Vec<ShapeNormalForm>),
    /// Flattened disjunction, including `OneOfShape`.
    Or(Vec<ShapeNormalForm>),
    /// Complement of another normal form.
    Not(Box<ShapeNormalForm>),
    /// Fixed or variadic list structure.
    List {
        /// Prefix item shapes.
        items: Vec<ShapeNormalForm>,
        /// Optional rest shape for variadic lists.
        rest: Option<Box<ShapeNormalForm>>,
    },
    /// Table fields and whether extra keys are rejected.
    Table {
        /// Named field shapes.
        fields: Vec<(Symbol, ShapeNormalForm)>,
        /// True when the table rejects extra keys.
        closed: bool,
    },
    /// Repeated collection body and bounds.
    Repeat {
        /// Item shape.
        body: Box<ShapeNormalForm>,
        /// Minimum item count.
        min: usize,
        /// Optional maximum item count.
        max: Option<usize>,
    },
    /// Shape with no exposed structure.
    Opaque,
}

impl ShapeNormalForm {
    fn new(kind: ShapeNormalKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
        }
    }
}

/// Build the conservative comparison normal form for a shape.
pub fn normalize_shape(cx: &mut Cx, shape: &dyn Shape) -> Result<ShapeNormalForm> {
    if shape.as_any().is::<AnyShape>() {
        return Ok(ShapeNormalForm::new(ShapeNormalKind::Any, "Any"));
    }

    if let Some(and) = shape.as_any().downcast_ref::<AndShape>() {
        let mut parts = Vec::new();
        for part in and.parts() {
            let normalized = normalize_shape(cx, part.as_ref())?;
            match normalized.kind {
                ShapeNormalKind::And(nested) => parts.extend(nested),
                _ => parts.push(normalized),
            }
        }
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::And(parts),
            label(cx, shape)?,
        ));
    }

    if let Some(or) = shape.as_any().downcast_ref::<OrShape>() {
        return normalize_or(cx, shape, or.choices());
    }

    if let Some(one_of) = shape.as_any().downcast_ref::<OneOfShape>() {
        return normalize_or(cx, shape, one_of.choices());
    }

    if let Some(not) = shape.as_any().downcast_ref::<NotShape>() {
        let inner = normalize_shape(cx, not.inner().as_ref())?;
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::Not(Box::new(inner)),
            label(cx, shape)?,
        ));
    }

    if let Some(list) = shape.as_any().downcast_ref::<ListShape>() {
        let items = list
            .items()
            .iter()
            .map(|item| normalize_shape(cx, item.as_ref()))
            .collect::<Result<Vec<_>>>()?;
        let rest = list
            .rest()
            .map(|rest| normalize_shape(cx, rest.as_ref()).map(Box::new))
            .transpose()?;
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::List { items, rest },
            label(cx, shape)?,
        ));
    }

    if let Some(table) = shape.as_any().downcast_ref::<TableShape>() {
        let fields = table
            .fields()
            .iter()
            .map(|field| {
                Ok((
                    field.key.clone(),
                    normalize_shape(cx, field.shape.as_ref())?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::Table {
                fields,
                closed: matches!(table.extra(), TableExtraPolicy::Reject),
            },
            label(cx, shape)?,
        ));
    }

    if let Some(repeat) = shape.as_any().downcast_ref::<RepeatShape>() {
        let body = normalize_shape(cx, repeat.body().as_ref())?;
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::Repeat {
                body: Box::new(body),
                min: repeat.min(),
                max: repeat.max(),
            },
            label(cx, shape)?,
        ));
    }

    if let Some(symbol) = shape.symbol() {
        return Ok(ShapeNormalForm::new(
            ShapeNormalKind::Atom(symbol.clone()),
            symbol.to_string(),
        ));
    }

    Ok(ShapeNormalForm::new(
        ShapeNormalKind::Opaque,
        label(cx, shape)?,
    ))
}

fn normalize_or(
    cx: &mut Cx,
    shape: &dyn Shape,
    choices: &[std::sync::Arc<dyn Shape>],
) -> Result<ShapeNormalForm> {
    let mut out = Vec::new();
    for choice in choices {
        let normalized = normalize_shape(cx, choice.as_ref())?;
        match normalized.kind {
            ShapeNormalKind::Or(nested) => out.extend(nested),
            _ => out.push(normalized),
        }
    }
    Ok(ShapeNormalForm::new(
        ShapeNormalKind::Or(out),
        label(cx, shape)?,
    ))
}

fn label(cx: &mut Cx, shape: &dyn Shape) -> Result<String> {
    Ok(shape.describe(cx)?.name)
}
