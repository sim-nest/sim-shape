mod core;
mod dispatch;
mod list;
mod object;

use std::sync::Arc;

use sim_kernel::{
    CORE_LIST_CLASS_ID, ClassId, ClassRef, Cx, DefaultFactory, Expr, LengthResult, ListValue,
    NoopEvalPolicy, NumberLiteral, Object, PreparedArgs, Result, Symbol, Value,
};

use crate::{Bindings, ClassShape, ShapeExprParser};

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn number_value(cx: &mut Cx, text: &str) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "f64"), text.to_owned())
        .unwrap()
}

fn test_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().nil()
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

/// A minimal class object for adversarial-recursion tests: configurable
/// parents and an optional self-referential `instance_shape`.
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

fn general_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("general".to_owned())
}

fn specific_case_impl(cx: &mut Cx, _args: &PreparedArgs, _bindings: Bindings) -> Result<Value> {
    cx.factory().string("specific".to_owned())
}
