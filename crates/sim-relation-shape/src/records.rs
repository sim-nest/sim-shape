use std::{any::Any, sync::Arc};

use sim_kernel::{
    ClassRef, Cx, Datum, Expr, Factory, Object, ObjectEncode, ObjectEncoding, Result, Symbol,
    Value, value_from_datum,
};

/// Relational record categories projected through Shape and Card metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// Typed scalar cell.
    Cell,
    /// Validated row.
    Row,
    /// Logical schema.
    Schema,
    /// Untrusted raw query or mutation plan.
    RawPlan,
    /// Inspectable summary of an admitted plan; never the checked seal itself.
    CheckedPlanSummary,
    /// Authored migration program.
    Migration,
    /// Pure placement/binding description.
    Binding,
    /// Redacted, immutable effect receipt data.
    Receipt,
}

impl RecordKind {
    /// Canonical node tag for the category.
    pub fn tag(self) -> Symbol {
        Symbol::qualified(
            "relation",
            match self {
                Self::Cell => "cell",
                Self::Row => "row",
                Self::Schema => "schema",
                Self::RawPlan => "raw-plan",
                Self::CheckedPlanSummary => "checked-plan-summary",
                Self::Migration => "migration",
                Self::Binding => "binding",
                Self::Receipt => "receipt",
            },
        )
    }
    /// Runtime Shape symbol for the category.
    pub fn shape_symbol(self) -> Symbol {
        Symbol::qualified("relation", format!("{}-shape", self.label()))
    }
    /// Stable human label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::Row => "row",
            Self::Schema => "schema",
            Self::RawPlan => "raw-plan",
            Self::CheckedPlanSummary => "checked-plan-summary",
            Self::Migration => "migration",
            Self::Binding => "binding",
            Self::Receipt => "receipt",
        }
    }
    /// Whether ordinary data may reconstruct this category.
    pub const fn is_reconstructible(self) -> bool {
        !matches!(self, Self::CheckedPlanSummary)
    }
}

/// Class symbol used by a record's versioned read-construct expression.
pub fn record_class_symbol(kind: RecordKind) -> Symbol {
    Symbol::qualified("relation", format!("{}-record", kind.label()))
}

/// Structured pure-admission diagnostic returned to Lisp builders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionDiagnostic {
    /// Machine-readable diagnostic code.
    pub code: Symbol,
    /// Human-readable explanation.
    pub message: String,
    /// Optional field name associated with the failure.
    pub field: Option<Symbol>,
}

impl AdmissionDiagnostic {
    fn new(code: &str, message: impl Into<String>, field: Option<Symbol>) -> Self {
        Self {
            code: Symbol::qualified("relation", code),
            message: message.into(),
            field,
        }
    }
    /// Projects the diagnostic as codec-neutral ordinary data.
    pub fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: Symbol::qualified("relation", "admission-diagnostic"),
            fields: vec![
                (Symbol::new("code"), Datum::Symbol(self.code.clone())),
                (Symbol::new("message"), Datum::String(self.message.clone())),
                (
                    Symbol::new("field"),
                    self.field.clone().map_or(Datum::Nil, Datum::Symbol),
                ),
            ],
        }
    }
}

/// Result of a pure constructor/builder admission attempt.
#[derive(Clone, Debug)]
pub struct Admission<T> {
    /// Admitted value when validation succeeded.
    pub value: Option<T>,
    /// Structured diagnostics; empty exactly when `value` is present.
    pub diagnostics: Vec<AdmissionDiagnostic>,
}

impl<T> Admission<T> {
    fn accept(value: T) -> Self {
        Self {
            value: Some(value),
            diagnostics: Vec::new(),
        }
    }
    fn reject(diagnostic: AdmissionDiagnostic) -> Self {
        Self {
            value: None,
            diagnostics: vec![diagnostic],
        }
    }
}

/// Inspectable relation citizen backed by one canonical pure datum.
#[derive(Clone, Debug)]
pub struct RelationRecord {
    kind: RecordKind,
    datum: Datum,
}

impl RelationRecord {
    /// Returns the record category.
    pub const fn kind(&self) -> RecordKind {
        self.kind
    }
    /// Returns the canonical Card/Lisp/codec projection.
    pub const fn datum(&self) -> &Datum {
        &self.datum
    }
    /// Converts this citizen to a runtime value.
    pub fn into_value(self) -> Result<Value> {
        sim_kernel::DefaultFactory.opaque(Arc::new(self))
    }
}

/// Pure Lisp/read-constructor builder with structured rejection diagnostics.
///
/// Checked-plan summaries cannot recreate the authority-bearing checked plan;
/// callers must use the plan crate's admission function to obtain a new seal.
pub fn construct_record(kind: RecordKind, datum: Datum) -> Admission<RelationRecord> {
    if !kind.is_reconstructible() {
        return Admission::reject(AdmissionDiagnostic::new(
            "non-forgeable",
            "checked plan summaries cannot reconstruct checked plan seals",
            None,
        ));
    }
    match &datum {
        Datum::Node { tag, .. } if *tag == kind.tag() => {
            Admission::accept(RelationRecord { kind, datum })
        }
        Datum::Node { tag, .. } => Admission::reject(AdmissionDiagnostic::new(
            "wrong-record-shape",
            format!("expected {}, found {tag}", kind.tag()),
            None,
        )),
        _ => Admission::reject(AdmissionDiagnostic::new(
            "expected-record",
            format!("{} constructor expects a Datum::Node", kind.label()),
            None,
        )),
    }
}

impl Object for RelationRecord {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<relation {}>", self.kind.label()))
    }
    fn snapshot(&self, _cx: &mut Cx) -> Result<Option<Datum>> {
        Ok(Some(self.datum.clone()))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl sim_kernel::ObjectCompat for RelationRecord {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = record_class_symbol(self.kind);
        if let Some(class) = cx.registry().class_by_symbol(&symbol) {
            return Ok(class.clone());
        }
        cx.factory()
            .class_stub(sim_kernel::CORE_EXPR_CLASS_ID, symbol)
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(record_class_symbol(self.kind))),
            args: vec![
                Expr::Symbol(Symbol::new("v1")),
                Expr::from(self.datum.clone()),
            ],
        })
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let Datum::Node { fields, .. } = &self.datum else {
            unreachable!("constructor validates relation nodes")
        };
        let mut entries = Vec::with_capacity(fields.len() + 2);
        entries.push((Symbol::new("kind"), cx.factory().symbol(self.kind.tag())?));
        entries.push((
            Symbol::new("reconstructible"),
            cx.factory().bool(self.kind.is_reconstructible())?,
        ));
        for (key, datum) in fields {
            entries.push((key.clone(), value_from_datum(cx, datum.clone())?));
        }
        cx.factory().table(entries)
    }
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for RelationRecord {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: record_class_symbol(self.kind),
            args: vec![
                Expr::Symbol(Symbol::new("v1")),
                Expr::from(self.datum.clone()),
            ],
        })
    }
}
