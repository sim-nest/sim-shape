//! Codec-neutral grammar graph records.

use sim_kernel::{Diagnostic, Expr, Symbol};

/// A codec-neutral grammar production.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Production {
    /// A literal or lexeme class named abstractly; renderers supply concrete
    /// terminals for the requested codec surface.
    Terminal(TerminalAtom),
    /// Ordered child productions.
    Seq(Vec<Production>),
    /// One-of-many child productions.
    Alt(Vec<Production>),
    /// Repetition of an inner production.
    Repeat {
        /// The repeated production.
        inner: Box<Production>,
        /// Minimum number of accepted repetitions.
        at_least: usize,
    },
    /// A call-like form with a head and positional argument productions.
    Call {
        /// The rendered call head.
        head: Box<Production>,
        /// The rendered positional arguments.
        args: Vec<Production>,
    },
    /// Reference to a named production in the graph definitions.
    Ref(Symbol),
}

/// Abstract terminal atoms that concrete renderers map to their own syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalAtom {
    /// Any value accepted by the target surface.
    Any,
    /// A symbol token.
    Symbol,
    /// A string token.
    String,
    /// A number token.
    Number,
    /// A boolean token.
    Bool,
    /// The nil/null token.
    Nil,
    /// An array/list form.
    List,
    /// A map/object form.
    Map,
    /// One exact expression literal.
    Exact(Expr),
}

/// A named codec-neutral grammar graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarGraph {
    /// The root production.
    pub root: Production,
    /// Named productions referenced by [`Production::Ref`].
    pub defs: Vec<(Symbol, Production)>,
    /// Diagnostics emitted during lowering.
    pub diagnostics: Vec<Diagnostic>,
}

impl GrammarGraph {
    /// Builds a graph with `root`, no named definitions, and no diagnostics.
    pub fn new(root: Production) -> Self {
        Self {
            root,
            defs: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// The concrete grammar dialect requested by a renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrammarDialect {
    /// JSON Schema.
    JsonSchema,
    /// GBNF.
    Gbnf,
    /// S-expression grammar.
    SExpr,
}

/// Output position for a grammar-rendered form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrammarPosition {
    /// Evaluation position.
    Eval,
    /// Quoted position.
    Quote,
    /// Data position.
    Data,
    /// Pattern position.
    Pattern,
    /// Surface/view position.
    Surface,
}

/// Target metadata for one rendered grammar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarTarget {
    /// The codec symbol this grammar targets.
    pub codec: Symbol,
    /// The concrete grammar dialect.
    pub dialect: GrammarDialect,
    /// The output position interpreted by the codec renderer.
    pub position: GrammarPosition,
}

/// Rendered grammar text plus its neutral graph and diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeGrammar {
    /// Target metadata used to render `text`.
    pub target: GrammarTarget,
    /// The source codec-neutral graph.
    pub graph: GrammarGraph,
    /// Rendered grammar text.
    pub text: String,
    /// Diagnostics emitted during lowering or rendering.
    pub diagnostics: Vec<Diagnostic>,
}
