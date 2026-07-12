//! Shape-backed validation for option maps.
//!
//! Option surfaces often start as runtime key/value pairs and then fan out into
//! local typed readers. This module provides the shared structural check for the
//! map shape before a consumer performs domain-specific parsing.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, Symbol};

use crate::{Shape, ShapeMatch, TableExtraPolicy, TableFieldSpec, TableShape};

/// One option field constraint for [`check_option_map`].
pub struct OptionFieldSpec {
    /// Symbol key to look up in the option map.
    pub key: Symbol,
    /// Shape that must accept the option value when present.
    pub shape: Arc<dyn Shape>,
    /// Whether this option must be present.
    pub required: bool,
}

impl OptionFieldSpec {
    /// Build a required option field.
    pub fn required(key: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self {
            key,
            shape,
            required: true,
        }
    }

    /// Build an optional option field.
    pub fn optional(key: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self {
            key,
            shape,
            required: false,
        }
    }
}

/// Check an option-map expression against field specs and an extra-key policy.
///
/// This is intentionally a thin wrapper around [`TableShape`]: it gives option
/// parsers a domain-named entry point while preserving the same match scoring,
/// captures, and diagnostics as the table grammar.
pub fn check_option_map(
    cx: &mut Cx,
    expr: &Expr,
    fields: Vec<OptionFieldSpec>,
    extra: TableExtraPolicy,
) -> Result<ShapeMatch> {
    let fields = fields
        .into_iter()
        .map(|field| TableFieldSpec {
            key: field.key,
            shape: field.shape,
            required: field.required,
        })
        .collect();
    TableShape::new(fields, extra).check_expr(cx, expr)
}
