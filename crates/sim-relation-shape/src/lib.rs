//! Runtime [`Shape`](sim_kernel::Shape) projection for relational records.
//!
//! Domain declarations contain durable [`Ref`](sim_kernel::Ref)s. This crate
//! resolves those references through the ordinary [`Cx`](sim_kernel::Cx)
//! registry and asks the resulting Shape to match each cell value. It neither
//! grants capabilities nor maintains a second type registry.
//!
//! ```
//! use std::sync::Arc;
//! use sim_kernel::{Cx, DefaultFactory, Factory, NoopEvalPolicy, Ref, Symbol};
//! use sim_relation_core::{Cell, DomainCatalog, DomainId, DomainSpec, StorageRepr, ToRelationDatum};
//! use sim_relation_shape::CellShape;
//! use sim_shape::{ExprKind, ExprKindShape, Shape, shape_value};
//!
//! let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
//! let shape_symbol = Symbol::qualified("example", "UuidShape");
//! let shape = shape_value(shape_symbol.clone(), Arc::new(ExprKindShape::new(ExprKind::String)));
//! cx.registry_mut().register_shape_value(shape_symbol.clone(), shape).unwrap();
//! let domain = DomainId::new(Symbol::qualified("example", "uuid")).unwrap();
//! let catalog = DomainCatalog::new([DomainSpec::new(
//!     domain.clone(), StorageRepr::Text, Ref::Symbol(shape_symbol), [],
//! ).unwrap()]).unwrap();
//! let matcher = CellShape::new(Arc::new(catalog)).binding(Symbol::new("uuid"));
//! let good = Cell::new(domain.clone(), Some(sim_kernel::Datum::String("018f".into())));
//! let value = sim_kernel::value_from_datum(&mut cx, good.to_datum()).unwrap();
//! let matched = matcher.check_value(&mut cx, value).unwrap();
//! assert!(matched.accepted);
//! assert_eq!(matched.captures.values().len(), 1);
//! let bad = Cell::new(domain, Some(sim_kernel::Datum::Bool(true)));
//! let value = sim_kernel::value_from_datum(&mut cx, bad.to_datum()).unwrap();
//! assert!(!matcher.check_value(&mut cx, value).unwrap().accepted);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod records;
mod shapes;

pub use records::{
    Admission, AdmissionDiagnostic, RecordKind, RelationRecord, construct_record,
    record_class_symbol,
};
pub use shapes::{CellShape, RecordShape, RowShape, resolve_domain_shape};

#[cfg(test)]
mod tests;
