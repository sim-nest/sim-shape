use std::sync::Arc;

use sim_kernel::{
    CORE_LIST_CLASS_ID, CaseId, ClassId, ClassRef, Cx, DefaultFactory, Expr, FunctionId,
    HintMetadata, LengthResult, ListValue, NoopEvalPolicy, NumberLiteral, Object, PreparedArgs,
    Result, Symbol, Value, shape_is_subshape_of,
};

use crate::{
    AnyShape, Bindings, CaptureShape, ClassShape, EffectfulShape, ExactExprShape, ExprKind,
    ExprKindShape, FieldShape, FieldSpec, FunctionCase, FunctionObject, ListShape, ObjectExpr,
    OneOfShape, PrattShape, Shape, ShapeExprParser,
};

fn cx() -> sim_kernel::Cx {
    sim_kernel::Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn number_value(cx: &mut sim_kernel::Cx, text: &str) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "f64"), text.to_owned())
        .unwrap()
}

#[test]
fn expr_kind_shape_checks_values_and_exprs() {
    let mut cx = cx();
    let shape = ExprKindShape::new(ExprKind::String);
    let expr = Expr::String("ok".to_owned());
    assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);

    let value = cx.factory().string("ok".to_owned()).unwrap();
    assert!(shape.check_value(&mut cx, value).unwrap().accepted);
}

#[test]
fn expr_kind_rejection_carries_shape_hint() {
    let mut cx = cx();
    let shape = ExprKindShape::new(ExprKind::Number);
    let matched = shape
        .check_expr(&mut cx, &Expr::String("text".to_owned()))
        .unwrap();

    assert!(!matched.accepted);
    let hints = HintMetadata::collect_from_diagnostic(&matched.diagnostics[0]);
    assert_eq!(hints[0].kind, Symbol::qualified("shape-hint", "expected"));
    assert!(hints[0].radar_text().contains("string expression"));
}

#[test]
fn list_shape_collects_captures() {
    let mut cx = cx();
    let shape = ListShape::new(vec![
        Arc::new(crate::ExactExprShape::new(Expr::Symbol(Symbol::new("+")))),
        Arc::new(CaptureShape::new(
            Symbol::new("x"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )),
        Arc::new(CaptureShape::new(
            Symbol::new("y"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )),
    ]);

    let expr = Expr::List(vec![
        Expr::Symbol(Symbol::new("+")),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1.0".to_owned(),
        }),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2.0".to_owned(),
        }),
    ]);

    let matched = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 2);
}

#[test]
fn capture_rejection_adds_binding_hint() {
    let mut cx = cx();
    let shape = CaptureShape::new(
        Symbol::new("n"),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
    );
    let matched = shape
        .check_expr(&mut cx, &Expr::String("text".to_owned()))
        .unwrap();

    assert!(!matched.accepted);
    let hints = HintMetadata::collect_from_diagnostic(&matched.diagnostics[0]);
    assert_eq!(hints[0].kind, Symbol::qualified("shape-hint", "binding"));
    assert!(hints[0].radar_text().contains("n"));
}

fn test_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().nil()
}

#[test]
fn callable_overload_rejection_reports_hints() {
    let mut cx = cx();
    let function = FunctionObject::new(
        FunctionId(7),
        Symbol::qualified("test", "only-number"),
        vec![FunctionCase {
            id: CaseId(1),
            name: Symbol::qualified("case", "number"),
            args: Arc::new(ListShape::new(vec![Arc::new(ExprKindShape::new(
                ExprKind::Number,
            ))])),
            result: None,
            demand: Vec::new(),
            priority: 0,
            implementation: test_case_impl,
        }],
    );
    let prepared = PreparedArgs::new(vec![cx.factory().string("text".to_owned()).unwrap()]);

    let err = match function.select_case(&mut cx, &prepared) {
        Ok(_) => panic!("expected no matching overload"),
        Err(err) => err,
    };
    let sim_kernel::Error::NoMatchingOverload { diagnostics, .. } = err else {
        panic!("expected no matching overload");
    };
    let overload = HintMetadata::collect_from_diagnostic(&diagnostics[0]);
    let callable = HintMetadata::collect_from_diagnostic(&diagnostics[2]);

    assert_eq!(
        overload[0].kind,
        Symbol::qualified("shape-hint", "overload-selection")
    );
    assert_eq!(
        callable[0].kind,
        Symbol::qualified("shape-hint", "callable-mismatch")
    );
}

#[derive(Clone)]
struct EndlessNumberList;

impl Object for EndlessNumberList {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("endless-number-list".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for EndlessNumberList {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(CORE_LIST_CLASS_ID, Symbol::qualified("core", "List"))
    }
    fn as_list(&self) -> Option<&dyn ListValue> {
        Some(self)
    }
}

impl ListValue for EndlessNumberList {
    fn is_empty(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(false)
    }

    fn car(&self, cx: &mut Cx) -> Result<Option<Value>> {
        Ok(Some(number_value(cx, "1")))
    }

    fn cdr(&self, cx: &mut Cx) -> Result<Option<Value>> {
        Ok(Some(cx.factory().opaque(Arc::new(Self))?))
    }

    fn len(&self, _cx: &mut Cx) -> Result<LengthResult> {
        Ok(LengthResult::Unknown)
    }

    fn len_cmp(&self, _cx: &mut Cx, _n: usize) -> Result<std::cmp::Ordering> {
        Ok(std::cmp::Ordering::Greater)
    }
}

#[test]
fn list_shape_with_rest_accepts_endless_value_list() {
    let mut cx = cx();
    // A total, non-binding rest accepts an unbounded tail without walking it
    // forever: the value path stops once a total rest binds nothing.
    let shape = ListShape::with_rest(
        vec![Arc::new(AnyShape), Arc::new(AnyShape)],
        Arc::new(AnyShape),
    );
    let endless = cx.factory().opaque(Arc::new(EndlessNumberList)).unwrap();
    let matched = shape.check_value(&mut cx, endless).unwrap();
    assert!(matched.accepted);
}

#[test]
fn bindings_can_populate_context_env() {
    let mut cx = cx();
    let mut bindings = Bindings::new();
    bindings.bind_expr(Symbol::new("s"), Expr::String("hello".to_owned()));
    bindings.bind_value(Symbol::new("n"), cx.factory().bool(true).unwrap());
    bindings.into_env(&mut cx).unwrap();

    assert!(cx.env().get(&Symbol::new("s")).is_some());
    assert!(cx.env().get(&Symbol::new("n")).is_some());
}

struct FakePrattParser;

impl ShapeExprParser for FakePrattParser {
    fn label(&self) -> &str {
        "fake-pratt"
    }

    fn parse_expr(&self, source: &str) -> Result<Expr> {
        if source != "1 + 2 * 3" {
            return Err(sim_kernel::Error::Eval("unsupported fake input".to_owned()));
        }
        Ok(Expr::Infix {
            operator: Symbol::new("+"),
            left: Box::new(Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            })),
            right: Box::new(Expr::Infix {
                operator: Symbol::new("*"),
                left: Box::new(Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                })),
                right: Box::new(Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "3".to_owned(),
                })),
            }),
        })
    }
}

#[test]
fn pratt_shape_parses_string_surface_before_matching() {
    let mut cx = cx();
    let shape = PrattShape::new(
        Arc::new(FakePrattParser),
        Arc::new(ExprKindShape::new(ExprKind::Infix)),
    );

    let matched = shape
        .check_expr(&mut cx, &Expr::String("1 + 2 * 3".to_owned()))
        .unwrap();

    assert!(matched.accepted);
}

#[test]
fn shape_crate_has_no_concrete_codec_dependency() {
    let manifest = include_str!("../Cargo.toml");

    for line in manifest.lines().map(str::trim) {
        assert!(
            !line.starts_with("sim-codec-"),
            "sim-shape must not depend on concrete codec crates: {line}"
        );
    }
}

#[test]
fn one_of_shape_reports_success_when_any_branch_matches() {
    let mut cx = cx();
    let shape = OneOfShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(AnyShape),
    ]);
    let matched = shape
        .check_expr(&mut cx, &Expr::String("fallback".to_owned()))
        .unwrap();
    assert!(matched.accepted);
}

#[test]
fn exact_expr_is_subshape_of_matching_expr_kind() {
    let mut cx = cx();
    let child = ExactExprShape::new(Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: "1".to_owned(),
    }));
    let number_parent = ExprKindShape::new(ExprKind::Number);
    let string_parent = ExprKindShape::new(ExprKind::String);

    assert!(shape_is_subshape_of(&mut cx, &child, &number_parent).unwrap());
    assert!(!shape_is_subshape_of(&mut cx, &child, &string_parent).unwrap());
}

#[test]
fn one_of_shape_is_subshape_only_when_every_branch_is_subshape() {
    let mut cx = cx();
    let number_parent = ExprKindShape::new(ExprKind::Number);
    let number_or_number = OneOfShape::new(vec![
        Arc::new(ExactExprShape::new(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        }))),
        Arc::new(ExactExprShape::new(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2".to_owned(),
        }))),
    ]);
    let number_or_string = OneOfShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    ]);

    assert!(shape_is_subshape_of(&mut cx, &number_or_number, &number_parent).unwrap());
    assert!(!shape_is_subshape_of(&mut cx, &number_or_string, &number_parent).unwrap());
}

#[test]
fn effectful_shape_is_not_subshape_of_inner_or_any() {
    let mut cx = cx();
    let effectful = EffectfulShape::new(Arc::new(AnyShape));

    assert!(!shape_is_subshape_of(&mut cx, &effectful, &AnyShape).unwrap());
}

#[test]
fn class_shape_checks_runtime_class() {
    let mut cx = cx();
    let value = cx
        .factory()
        .class_stub(ClassId(77), Symbol::new("Point"))
        .unwrap();
    let shape = ClassShape::new(Symbol::new("Point"));
    assert!(shape.check_value(&mut cx, value).unwrap().accepted);
    assert!(
        !shape
            .check_expr(&mut cx, &Expr::Map(Vec::new()))
            .unwrap()
            .accepted
    );
}

#[test]
fn object_expr_roundtrips() {
    let expr = ObjectExpr {
        class: Symbol::new("Point"),
        fields: vec![(Symbol::new("x"), Expr::Bool(true))],
    }
    .to_expr();
    let parsed = ObjectExpr::parse(&expr).unwrap();
    assert_eq!(parsed.class, Symbol::new("Point"));
    assert_eq!(parsed.field(&Symbol::new("x")), Some(&Expr::Bool(true)));
}

#[test]
fn field_shape_checks_required_fields() {
    let mut cx = cx();
    let shape = FieldShape::new(
        Symbol::new("Point"),
        vec![FieldSpec::required(
            Symbol::new("x"),
            Arc::new(ExprKindShape::new(ExprKind::Number)),
        )],
    );
    let expr = ObjectExpr {
        class: Symbol::new("Point"),
        fields: vec![(Symbol::new("y"), Expr::Bool(true))],
    }
    .to_expr();
    let matched = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(!matched.accepted);
}

/// A minimal class object for adversarial-recursion tests: configurable
/// parents (resolved from the registry by symbol) and an optional
/// self-referential `instance_shape` that resolves back to this very class.
struct TestClass {
    id: ClassId,
    symbol: Symbol,
    parents: Vec<Symbol>,
    self_instance_shape: bool,
}

impl Object for TestClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("test-class {}", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for TestClass {
    fn as_class(&self) -> Option<&dyn sim_kernel::Class> {
        Some(self)
    }
}

impl sim_kernel::Callable for TestClass {
    fn call(&self, _cx: &mut Cx, _args: sim_kernel::Args) -> Result<Value> {
        Err(sim_kernel::Error::Eval(
            "test class is not constructible".to_owned(),
        ))
    }
}

impl sim_kernel::Class for TestClass {
    fn id(&self) -> ClassId {
        self.id
    }

    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn parents(&self, cx: &mut Cx) -> Result<Vec<ClassRef>> {
        Ok(self
            .parents
            .iter()
            .filter_map(|symbol| cx.registry().class_by_symbol(symbol).cloned())
            .collect())
    }

    fn constructor_shape(&self, cx: &mut Cx) -> Result<sim_kernel::ShapeRef> {
        cx.factory().nil()
    }

    fn instance_shape(&self, cx: &mut Cx) -> Result<sim_kernel::ShapeRef> {
        if self.self_instance_shape {
            Ok(crate::shape_value(
                self.symbol.clone(),
                Arc::new(ClassShape::new(self.symbol.clone())),
            ))
        } else {
            cx.factory().nil()
        }
    }

    fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<sim_kernel::ReadConstructorRef>> {
        Ok(None)
    }

    fn members(&self, cx: &mut Cx) -> Result<sim_kernel::TableRef> {
        cx.factory().table(Vec::new())
    }
}

/// A bare instance whose class resolves to a registered [`TestClass`].
struct TestInstance {
    class_symbol: Symbol,
}

impl Object for TestInstance {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("test-instance of {}", self.class_symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for TestInstance {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.registry()
            .class_by_symbol(&self.class_symbol)
            .cloned()
            .ok_or_else(|| sim_kernel::Error::Eval("missing test class".to_owned()))
    }
}

fn register_test_class(
    cx: &mut Cx,
    id: ClassId,
    symbol: Symbol,
    parents: Vec<Symbol>,
    self_instance_shape: bool,
) {
    let value = cx
        .factory()
        .opaque(Arc::new(TestClass {
            id,
            symbol: symbol.clone(),
            parents,
            self_instance_shape,
        }))
        .unwrap();
    cx.registry_mut()
        .register_class_value(symbol, value)
        .unwrap();
}

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
fn list_shape_rest_captures_identically_for_value_and_expr() {
    let mut cx = cx();
    let shape = ListShape::with_rest(
        Vec::new(),
        Arc::new(CaptureShape::new(Symbol::new("rest"), Arc::new(AnyShape))),
    );

    let elements = vec![Expr::Bool(true), Expr::Bool(false), Expr::Bool(true)];
    let expr = Expr::List(elements.clone());
    let expr_match = shape.check_expr(&mut cx, &expr).unwrap();
    assert!(expr_match.accepted);
    assert_eq!(expr_match.captures.exprs().len(), 3);

    let values = elements
        .iter()
        .map(|element| match element {
            Expr::Bool(flag) => cx.factory().bool(*flag).unwrap(),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    let list_value = cx.factory().list(values).unwrap();
    let value_match = shape.check_value(&mut cx, list_value).unwrap();
    assert!(value_match.accepted);

    // The value path now captures the trailing rest identically to the expr
    // path instead of early-returning and binding nothing.
    assert_eq!(value_match.captures.values().len(), 3);
    assert_eq!(
        value_match.captures.values().len(),
        expr_match.captures.exprs().len()
    );
}

fn general_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("general".to_owned())
}

fn specific_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("specific".to_owned())
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
