use std::sync::Arc;

use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Ref, Symbol, value_from_datum};
use sim_relation_core::{Cell, DomainCatalog, DomainId, DomainSpec, StorageRepr, ToRelationDatum};
use sim_shape::{ExprKind, ExprKindShape, Shape, shape_value};

use crate::CellShape;

// conformance: custom relational domains bind through the installed Shape.

#[test]
fn custom_domain_binding_accepts_text_and_rejects_shape_mismatch() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    let symbol = Symbol::qualified("specimen", "UuidShape");
    cx.registry_mut()
        .register_shape_value(
            symbol.clone(),
            shape_value(
                symbol.clone(),
                Arc::new(ExprKindShape::new(ExprKind::String)),
            ),
        )
        .unwrap();
    let domain = DomainId::new(Symbol::qualified("specimen", "uuid")).unwrap();
    let catalog = Arc::new(
        DomainCatalog::new([DomainSpec::new(
            domain.clone(),
            StorageRepr::Text,
            Ref::Symbol(symbol),
            [],
        )
        .unwrap()])
        .unwrap(),
    );
    let shape = CellShape::new(catalog).binding(Symbol::new("uuid"));

    let text = Cell::new(domain.clone(), Some(Datum::String("018f".into())));
    let value = value_from_datum(&mut cx, text.to_datum()).unwrap();
    let matched = shape.check_value(&mut cx, value).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.values().len(), 1);

    let boolean = Cell::new(domain, Some(Datum::Bool(true)));
    let value = value_from_datum(&mut cx, boolean.to_datum()).unwrap();
    assert!(!shape.check_value(&mut cx, value).unwrap().accepted);
}
