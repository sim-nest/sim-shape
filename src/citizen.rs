//! Citizen integration for shape objects: class registration, codec, and
//! construction wiring, plus the per-shape class-symbol accessors.

#[path = "citizen/class.rs"]
mod class;
#[path = "citizen/codec.rs"]
mod codec;
#[path = "citizen/construct.rs"]
mod construct;
#[path = "citizen/inventory.rs"]
mod inventory;
#[path = "citizen/recursive_codec.rs"]
mod recursive_codec;

pub(crate) use class::register_shape_citizen_class;
pub(crate) use codec::{
    build_shape_value, decode_expr_kind, decode_extra, decode_hooks, decode_shape_list,
    decode_shape_value, decode_symbol, decode_table_fields, decode_venn_members, encode_extra,
    encode_hooks, encode_shape_expr, encode_shape_list, encode_table_fields, expr_kind_symbol,
    or_strategy_symbol,
};
pub(crate) use recursive_codec::{decode_shape_defs, encode_shape_defs};

/// Class symbol for the `Any` shape citizen (`shape/Any`).
pub fn any_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Any")
}

/// Class symbol for the exact-expression shape citizen (`shape/ExactExpr`).
pub fn exact_expr_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "ExactExpr")
}

/// Class symbol for the expression-kind shape citizen (`shape/ExprKind`).
pub fn expr_kind_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "ExprKind")
}

/// Class symbol for the class-membership shape citizen (`shape/Class`).
pub fn class_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Class")
}

/// Class symbol for the list shape citizen (`shape/List`).
pub fn list_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "List")
}

/// Class symbol for the table shape citizen (`shape/Table`).
pub fn table_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Table")
}

/// Class symbol for the disjunction shape citizen (`shape/Or`).
pub fn or_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Or")
}

/// Class symbol for the conjunction shape citizen (`shape/And`).
pub fn and_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "And")
}

/// Class symbol for the negation shape citizen (`shape/Not`).
pub fn not_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Not")
}

/// Class symbol for the repetition shape citizen (`shape/Repeat`).
pub fn repeat_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Repeat")
}

/// Class symbol for the recursive shape definitions citizen (`shape/Defs`).
pub fn shape_defs_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Defs")
}

/// Class symbol for the recursive shape reference citizen (`shape/Ref`).
pub fn shape_def_ref_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Ref")
}

/// Class symbol for the hook-wrapped shape citizen (`shape/Hooked`).
pub fn hooked_shape_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Hooked")
}

/// Class symbol for the Venn-set shape citizen (`shape/Venn`).
pub fn venn_shape_set_class_symbol() -> sim_kernel::Symbol {
    sim_kernel::Symbol::qualified("shape", "Venn")
}
