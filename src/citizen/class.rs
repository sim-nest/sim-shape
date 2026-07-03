//! The shape citizen class: the `Class`/`Callable`/`ReadConstructor` object
//! that registers shape types with the kernel and constructs shape values.

use std::sync::Arc;

use sim_kernel::{
    Args, Callable, Class, ClassId, ClassRef, Cx, DefaultFactory, Expr, Factory, Linker, Object,
    ReadConstructor, ReadConstructorRef, Result, ShapeRef, Symbol, TableRef, Value,
};

pub(crate) type ConstructFn = fn(&mut Cx, Vec<Value>) -> Result<Value>;

#[derive(Clone)]
struct ShapeCitizenClass {
    id: ClassId,
    symbol: Symbol,
    construct: ConstructFn,
}

impl ShapeCitizenClass {
    fn new(id: ClassId, symbol: Symbol, construct: ConstructFn) -> Self {
        Self {
            id,
            symbol,
            construct,
        }
    }
}

impl Object for ShapeCitizenClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<class {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for ShapeCitizenClass {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Class"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_CLASS_CLASS_ID,
            Symbol::qualified("core", "Class"),
        )
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }

    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        Some(self)
    }
}

impl Callable for ShapeCitizenClass {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        (self.construct)(cx, args.into_vec())
    }
}

impl Class for ShapeCitizenClass {
    fn id(&self) -> ClassId {
        self.id
    }

    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn parents(&self, cx: &mut Cx) -> Result<Vec<ClassRef>> {
        Ok(cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Shape"))
            .cloned()
            .into_iter()
            .collect())
    }

    fn constructor_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn instance_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        Ok(cx
            .registry()
            .shape_by_symbol(&Symbol::qualified("core", "Shape"))
            .cloned()
            .unwrap_or(cx.factory().nil()?))
    }

    fn read_constructor(&self, cx: &mut Cx) -> Result<Option<ReadConstructorRef>> {
        Ok(cx.registry().class_by_symbol(&self.symbol).cloned())
    }

    fn members(&self, cx: &mut Cx) -> Result<TableRef> {
        cx.factory().table(vec![(
            Symbol::new("version"),
            cx.factory().symbol(Symbol::new("v1"))?,
        )])
    }
}

impl ReadConstructor for ShapeCitizenClass {
    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn args_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        (self.construct)(cx, args)
    }
}

pub(crate) fn register_shape_citizen_class(
    linker: &mut Linker<'_>,
    symbol: Symbol,
    construct: ConstructFn,
) -> Result<()> {
    let id = linker.class(symbol.clone())?;
    let class = Arc::new(ShapeCitizenClass::new(id, symbol, construct));
    linker.bind_class_value(id, DefaultFactory.opaque(class)?)
}
