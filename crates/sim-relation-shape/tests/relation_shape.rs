use std::sync::Arc;

use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Ref, Symbol, value_from_datum};
use sim_relation_core::{Cell, DomainCatalog, DomainId, DomainSpec, StorageRepr, ToRelationDatum};
use sim_relation_shape::{CellShape, RecordKind, RecordShape, construct_record};
use sim_shape::{ExprKind, ExprKindShape, Shape, shape_value};

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

#[test]
fn custom_domain_binding_and_mismatch_use_installed_shape() {
    let mut cx = cx();
    let symbol = Symbol::qualified("test", "UuidShape");
    cx.registry_mut()
        .register_shape_value(
            symbol.clone(),
            shape_value(
                symbol.clone(),
                Arc::new(ExprKindShape::new(ExprKind::String)),
            ),
        )
        .unwrap();
    let id = DomainId::new(Symbol::qualified("test", "uuid")).unwrap();
    let catalog = Arc::new(
        DomainCatalog::new([DomainSpec::new(
            id.clone(),
            StorageRepr::Text,
            Ref::Symbol(symbol),
            [],
        )
        .unwrap()])
        .unwrap(),
    );
    let shape = CellShape::new(catalog).binding(Symbol::new("uuid"));

    let good = Cell::new(id.clone(), Some(Datum::String("018f".into())));
    let value = value_from_datum(&mut cx, good.to_datum()).unwrap();
    let matched = shape.check_value(&mut cx, value).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.values()[0].0, Symbol::new("uuid"));

    let bad = Cell::new(id, Some(Datum::Bool(true)));
    let value = value_from_datum(&mut cx, bad.to_datum()).unwrap();
    assert!(!shape.check_value(&mut cx, value).unwrap().accepted);
}

#[test]
fn all_record_shapes_are_open_and_summary_seals_are_not_forgeable() {
    let mut cx = cx();
    for kind in [
        RecordKind::Cell,
        RecordKind::Row,
        RecordKind::Schema,
        RecordKind::RawPlan,
        RecordKind::Migration,
        RecordKind::Binding,
        RecordKind::Receipt,
    ] {
        let datum = Datum::Node {
            tag: kind.tag(),
            fields: vec![(Symbol::new("example"), Datum::Bool(true))],
        };
        let record = construct_record(kind, datum).value.unwrap();
        let value = record.into_value().unwrap();
        assert!(
            RecordShape::new(kind)
                .check_value(&mut cx, value)
                .unwrap()
                .accepted
        );
    }

    let sealed = Datum::Node {
        tag: RecordKind::CheckedPlanSummary.tag(),
        fields: Vec::new(),
    };
    let rejected = construct_record(RecordKind::CheckedPlanSummary, sealed);
    assert!(rejected.value.is_none());
    assert_eq!(
        rejected.diagnostics[0].code,
        Symbol::qualified("relation", "non-forgeable")
    );
}

#[test]
fn wrong_record_tag_returns_structured_diagnostic() {
    let admission = construct_record(
        RecordKind::Row,
        Datum::Node {
            tag: RecordKind::Cell.tag(),
            fields: Vec::new(),
        },
    );
    assert_eq!(
        admission.diagnostics[0].code,
        Symbol::qualified("relation", "wrong-record-shape")
    );
    assert!(matches!(
        admission.diagnostics[0].to_datum(),
        Datum::Node { .. }
    ));
}
