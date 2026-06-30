use std::fmt;

/// Byte offset span in source code: (start, end)
pub type Span = (usize, usize);

/// A spanned value
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

/// Icity - whether an argument is explicit or implicit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icity {
    Explicit,
    Implicit,
}

/// Binary operators on primitive values. Reduced in-eval when both operands
/// are literals; otherwise stuck (`Val::BinOp` carries the unreduced operands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Eq => "==",
            BinOp::Neq => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Lte => "<=",
            BinOp::Gte => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// True for operators that take any equality-comparable operand pair
    /// (Integer, Double, String, Bool, Char). Lt/Gt/Lte/Gte require ordered
    /// primitives (Integer, Double, Char); And/Or require Bool.
    pub fn is_equality(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Neq)
    }

    pub fn is_ordering(self) -> bool {
        matches!(self, BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte)
    }

    pub fn is_logical(self) -> bool {
        matches!(self, BinOp::And | BinOp::Or)
    }
}

/// Unary operators. Currently only logical negation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
}

impl UnOp {
    pub fn symbol(self) -> &'static str {
        match self {
            UnOp::Not => "!",
        }
    }
}

/// Literal values
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Integer(i64),
    Double(f64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lit::Integer(n) => write!(f, "{n}"),
            Lit::Double(n) => write!(f, "{n}"),
            Lit::String(s) => write!(f, "\"{s}\""),
            Lit::Char(c) => write!(f, "'{c}'"),
            Lit::Bool(true) => write!(f, "True"),
            Lit::Bool(false) => write!(f, "False"),
            Lit::Unit => write!(f, "()"),
        }
    }
}

/// Built-in type names
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeLit {
    Type,
    String,
    Integer,
    Double,
    Char,
    Bool,
    Unit,
    Row,
}

impl fmt::Display for TypeLit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeLit::Type => write!(f, "Type"),
            TypeLit::String => write!(f, "String"),
            TypeLit::Integer => write!(f, "Integer"),
            TypeLit::Double => write!(f, "Double"),
            TypeLit::Char => write!(f, "Char"),
            TypeLit::Bool => write!(f, "Bool"),
            TypeLit::Unit => write!(f, "Unit"),
            TypeLit::Row => write!(f, "Row"),
        }
    }
}

/// A record field in a literal: { name = expr, ... }
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: String,
    pub value: Expr,
}

/// A record type field: { name : type, ... }
#[derive(Debug, Clone, PartialEq)]
pub struct RecordTypeField {
    pub name: String,
    pub ty: Expr,
}

/// A row field: name : type
#[derive(Debug, Clone, PartialEq)]
pub struct RowField {
    pub name: String,
    pub ty: Expr,
}

/// A variant tag in sugar syntax: < 'Tag Type | ... >
#[derive(Debug, Clone, PartialEq)]
pub struct VariantTag {
    pub name: String,
    pub payload: Option<Expr>,
}

/// An argument in the spine of a function application
#[derive(Debug, Clone, PartialEq)]
pub struct AppArg {
    pub icity: Icity,
    pub expr: Expr,
}

/// Pattern in a record: { x } or { x = pat }
#[derive(Debug, Clone, PartialEq)]
pub enum RecordPatternField {
    /// { x } - binds name x to field x
    Pun(Spanned<String>),
    /// { x = pat } - matches field x against pat
    Match(String, Pattern),
}

/// Patterns
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub node: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    /// Variable binding: x
    Var(String),
    /// Literal pattern: 42, "hello", True
    Lit(Lit),
    /// Wildcard: _
    Wildcard,
    /// Variant constructor: 'Tag or 'Tag pat
    Variant(String, Option<Box<Pattern>>),
    /// Record pattern: { x, y } or { x = pat, y = pat }
    Record(Vec<RecordPatternField>),
    /// Type annotation: (pat : type)
    Ann(Box<Pattern>, Expr),
}

/// A case branch: pattern -> expr
#[derive(Debug, Clone, PartialEq)]
pub struct CaseBranch {
    pub pattern: Pattern,
    pub body: Expr,
}

/// A let binding
#[derive(Debug, Clone, PartialEq)]
pub struct LetBinding {
    /// Type declaration, if any
    pub ty: Option<Expr>,
    /// Name being bound
    pub name: String,
    /// Parameters (for function definitions)
    pub params: Vec<Pattern>,
    /// The value
    pub value: Expr,
    pub span: Span,
}

/// Pi type parameter
#[derive(Debug, Clone, PartialEq)]
pub struct PiParam {
    pub icity: Icity,
    pub name: Option<String>,
    pub ty: Expr,
}

/// Expressions
pub type Expr = Box<Spanned<ExprKind>>;

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Variable: foo, Bar
    Var(String),

    /// Literal: 42, "hello", True, ()
    Lit(Lit),

    /// Type literal: Type, String, Integer, etc.
    TypeLit(TypeLit),

    /// Lambda: \x => body or \x y z => body
    Lam(Vec<Pattern>, Expr),

    /// Function application: f x y
    App(Expr, Vec<AppArg>),

    /// Let expression: let x = e1 in e2
    Let(Vec<LetBinding>, Expr),

    /// If-then-else: if cond then t else f
    If(Expr, Expr, Expr),

    /// Case expression: case e of { pat -> body; ... }
    Case(Expr, Vec<CaseBranch>),

    /// Type annotation: (e : t)
    Ann(Expr, Expr),

    /// Record literal: { x = 1, y = 2 }
    Record(Vec<RecordField>),

    /// Record type: Rec { x : T, y : U }
    RecordType(Vec<RecordTypeField>, Option<Expr>),

    /// Record access: e.field
    RecordAccess(Expr, String),

    /// Variant constructor: 'Tag or 'Tag expr
    Variant(String, Option<Expr>),

    /// Variant type: < 'Tag1 | 'Tag2 Type >
    VariantType(Vec<VariantTag>, Option<Expr>),

    /// Pi type: (x : A) -> B or A -> B
    Pi(Vec<PiParam>, Expr),

    /// Arrow type (non-dependent): A -> B
    Arrow(Expr, Expr),

    /// List literal: [1, 2, 3]
    List(Vec<Expr>),

    /// List type: List T
    ListType(Expr),

    /// Array literal: Array(1, 2, 3)
    ArrayLit(Vec<Expr>),

    /// Array type: Array T
    ArrayType(Expr),

    /// Hole: ?name
    Hole(String),

    /// Import: import "path"
    Import(String),

    /// Undefined
    Undefined,

    /// Lazy: Lazy expr
    Lazy(Expr),

    /// Force: force expr (not a keyword; handled as application for now)
    Force(Expr),

    /// Mu type (isorecursive): Mu x. F x
    Mu(String, Expr),

    /// Fold: fold expr — wraps a value into a Mu type
    Fold(Expr),

    /// Unfold: unfold expr — unwraps a Mu type
    Unfold(Expr),

    /// Record update/spread: { ...base, field = value, ... }
    RecordUpdate(Expr, Vec<RecordField>),

    /// Pipeline: lhs |> rhs (desugars to rhs lhs)
    Pipe(Expr, Expr),

    /// Binary operator: lhs <op> rhs. Reduces in eval when both sides are
    /// literals; otherwise stays stuck. Type-checked in elaborate.
    BinOp(BinOp, Expr, Expr),

    /// Unary prefix operator: !expr.
    UnOp(UnOp, Expr),
}
