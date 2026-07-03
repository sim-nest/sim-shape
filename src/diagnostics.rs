//! Diagnostic constructors shared by shape implementations.

use sim_kernel::{Diagnostic, Expr, HintMetadata, Symbol};

/// Builds a diagnostic for an expected-shape mismatch.
pub fn expected_shape_diagnostic(
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Diagnostic {
    let expected = expected.into();
    let actual = actual.into();
    let message = format!("expected {expected}, found {actual}");
    let hint = HintMetadata::new(
        Symbol::qualified("shape-hint", "expected"),
        "shape mismatch",
    )
    .with_detail(message.clone())
    .with_tag(Symbol::qualified("shape", "expected"))
    .with_tag(Symbol::qualified("shape", "actual"))
    .with_argument(Symbol::new("value"));
    hint.attach_to(Diagnostic::error(message).with_code(Symbol::qualified("shape", "expected")))
}

/// Builds a diagnostic for a binding failure inside a capture or destructuring shape.
pub fn binding_failure_diagnostic(
    binding: &Symbol,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Diagnostic {
    let expected = expected.into();
    let actual = actual.into();
    let message = format!("binding {binding} expected {expected}, found {actual}");
    let hint = HintMetadata::new(
        Symbol::qualified("shape-hint", "binding"),
        "binding mismatch",
    )
    .with_detail(message.clone())
    .with_tag(Symbol::qualified("shape", "binding"))
    .with_argument(binding.clone());
    hint.attach_to(Diagnostic::error(message).with_code(Symbol::qualified("shape", "binding")))
}

/// Builds a diagnostic for a callable shape argument mismatch.
pub fn callable_mismatch_diagnostic(
    callable: &Symbol,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Diagnostic {
    let expected = expected.into();
    let actual = actual.into();
    let message = format!("callable {callable} expected {expected}, found {actual}");
    let hint = HintMetadata::new(
        Symbol::qualified("shape-hint", "callable-mismatch"),
        "callable mismatch",
    )
    .with_detail(message.clone())
    .with_tag(Symbol::qualified("shape", "callable"))
    .with_argument(callable.clone());
    hint.attach_to(
        Diagnostic::error(message).with_code(Symbol::qualified("shape", "callable-mismatch")),
    )
}

/// Builds a diagnostic for overload selection failures or ambiguity.
pub fn overload_selection_diagnostic(function: &Symbol, reason: impl Into<String>) -> Diagnostic {
    let reason = reason.into();
    let message = format!("overload selection for {function}: {reason}");
    let hint = HintMetadata::new(
        Symbol::qualified("shape-hint", "overload-selection"),
        "overload selection",
    )
    .with_detail(message.clone())
    .with_tag(Symbol::qualified("shape", "overload"))
    .with_argument(function.clone());
    hint.attach_to(
        Diagnostic::error(message).with_code(Symbol::qualified("shape", "overload-selection")),
    )
}

/// Returns a compact label for the actual expression kind.
pub(crate) fn expr_actual_label(expr: &Expr) -> String {
    match expr {
        Expr::Nil => "nil expression",
        Expr::Bool(_) => "bool expression",
        Expr::Number(_) => "number expression",
        Expr::Symbol(_) | Expr::Local(_) => "symbol expression",
        Expr::String(_) => "string expression",
        Expr::Bytes(_) => "bytes expression",
        Expr::List(_) => "list expression",
        Expr::Vector(_) => "vector expression",
        Expr::Map(_) => "map expression",
        Expr::Set(_) => "set expression",
        Expr::Call { .. } => "call expression",
        Expr::Infix { .. } => "infix expression",
        Expr::Prefix { .. } => "prefix expression",
        Expr::Postfix { .. } => "postfix expression",
        Expr::Block(_) => "block expression",
        Expr::Quote { .. } => "quote expression",
        Expr::Annotated { .. } => "annotated expression",
        Expr::Extension { .. } => "extension expression",
    }
    .to_owned()
}
