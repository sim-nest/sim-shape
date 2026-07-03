//! Boolean and collection shape algebra: combinator shapes that compose other
//! shapes into conjunctions, disjunctions, negations, repetitions, and
//! table-structured matches.

mod boolean;
mod collection;

#[cfg(test)]
mod tests;

use sim_kernel::{Cx, Expr, NumberLiteral, Result, Symbol, Value};

pub use boolean::{AndShape, NotShape, OrShape, OrStrategy};
pub use collection::{RepeatShape, TableExtraPolicy, TableFieldSpec, TableShape};

pub(crate) fn capture_symbol(name: &str) -> Symbol {
    Symbol::qualified("shape", name)
}

pub(crate) fn number_expr(value: usize) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

pub(crate) fn number_value(cx: &mut Cx, value: usize) -> Result<Value> {
    cx.factory()
        .number_literal(Symbol::qualified("numbers", "f64"), value.to_string())
}

pub(crate) fn symbol_list_expr(symbols: &[Symbol]) -> Expr {
    Expr::List(symbols.iter().cloned().map(Expr::Symbol).collect())
}

pub(crate) fn symbol_list_value(cx: &mut Cx, symbols: &[Symbol]) -> Result<Value> {
    cx.factory().list(
        symbols
            .iter()
            .cloned()
            .map(|symbol| cx.factory().symbol(symbol))
            .collect::<Result<Vec<_>>>()?,
    )
}
