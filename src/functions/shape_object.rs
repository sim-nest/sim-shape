//! `ShapeObject`: wraps a shape as a callable runtime object and provides the
//! `shape_value` helpers that turn a shape into a kernel value.

use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, DefaultFactory, Expr, Factory, Object, ObjectEncode,
    ObjectEncoding, RawArgs, Result, ShapeRef, Symbol, Value,
};

use crate::base::{Shape, ShapeDoc, ShapeMatch};

struct NamedShape {
    symbol: Symbol,
    shape: Arc<dyn Shape>,
}

impl NamedShape {
    fn new(symbol: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self { symbol, shape }
    }
}

impl Shape for NamedShape {
    fn id(&self) -> Option<sim_kernel::ShapeId> {
        self.shape.id()
    }

    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }

    fn parents(&self, cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        self.shape.parents(cx)
    }

    fn is_effectful(&self) -> bool {
        self.shape.is_effectful()
    }

    fn is_total(&self) -> bool {
        self.shape.is_total()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        let parent = parent
            .as_any()
            .downcast_ref::<NamedShape>()
            .map(|parent| parent.shape.as_ref())
            .unwrap_or(parent);
        self.shape.is_subshape_of(cx, parent)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        self.shape.check_value(cx, value)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        self.shape.check_expr(cx, expr)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        self.shape.describe(cx)
    }
}

/// Runtime object wrapping a [`Shape`] so it is usable as a first-class value:
/// a callable matcher, a kernel class, and (optionally) a re-encodable
/// constructor expression.
#[derive(Clone)]
pub struct ShapeObject {
    /// Symbol naming the wrapped shape.
    pub symbol: Symbol,
    /// The wrapped shape engine.
    pub shape: Arc<dyn Shape>,
    /// Constructor encoding used to round-trip the shape back to an expression.
    pub encoding: Option<ObjectEncoding>,
}

impl ShapeObject {
    /// Wrap a shape under a symbol with no constructor encoding.
    pub fn new(symbol: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self {
            symbol,
            shape,
            encoding: None,
        }
    }

    /// Wrap a shape and record the constructor encoding used to re-encode it.
    pub fn with_encoding(symbol: Symbol, shape: Arc<dyn Shape>, encoding: ObjectEncoding) -> Self {
        Self {
            symbol,
            shape,
            encoding: Some(encoding),
        }
    }

    /// Describe the wrapped shape (name and details).
    pub fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        self.shape.describe(cx)
    }
}

impl Object for ShapeObject {
    fn display(&self, cx: &mut Cx) -> Result<String> {
        let doc = self.shape.describe(cx)?;
        Ok(format!("#<shape {} {}>", self.symbol, doc.name))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for ShapeObject {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx.registry().class_by_symbol(&self.symbol) {
            return Ok(value.clone());
        }
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Shape"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_SHAPE_CLASS_ID,
            Symbol::qualified("core", "Shape"),
        )
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<sim_kernel::Expr> {
        match &self.encoding {
            Some(ObjectEncoding::Constructor { class, args }) => Ok(sim_kernel::Expr::Call {
                operator: Box::new(sim_kernel::Expr::Symbol(class.clone())),
                args: args.clone(),
            }),
            _ => Ok(sim_kernel::Expr::Symbol(self.symbol.clone())),
        }
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let doc = self.shape.describe(cx)?;
        let mut entries = vec![
            (Symbol::new("name"), cx.factory().string(doc.name)?),
            (
                Symbol::new("effectful"),
                cx.factory().bool(self.shape.is_effectful())?,
            ),
            (
                Symbol::new("total"),
                cx.factory().bool(self.shape.is_total())?,
            ),
        ];
        for (index, detail) in doc.details.into_iter().enumerate() {
            entries.push((
                Symbol::qualified("detail", index.to_string()),
                cx.factory().string(detail)?,
            ));
        }
        cx.factory().table(entries)
    }
    fn as_shape(&self) -> Option<&dyn Shape> {
        Some(self.shape.as_ref())
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        self.encoding.as_ref().map(|_| self as &dyn ObjectEncode)
    }
}

impl ObjectEncode for ShapeObject {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        self.encoding.clone().ok_or_else(|| {
            sim_kernel::Error::Eval(format!("shape {} has no constructor encoding", self.symbol))
        })
    }
}

impl Callable for ShapeObject {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let [value] = args.values() else {
            return Err(sim_kernel::Error::Eval(
                "shape call expects 1 argument".to_owned(),
            ));
        };
        sim_kernel::call_shape(
            cx,
            self.shape.as_ref(),
            sim_kernel::ShapeCallTarget::Value(value.clone()),
        )
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let [expr] = args.exprs() else {
            return Err(sim_kernel::Error::Eval(
                "shape call expects 1 expression".to_owned(),
            ));
        };
        sim_kernel::call_shape(
            cx,
            self.shape.as_ref(),
            sim_kernel::ShapeCallTarget::Expr(expr.clone()),
        )
    }
}

/// Wrap a shape as a kernel value: an opaque [`ShapeObject`] that carries the
/// given symbol as the shape's name and exposes it as a callable matcher.
///
/// # Examples
///
/// ```rust
/// # use std::sync::Arc;
/// # use sim_kernel::Symbol;
/// # use sim_shape::{AnyShape, shape_value};
/// let value = shape_value(Symbol::new("any"), Arc::new(AnyShape));
/// assert!(value.object().as_shape().is_some());
/// ```
pub fn shape_value(symbol: Symbol, shape: Arc<dyn Shape>) -> Value {
    DefaultFactory
        .opaque(Arc::new(ShapeObject::new(
            symbol.clone(),
            Arc::new(NamedShape::new(symbol, shape)),
        )))
        .expect("shape object should always be boxable")
}

/// Like [`shape_value`] but also records the constructor encoding so the
/// resulting value can be re-encoded back to its constructor expression.
pub fn shape_value_with_encoding(
    symbol: Symbol,
    shape: Arc<dyn Shape>,
    encoding: ObjectEncoding,
) -> Value {
    DefaultFactory
        .opaque(Arc::new(ShapeObject::with_encoding(
            symbol, shape, encoding,
        )))
        .expect("shape object should always be boxable")
}
