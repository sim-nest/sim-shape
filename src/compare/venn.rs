//! `VennShapeSet`: a shape object that reasons about set membership across a
//! collection of shapes built from and/or/not combinators.

use std::{collections::BTreeSet, sync::Arc};

use sim_kernel::{ClassRef, Cx, Error, Expr, Object, ObjectEncode, ObjectEncoding, Result, Symbol};

use crate::{AndShape, NotShape, OrShape, Shape};

/// Named set of shapes used to build Venn-style regions.
///
/// `VennShapeSet` is a runtime object so Lisp helpers can create it once and
/// then request union, intersection, selected-only, outside, or exactly regions.
#[derive(Clone)]
pub struct VennShapeSet {
    members: Vec<(Symbol, Arc<dyn Shape>)>,
}

impl VennShapeSet {
    /// Create a named Venn set from shape members.
    pub fn new(members: Vec<(Symbol, Arc<dyn Shape>)>) -> Self {
        Self { members }
    }

    /// Return the named members in registration order.
    pub fn members(&self) -> &[(Symbol, Arc<dyn Shape>)] {
        &self.members
    }

    /// Build a shape accepted by any member.
    pub fn union(&self) -> Arc<dyn Shape> {
        Arc::new(OrShape::new(self.member_shapes()))
    }

    /// Build a shape accepted by every member.
    pub fn intersection(&self) -> Arc<dyn Shape> {
        Arc::new(AndShape::new(self.member_shapes()))
    }

    /// Build a shape accepted by one named member and rejected by the others.
    pub fn only(&self, name: &Symbol) -> Result<Arc<dyn Shape>> {
        let target = self.member_shape(name)?.clone();
        let others = self
            .members
            .iter()
            .filter(|(candidate, _)| candidate != name)
            .map(|(_, shape)| shape.clone())
            .collect::<Vec<_>>();
        if others.is_empty() {
            return Ok(target);
        }
        Ok(Arc::new(AndShape::new(vec![
            target,
            Arc::new(NotShape::new(Arc::new(OrShape::new(others)))),
        ])))
    }

    /// Build a shape rejected by the union of all members.
    pub fn outside_all(&self) -> Arc<dyn Shape> {
        Arc::new(NotShape::new(self.union()))
    }

    /// Build a shape accepted by exactly the selected member names.
    pub fn exactly(&self, names: &[Symbol]) -> Result<Arc<dyn Shape>> {
        let selected = names.iter().cloned().collect::<BTreeSet<_>>();
        for name in &selected {
            self.member_shape(name)?;
        }

        let mut parts = Vec::new();
        let mut others = Vec::new();
        for (name, shape) in &self.members {
            if selected.contains(name) {
                parts.push(shape.clone());
            } else {
                others.push(shape.clone());
            }
        }
        if !others.is_empty() {
            parts.push(Arc::new(NotShape::new(Arc::new(OrShape::new(others)))));
        }
        Ok(Arc::new(AndShape::new(parts)))
    }

    fn member_shape(&self, name: &Symbol) -> Result<&Arc<dyn Shape>> {
        self.members
            .iter()
            .find_map(|(candidate, shape)| (candidate == name).then_some(shape))
            .ok_or_else(|| Error::Eval(format!("shape-venn: unknown member {name}")))
    }

    fn member_shapes(&self) -> Vec<Arc<dyn Shape>> {
        self.members
            .iter()
            .map(|(_, shape)| shape.clone())
            .collect()
    }
}

impl Object for VennShapeSet {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<shape-venn {} members>", self.members.len()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for VennShapeSet {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        let symbol = crate::citizen::venn_shape_set_class_symbol();
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory().nil()
    }

    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        match self.object_encoding(cx)? {
            ObjectEncoding::Constructor { class, args } => Ok(Expr::Call {
                operator: Box::new(Expr::Symbol(class)),
                args,
            }),
            _ => Err(Error::Eval(
                "venn shape set produced a non-constructor object encoding; only \
                 constructor encodings can render as an expression"
                    .to_owned(),
            )),
        }
    }

    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}
