//! Shape-to-grammar lowering and seed JSON Schema rendering.
//!
//! The public lowering returns a codec-neutral [`GrammarGraph`]. JSON Schema is
//! kept here only as the seed renderer that needs no codec-specific terminals;
//! codec-owned renderers can consume the same graph without reversing the
//! dependency arrow.

mod graph;

use sim_kernel::{Error, Expr, Result, Symbol};

use crate::{
    AnyShape, ExactExprShape, ExprKind, ExprKindShape, FieldShape, ListShape, OneOfShape, Shape,
    ShapeDefRef, ShapeDefs,
};

pub use graph::{
    GrammarDialect, GrammarGraph, GrammarPosition, GrammarTarget, Production, ShapeGrammar,
    TerminalAtom,
};

/// Lower a [`Shape`] into a codec-neutral production graph.
pub fn shape_grammar_graph(shape: &dyn Shape) -> Result<GrammarGraph> {
    let lowered = lower_shape(shape)?;
    Ok(GrammarGraph {
        root: lowered.production,
        defs: lowered.defs,
        diagnostics: Vec::new(),
    })
}

/// Render `shape` as JSON Schema using the seed renderer.
///
/// JSON Schema is a concrete dialect, not the neutral grammar form. This helper
/// exists for callers that already consume JSON Schema text while codec-owned
/// renderers migrate to [`GrammarGraph`].
pub fn shape_json_schema(shape: &dyn Shape) -> Result<String> {
    let graph = shape_grammar_graph(shape)?;
    render_json_graph(&graph)
}

/// Renders a neutral [`GrammarGraph`] into one concrete codec grammar surface.
pub trait GrammarRenderer {
    /// Codec symbol this renderer targets.
    fn codec_symbol(&self) -> Symbol;

    /// Concrete grammar dialect this renderer emits.
    fn dialect(&self) -> GrammarDialect;

    /// Renders `graph` for this codec at `position`.
    fn render(&self, graph: &GrammarGraph, position: GrammarPosition) -> Result<String>;
}

/// Lowers `shape` and renders it through a supplied codec-owned renderer.
pub fn shape_grammar(
    shape: &dyn Shape,
    target: GrammarTarget,
    renderer: &dyn GrammarRenderer,
) -> Result<ShapeGrammar> {
    let renderer_codec = renderer.codec_symbol();
    if renderer_codec != target.codec {
        return Err(unsupported_shape(format!(
            "grammar renderer targets codec {}, not {}",
            renderer_codec, target.codec
        )));
    }
    let renderer_dialect = renderer.dialect();
    if renderer_dialect != target.dialect {
        return Err(unsupported_shape(format!(
            "grammar renderer targets dialect {:?}, not {:?}",
            renderer_dialect, target.dialect
        )));
    }
    let graph = shape_grammar_graph(shape)?;
    let text = renderer.render(&graph, target.position)?;
    let diagnostics = graph.diagnostics.clone();
    Ok(ShapeGrammar {
        target,
        graph,
        text,
        diagnostics,
    })
}

struct LoweredProduction {
    production: Production,
    defs: Vec<(Symbol, Production)>,
}

impl LoweredProduction {
    fn new(production: Production) -> Self {
        Self {
            production,
            defs: Vec::new(),
        }
    }
}

fn lower_shape(shape: &dyn Shape) -> Result<LoweredProduction> {
    if shape.as_any().is::<AnyShape>() {
        return Ok(LoweredProduction::new(Production::Alt(vec![
            Production::Terminal(TerminalAtom::Any),
        ])));
    }
    if let Some(kind) = shape.as_any().downcast_ref::<ExprKindShape>() {
        return lower_expr_kind(kind.kind());
    }
    if let Some(defs) = shape.as_any().downcast_ref::<ShapeDefs>() {
        let root = lower_shape(defs.root().as_ref())?;
        let mut graph_defs = root.defs;
        for (name, shape) in defs.defs() {
            let lowered = lower_shape(shape.as_ref())?;
            graph_defs.extend(lowered.defs);
            graph_defs.push((name.clone(), lowered.production));
        }
        return Ok(LoweredProduction {
            production: root.production,
            defs: graph_defs,
        });
    }
    if let Some(reference) = shape.as_any().downcast_ref::<ShapeDefRef>() {
        return Ok(LoweredProduction::new(Production::Ref(
            reference.name().clone(),
        )));
    }
    if let Some(fields) = shape.as_any().downcast_ref::<FieldShape>() {
        return lower_field_shape(fields);
    }
    if let Some(list) = shape.as_any().downcast_ref::<ListShape>() {
        return lower_list_shape(list);
    }
    if let Some(one_of) = shape.as_any().downcast_ref::<OneOfShape>() {
        let choices = one_of
            .choices()
            .iter()
            .map(|choice| lower_shape(choice.as_ref()))
            .collect::<Result<Vec<_>>>()?;
        let mut defs = Vec::new();
        let choices = choices
            .into_iter()
            .map(|choice| {
                defs.extend(choice.defs);
                choice.production
            })
            .collect();
        return Ok(LoweredProduction {
            production: Production::Alt(choices),
            defs,
        });
    }
    if let Some(exact) = shape.as_any().downcast_ref::<ExactExprShape>() {
        return Ok(LoweredProduction::new(Production::Terminal(
            TerminalAtom::Exact(exact.expected().clone()),
        )));
    }
    Err(unsupported_shape(
        "shape_grammar_graph does not support this shape",
    ))
}

fn lower_expr_kind(kind: &ExprKind) -> Result<LoweredProduction> {
    let atom = match kind {
        ExprKind::Nil => TerminalAtom::Nil,
        ExprKind::Bool => TerminalAtom::Bool,
        ExprKind::Number => TerminalAtom::Number,
        ExprKind::String => TerminalAtom::String,
        ExprKind::List | ExprKind::Vector => TerminalAtom::List,
        ExprKind::Map => TerminalAtom::Map,
        ExprKind::Symbol => TerminalAtom::Symbol,
        other => {
            return Err(unsupported_shape(format!(
                "shape_grammar_graph does not support expr-kind {}",
                other.name()
            )));
        }
    };
    Ok(LoweredProduction::new(Production::Terminal(atom)))
}

fn lower_field_shape(shape: &FieldShape) -> Result<LoweredProduction> {
    let mut defs = Vec::new();
    let args = shape
        .fields()
        .iter()
        .map(|field| {
            let lowered = lower_shape(field.shape().as_ref())?;
            defs.extend(lowered.defs);
            Ok(Production::Seq(vec![
                Production::Terminal(TerminalAtom::Exact(Expr::Symbol(field.name().clone()))),
                lowered.production,
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(LoweredProduction {
        production: Production::Call {
            head: Box::new(Production::Terminal(TerminalAtom::Exact(Expr::Symbol(
                shape
                    .class_symbol()
                    .cloned()
                    .unwrap_or_else(|| Symbol::qualified("shape", "fields")),
            )))),
            args,
        },
        defs,
    })
}

fn lower_list_shape(shape: &ListShape) -> Result<LoweredProduction> {
    let mut defs = Vec::new();
    let mut items = shape
        .items()
        .iter()
        .map(|item| {
            let lowered = lower_shape(item.as_ref())?;
            defs.extend(lowered.defs);
            Ok(lowered.production)
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(rest) = shape.rest() {
        let lowered = lower_shape(rest.as_ref())?;
        defs.extend(lowered.defs);
        items.push(Production::Repeat {
            inner: Box::new(lowered.production),
            at_least: 0,
        });
    }
    Ok(LoweredProduction {
        production: Production::Seq(items),
        defs,
    })
}

fn render_json_graph(graph: &GrammarGraph) -> Result<String> {
    let root = render_json_schema(&graph.root)?;
    if graph.defs.is_empty() {
        return Ok(root);
    }
    let defs = graph
        .defs
        .iter()
        .map(|(name, production)| {
            Ok(format!(
                "{}:{}",
                json_string(&name.to_string()),
                render_json_schema(production)?
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(format!(
        r#"{{"allOf":[{}],"$defs":{{{}}}}}"#,
        root,
        defs.join(",")
    ))
}

fn render_json_schema(production: &Production) -> Result<String> {
    match production {
        Production::Terminal(atom) => render_json_terminal(atom),
        Production::Seq(items) => render_json_seq(items),
        Production::Alt(choices) => {
            if choices.len() == 1
                && matches!(
                    choices.first(),
                    Some(Production::Terminal(TerminalAtom::Any))
                )
            {
                return Ok("true".to_owned());
            }
            let choices = choices
                .iter()
                .map(render_json_schema)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!(r#"{{"anyOf":[{}]}}"#, choices.join(",")))
        }
        Production::Repeat { inner, at_least } => {
            let min_items = (*at_least > 0).then(|| format!(r#","minItems":{at_least}"#));
            Ok(format!(
                r#"{{"type":"array","items":{}{}}}"#,
                render_json_schema(inner)?,
                min_items.unwrap_or_default(),
            ))
        }
        Production::Call { head: _, args } => render_json_object(args),
        Production::Ref(name) => Ok(format!(r##"{{"$ref":"#/$defs/{}"}}"##, name)),
    }
}

fn render_json_terminal(atom: &TerminalAtom) -> Result<String> {
    Ok(match atom {
        TerminalAtom::Any => "true".to_owned(),
        TerminalAtom::Nil => r#"{"type":"null"}"#.to_owned(),
        TerminalAtom::Bool => r#"{"type":"boolean"}"#.to_owned(),
        TerminalAtom::Number => r#"{"type":"number"}"#.to_owned(),
        TerminalAtom::String => r#"{"type":"string"}"#.to_owned(),
        TerminalAtom::List => r#"{"type":"array"}"#.to_owned(),
        TerminalAtom::Map => r#"{"type":"object"}"#.to_owned(),
        TerminalAtom::Symbol => r#"{"type":"string","description":"symbol"}"#.to_owned(),
        TerminalAtom::Exact(expr) => format!(r#"{{"const":{}}}"#, json_expr(expr)?),
    })
}

fn render_json_seq(items: &[Production]) -> Result<String> {
    let (prefix, rest) = match items.split_last() {
        Some((Production::Repeat { inner, at_least: 0 }, prefix)) => (prefix, Some(inner)),
        _ => (items, None),
    };
    let prefix_items = prefix
        .iter()
        .map(render_json_schema)
        .collect::<Result<Vec<_>>>()?;
    let items = match rest {
        Some(rest) => render_json_schema(rest)?,
        None => "false".to_owned(),
    };
    let bounds = rest.is_none().then(|| {
        format!(
            r#","minItems":{},"maxItems":{}"#,
            prefix.len(),
            prefix.len()
        )
    });
    Ok(format!(
        r#"{{"type":"array","prefixItems":[{}],"items":{}{}}}"#,
        prefix_items.join(","),
        items,
        bounds.unwrap_or_default(),
    ))
}

fn render_json_object(args: &[Production]) -> Result<String> {
    let mut properties = Vec::new();
    let mut required = Vec::new();
    for arg in args {
        let Production::Seq(parts) = arg else {
            return Err(unsupported_shape(
                "shape_json_schema object field must lower to a sequence",
            ));
        };
        let [
            Production::Terminal(TerminalAtom::Exact(Expr::Symbol(name))),
            value,
        ] = parts.as_slice()
        else {
            return Err(unsupported_shape(
                "shape_json_schema object field must start with a symbol name",
            ));
        };
        properties.push(format!(
            "{}:{}",
            json_string(name.name.as_ref()),
            render_json_schema(value)?
        ));
        required.push(json_string(name.name.as_ref()));
    }
    Ok(format!(
        r#"{{"type":"object","properties":{{{}}},"required":[{}],"additionalProperties":false}}"#,
        properties.join(","),
        required.join(","),
    ))
}

fn json_expr(expr: &Expr) -> Result<String> {
    Ok(match expr {
        Expr::Nil => "null".to_owned(),
        Expr::Bool(value) => value.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::String(text) => json_string(text),
        Expr::Symbol(symbol) => json_string(&symbol.to_string()),
        Expr::List(items) | Expr::Vector(items) => {
            let items = items.iter().map(json_expr).collect::<Result<Vec<_>>>()?;
            format!("[{}]", items.join(","))
        }
        Expr::Map(entries) => {
            let entries = entries
                .iter()
                .map(|(key, value)| {
                    let Expr::Symbol(symbol) = key else {
                        return Err(unsupported_shape(
                            "shape_json_schema exact map keys must be symbols",
                        ));
                    };
                    Ok(format!(
                        "{}:{}",
                        json_string(symbol.name.as_ref()),
                        json_expr(value)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            format!("{{{}}}", entries.join(","))
        }
        _ => {
            return Err(unsupported_shape(
                "shape_json_schema exact expr lowering only supports json-like forms",
            ));
        }
    })
}

fn json_string(text: &str) -> String {
    format!("{text:?}")
}

fn unsupported_shape(message: impl Into<String>) -> Error {
    Error::Eval(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{Expr, NumberLiteral, Result, Symbol};

    use super::{
        GrammarDialect, GrammarPosition, GrammarRenderer, GrammarTarget, Production, TerminalAtom,
        shape_grammar, shape_grammar_graph, shape_json_schema,
    };
    use crate::{
        ExactExprShape, ExprKind, ExprKindShape, FieldShape, FieldSpec, ListShape,
        NumberValueShape, OneOfShape,
    };

    #[test]
    fn graph_lowers_non_trivial_object_shape() {
        let graph = shape_grammar_graph(&record_shape()).unwrap();

        let Production::Call { args, .. } = graph.root else {
            panic!("field shape should lower to a call production");
        };
        assert_eq!(args.len(), 2);
        assert!(graph.defs.is_empty());
        assert!(graph.diagnostics.is_empty());
    }

    #[test]
    fn json_schema_renderer_preserves_seed_object_output() {
        let grammar = shape_json_schema(&record_shape()).unwrap();

        assert!(grammar.contains(r#""type":"object""#));
        assert!(grammar.contains(r#""name":{"type":"string"}"#));
        assert!(grammar.contains(r#""versions":{"type":"array""#));
        assert!(grammar.contains(r#""additionalProperties":false"#));
    }

    #[test]
    fn exact_expr_lowers_to_exact_terminal_and_const_schema() {
        let expr = Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("head")),
                Expr::String("ok".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("count")),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("num", "int"),
                    canonical: "3".to_owned(),
                }),
            ),
        ]);
        let graph = shape_grammar_graph(&ExactExprShape::new(expr)).unwrap();

        assert!(matches!(
            graph.root,
            Production::Terminal(TerminalAtom::Exact(_))
        ));
        assert_eq!(
            shape_json_schema(&ExactExprShape::new(Expr::Bool(true))).unwrap(),
            r#"{"const":true}"#,
        );
    }

    #[test]
    fn one_of_lowers_to_alt_and_any_remains_permissive() {
        let shape = OneOfShape::new(vec![
            Arc::new(ExprKindShape::new(ExprKind::Number)),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        ]);
        let graph = shape_grammar_graph(&shape).unwrap();

        assert!(matches!(graph.root, Production::Alt(_)));
        assert_eq!(shape_json_schema(&crate::AnyShape).unwrap(), "true");
    }

    #[test]
    fn shape_grammar_uses_supplied_renderer() {
        let target = GrammarTarget {
            codec: Symbol::qualified("codec", "test"),
            dialect: GrammarDialect::SExpr,
            position: GrammarPosition::Data,
        };
        let rendered = shape_grammar(
            &record_shape(),
            target.clone(),
            &StubRenderer {
                codec: target.codec.clone(),
                dialect: target.dialect,
            },
        )
        .unwrap();

        assert_eq!(rendered.target, target);
        assert!(rendered.text.contains("codec/test"));
        assert!(rendered.text.contains("root=Call"));
        assert!(rendered.diagnostics.is_empty());
    }

    #[test]
    fn shape_grammar_fails_closed_on_renderer_mismatch() {
        let target = GrammarTarget {
            codec: Symbol::qualified("codec", "test"),
            dialect: GrammarDialect::SExpr,
            position: GrammarPosition::Data,
        };
        let wrong_codec = shape_grammar(
            &record_shape(),
            target.clone(),
            &StubRenderer {
                codec: Symbol::qualified("codec", "other"),
                dialect: target.dialect,
            },
        )
        .unwrap_err();
        assert!(wrong_codec.to_string().contains("codec codec/other"));

        let wrong_dialect = shape_grammar(
            &record_shape(),
            target.clone(),
            &StubRenderer {
                codec: target.codec.clone(),
                dialect: GrammarDialect::JsonSchema,
            },
        )
        .unwrap_err();
        assert!(wrong_dialect.to_string().contains("dialect JsonSchema"));
    }

    #[test]
    fn unsupported_shapes_fail_closed() {
        let err = shape_grammar_graph(&NumberValueShape).unwrap_err();
        assert!(err.to_string().contains("does not support this shape"));

        let err = shape_grammar_graph(&ExprKindShape::new(ExprKind::Bytes)).unwrap_err();
        assert!(err.to_string().contains("expr-kind bytes"));
    }

    fn record_shape() -> FieldShape {
        FieldShape::anonymous(vec![
            FieldSpec::required(
                Symbol::new("name"),
                Arc::new(ExprKindShape::new(ExprKind::String)),
            ),
            FieldSpec::required(
                Symbol::new("versions"),
                Arc::new(ListShape::new(vec![
                    Arc::new(ExprKindShape::new(ExprKind::String)),
                    Arc::new(ExprKindShape::new(ExprKind::String)),
                ])),
            ),
        ])
    }

    struct StubRenderer {
        codec: Symbol,
        dialect: GrammarDialect,
    }

    impl GrammarRenderer for StubRenderer {
        fn codec_symbol(&self) -> Symbol {
            self.codec.clone()
        }

        fn dialect(&self) -> GrammarDialect {
            self.dialect
        }

        fn render(&self, graph: &crate::GrammarGraph, position: GrammarPosition) -> Result<String> {
            Ok(format!(
                "codec={} dialect={:?} position={:?} root={}",
                self.codec,
                self.dialect,
                position,
                production_kind(&graph.root)
            ))
        }
    }

    fn production_kind(production: &Production) -> &'static str {
        match production {
            Production::Terminal(_) => "Terminal",
            Production::Seq(_) => "Seq",
            Production::Alt(_) => "Alt",
            Production::Repeat { .. } => "Repeat",
            Production::Call { .. } => "Call",
            Production::Ref(_) => "Ref",
        }
    }
}
