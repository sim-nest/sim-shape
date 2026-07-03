//! Shape comparison and subsumption: normalization, relation analysis between
//! two shapes, and Venn set reasoning over shape membership.

mod normal;
mod relation;
mod venn;

#[cfg(test)]
mod tests;

pub use normal::{ShapeNormalForm, ShapeNormalKind, normalize_shape};
pub use relation::{ShapeProbe, ShapeRelation, ShapeRelationKind, ShapeWitness, relate_shapes};
pub use venn::VennShapeSet;
