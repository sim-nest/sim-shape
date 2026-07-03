//! Cycle and depth guards for the shape match path.
//!
//! The kernel `Shape` protocol lets libraries build self-referential shapes
//! through open metadata: a class whose `instance_shape` resolves back to
//! itself, or a class hierarchy with a parent cycle. Matching such a shape must
//! fail closed -- reject -- rather than recurse to a stack overflow. These
//! helpers bound the sim-shape match path with a depth budget and a
//! cycle-pruned class walk, both of which terminate on adversarial input while
//! leaving any legitimate finite shape untouched.

use std::cell::Cell;

use sim_kernel::{Class, ClassId, ClassRef, Cx, Result};

/// Maximum nesting depth for the shape match path before it fails closed.
///
/// Generous enough for any legitimate finite shape (hand-written grammars and
/// class hierarchies never approach it) while still bounding the adversarial
/// self-referential case to a fixed number of stack frames.
pub(crate) const MAX_SHAPE_DEPTH: usize = 256;

thread_local! {
    static SHAPE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII guard bounding shape-match recursion depth on the current thread.
///
/// Held for the duration of one recursive re-entry; the depth counter is
/// decremented when the guard drops, so sequential (non-nested) matches never
/// accumulate budget.
pub(crate) struct DepthGuard {
    _seal: (),
}

impl DepthGuard {
    /// Enter one level of shape recursion, or `None` when the budget is spent.
    ///
    /// A `None` result is the caller's signal to fail closed (reject or error)
    /// instead of recursing further.
    pub(crate) fn enter() -> Option<Self> {
        SHAPE_DEPTH.with(|depth| {
            let current = depth.get();
            if current >= MAX_SHAPE_DEPTH {
                None
            } else {
                depth.set(current + 1);
                Some(Self { _seal: () })
            }
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        SHAPE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Cycle-safe replacement for the kernel `class_is_subclass_of` walk.
///
/// The kernel walk recurses through `parents()` with no visited set, so a class
/// hierarchy with a cycle (constructible through open-metadata subclassing)
/// overflows the stack. This variant carries an explicit visited-id set and
/// reports `false` rather than recursing once a class repeats.
pub(crate) fn class_is_subclass_of_guarded(
    cx: &mut Cx,
    child: &dyn Class,
    expected: ClassRef,
) -> Result<bool> {
    let Some(expected_class) = expected.object().as_class() else {
        return Ok(false);
    };
    let expected_id = expected_class.id();
    let mut visited = Vec::new();
    subclass_walk(cx, child, expected_id, &mut visited)
}

/// Whether `parent` is a cyclic back-edge from `child`'s perspective.
///
/// A parent edge is a cycle when `child` is itself reachable from `parent` --
/// that is, `parent` is (transitively) a subclass of `child`. Pruning such
/// edges from the reported parent set lets the kernel subshape walk terminate
/// on a cyclic hierarchy instead of overflowing the stack.
pub(crate) fn is_cyclic_parent_edge(
    cx: &mut Cx,
    child_id: ClassId,
    parent: &dyn Class,
) -> Result<bool> {
    let mut visited = Vec::new();
    subclass_walk(cx, parent, child_id, &mut visited)
}

fn subclass_walk(
    cx: &mut Cx,
    child: &dyn Class,
    expected_id: ClassId,
    visited: &mut Vec<ClassId>,
) -> Result<bool> {
    let child_id = child.id();
    if child_id == expected_id {
        return Ok(true);
    }
    if visited.contains(&child_id) {
        return Ok(false);
    }
    visited.push(child_id);
    for parent in child.parents(cx)? {
        let Some(parent) = parent.object().as_class() else {
            continue;
        };
        if subclass_walk(cx, parent, expected_id, visited)? {
            return Ok(true);
        }
    }
    Ok(false)
}
