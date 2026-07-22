//! Internal helpers for rejecting duplicate symbol keys in map-like inputs.

use std::collections::BTreeSet;

use sim_kernel::{Error, Expr, Result, Symbol};

pub(crate) fn reject_duplicate_expr_symbol_keys(
    entries: &[(Expr, Expr)],
    context: &str,
) -> Result<()> {
    reject_duplicate_symbols(
        entries.iter().filter_map(|(key, _)| match key {
            Expr::Symbol(symbol) => Some(symbol),
            _ => None,
        }),
        context,
    )
}

pub(crate) fn reject_duplicate_symbol_keys<T>(
    entries: &[(Symbol, T)],
    context: &str,
) -> Result<()> {
    reject_duplicate_symbols(entries.iter().map(|(symbol, _)| symbol), context)
}

fn reject_duplicate_symbols<'a>(
    symbols: impl IntoIterator<Item = &'a Symbol>,
    context: &str,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for symbol in symbols {
        if !seen.insert(symbol.clone()) {
            return Err(Error::Eval(format!("{context}: duplicate key {symbol}")));
        }
    }
    Ok(())
}
