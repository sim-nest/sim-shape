use std::sync::Arc;

use sim_codec::{DecodePosition, Output, encode_with_codec, grammar_check};
use sim_codec_json::{JsonCodecLib, JsonGrammarRenderer};
use sim_codec_lisp::{LispCodecLib, LispGrammarRenderer};
use sim_kernel::{Cx, EncodeOptions, Expr, Result, Symbol};
use sim_shape::{
    ExprKind, ExprKindShape, GrammarDialect, GrammarPosition, GrammarTarget, ListShape, Shape,
    shape_grammar, shape_grammar_graph, shape_json_schema,
};

fn main() -> Result<()> {
    let mut cx = cx()?;
    let shape = ListShape::new(vec![string_shape(), bool_shape()]);

    let graph = shape_grammar_graph(&shape)?;
    let json_schema = shape_json_schema(&shape)?;
    let json_gbnf = render_json_gbnf(&shape)?;
    let lisp_grammar = render_lisp_grammar(&shape)?;

    assert!(json_schema.contains(r#""type":"array""#));
    assert!(json_gbnf.contains("root ::="));
    assert!(lisp_grammar.contains("(grammar "));

    let codec = q("codec", "json");
    let good_text = encode_text(
        &mut cx,
        &codec,
        &Expr::List(vec![Expr::String("ok".to_owned()), Expr::Bool(true)]),
    )?;
    let bad_text = encode_text(
        &mut cx,
        &codec,
        &Expr::List(vec![
            Expr::String("ok".to_owned()),
            Expr::String("not-bool".to_owned()),
        ]),
    )?;

    let good = grammar_check(&mut cx, &shape, &codec, &good_text, DecodePosition::Data)?;
    let bad = grammar_check(&mut cx, &shape, &codec, &bad_text, DecodePosition::Data)?;

    assert!(good.accepted);
    assert!(!bad.accepted);
    assert!(bad.decoded.is_some());

    println!("grammar graph defs: {}", graph.defs.len());
    println!("json schema rendered: {}", !json_schema.is_empty());
    println!("json gbnf rendered: {}", !json_gbnf.is_empty());
    println!("lisp grammar rendered: {}", !lisp_grammar.is_empty());
    println!("good value accepted: {}", good.accepted);
    println!("bad value accepted: {}", bad.accepted);
    Ok(())
}

fn cx() -> Result<Cx> {
    let mut cx = sim_test_support::core_cx();
    let json = JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json)?;
    let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id())?;
    cx.load_lib(&lisp)?;
    Ok(cx)
}

fn render_json_gbnf(shape: &dyn Shape) -> Result<String> {
    Ok(shape_grammar(
        shape,
        GrammarTarget {
            codec: q("codec", "json"),
            dialect: GrammarDialect::Gbnf,
            position: GrammarPosition::Data,
        },
        &JsonGrammarRenderer::gbnf(),
    )?
    .text)
}

fn render_lisp_grammar(shape: &dyn Shape) -> Result<String> {
    Ok(shape_grammar(
        shape,
        GrammarTarget {
            codec: q("codec", "lisp"),
            dialect: GrammarDialect::SExpr,
            position: GrammarPosition::Data,
        },
        &LispGrammarRenderer::sexpr(),
    )?
    .text)
}

fn encode_text(cx: &mut Cx, codec: &Symbol, expr: &Expr) -> Result<String> {
    let output = encode_with_codec(cx, codec, expr, EncodeOptions::default())?;
    Ok(match output {
        Output::Text(text) => text,
        Output::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn string_shape() -> Arc<dyn Shape> {
    Arc::new(ExprKindShape::new(ExprKind::String))
}

fn bool_shape() -> Arc<dyn Shape> {
    Arc::new(ExprKindShape::new(ExprKind::Bool))
}

fn q(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}
