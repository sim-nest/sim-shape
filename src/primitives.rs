//! Primitive shapes: the atomic shapes, the combinators that build larger
//! shapes from them, and the object-grammar shapes for structured expressions.

mod atomic;
mod combinators;
mod object;

pub use atomic::{AnyShape, ClassShape, ExactExprShape, ExprKindShape, NumberValueShape};
pub use combinators::{
    CaptureShape, EffectfulShape, ListShape, OneOfShape, PrattShape, ShapeExprParser,
};
pub use object::{FieldShape, FieldSpec, ObjectExpr};
