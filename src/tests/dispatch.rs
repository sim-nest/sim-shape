use std::sync::Arc;

use sim_kernel::{CaseId, ClassId, Expr, FunctionId, PreparedArgs, Symbol, shape_is_subshape_of};

use crate::{
    ClassShape, ExprKind, ExprKindShape, FunctionCase, FunctionObject, ListShape, ObjectExpr, Shape,
};

use super::{TestInstance, cx, general_case_impl, register_test_class, specific_case_impl};

#[test]
fn class_shape_with_self_referential_instance_shape_rejects_without_overflow() {
    let mut cx = cx();
    let symbol = Symbol::new("SelfRef");
    register_test_class(&mut cx, ClassId(101), symbol.clone(), Vec::new(), true);

    let shape = ClassShape::new(symbol.clone());
    let expr = ObjectExpr {
        class: symbol,
        fields: vec![(Symbol::new("x"), Expr::Bool(true))],
    }
    .to_expr();

    // The instance shape resolves back to this class; matching must fail closed
    // by exhausting the depth budget rather than overflowing the stack.
    let matched = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(!matched.accepted);
}

#[test]
fn class_shape_two_cycle_hierarchy_terminates() {
    let mut cx = cx();
    let a = Symbol::new("CycleA");
    let b = Symbol::new("CycleB");
    let unrelated = Symbol::new("Unrelated");
    register_test_class(&mut cx, ClassId(111), a.clone(), vec![b.clone()], false);
    register_test_class(&mut cx, ClassId(112), b.clone(), vec![a.clone()], false);
    register_test_class(&mut cx, ClassId(113), unrelated.clone(), Vec::new(), false);

    let shape_a = ClassShape::new(a);
    let shape_b = ClassShape::new(b.clone());
    let shape_unrelated = ClassShape::new(unrelated);

    // A is a subclass of its parent B; the cyclic walk terminates with `true`.
    assert!(shape_is_subshape_of(&mut cx, &shape_a, &shape_b).unwrap());
    // A is not a subshape of an unrelated class; the cyclic walk terminates
    // with `false` instead of overflowing the stack.
    assert!(!shape_is_subshape_of(&mut cx, &shape_a, &shape_unrelated).unwrap());

    // Against a non-class parent the kernel walks ClassShape::parents(); the
    // pruned, cycle-free parent set keeps that walk terminating too.
    let number = ExprKindShape::new(ExprKind::Number);
    assert!(!shape_is_subshape_of(&mut cx, &shape_a, &number).unwrap());
}

#[test]
fn select_case_prefers_proven_more_specific_overload() {
    let mut cx = cx();
    let parent = Symbol::new("DispatchParent");
    let child = Symbol::new("DispatchChild");
    register_test_class(&mut cx, ClassId(121), parent.clone(), Vec::new(), false);
    register_test_class(
        &mut cx,
        ClassId(122),
        child.clone(),
        vec![parent.clone()],
        false,
    );

    // Equal priority and an identical additive match score (both items score as
    // class matches): the only discriminator is that the child-arg overload is
    // a proven subshape of the parent-arg one.
    let general = FunctionCase {
        id: CaseId(1),
        name: Symbol::qualified("case", "general"),
        args: Arc::new(ListShape::new(vec![Arc::new(ClassShape::new(parent))])),
        result: None,
        demand: Vec::new(),
        priority: 0,
        implementation: general_case_impl,
    };
    let specific = FunctionCase {
        id: CaseId(2),
        name: Symbol::qualified("case", "specific"),
        args: Arc::new(ListShape::new(vec![Arc::new(ClassShape::new(
            child.clone(),
        ))])),
        result: None,
        demand: Vec::new(),
        priority: 0,
        implementation: specific_case_impl,
    };
    let function = FunctionObject::new(
        FunctionId(9),
        Symbol::qualified("test", "dispatch"),
        vec![general, specific],
    );

    let instance = cx
        .factory()
        .opaque(Arc::new(TestInstance {
            class_symbol: child,
        }))
        .unwrap();
    let prepared = PreparedArgs::new(vec![instance]);

    let selected = function
        .select_case(&mut cx, &prepared)
        .expect("more-specific overload should be selected, not ambiguous");
    assert_eq!(selected.case.id, CaseId(2));
}
