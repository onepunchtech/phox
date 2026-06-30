pub mod ast;
pub mod elaborate;
pub mod error;
#[cfg(feature = "format")]
pub mod format;
pub mod lexer;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod parser;

// Re-exports for common types
pub use ast::{Expr, ExprKind, Icity, Lit, Span, TypeLit};
pub use elaborate::{
    check, infer, Closure, Cxt, ElabError, Env, MetaCxt, MetaVar, Name, SpannedError, Term,
    TermPrinter, Val, VTy,
};
pub use parser::ParseError;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

// Embedded standard library
const STD_PRELUDE: &str = include_str!("../std/prelude.px");

// ---------------------------------------------------------------------------
// Unified error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PhoxError {
    Parse(Vec<ParseError>),
    Elab(SpannedError),
    Eval(ElabError),
    Io(String, std::io::Error),
    Import { path: String, source: Box<PhoxError> },
    CyclicImport(String),
}

impl fmt::Display for PhoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhoxError::Parse(errors) => {
                for e in errors {
                    writeln!(f, "{e}")?;
                }
                Ok(())
            }
            PhoxError::Elab(e) => write!(f, "{e}"),
            PhoxError::Eval(e) => write!(f, "{e}"),
            PhoxError::Io(path, e) => write!(f, "error reading {path}: {e}"),
            PhoxError::Import { path, source } => write!(f, "in import \"{path}\": {source}"),
            PhoxError::CyclicImport(path) => write!(f, "cyclic import detected: {path}"),
        }
    }
}

impl std::error::Error for PhoxError {}

impl PhoxError {
    /// Pretty-print the error with source context using ariadne.
    pub fn report(&self, source: &str, filename: &str) {
        match self {
            PhoxError::Parse(errors) => {
                error::report_parse_errors(source, filename, errors);
            }
            PhoxError::Elab(e) => {
                error::report_elab_error(source, filename, e);
            }
            PhoxError::Eval(e) => {
                eprintln!("Evaluation error: {e}");
            }
            PhoxError::Io(path, e) => {
                eprintln!("Error reading {path}: {e}");
            }
            PhoxError::Import { path, source } => {
                eprintln!("In import \"{path}\":");
                source.report("", path);
            }
            PhoxError::CyclicImport(path) => {
                eprintln!("Cyclic import detected: {path}");
            }
        }
    }
}

impl From<Vec<ParseError>> for PhoxError {
    fn from(errors: Vec<ParseError>) -> Self {
        PhoxError::Parse(errors)
    }
}

impl From<SpannedError> for PhoxError {
    fn from(e: SpannedError) -> Self {
        PhoxError::Elab(e)
    }
}

impl From<ElabError> for PhoxError {
    fn from(e: ElabError) -> Self {
        PhoxError::Eval(e)
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of elaborating a phox expression (before evaluation).
pub struct ElabResult {
    /// Elaborated core term (de Bruijn indexed)
    pub term: Term,
    /// Inferred type as a semantic value
    pub ty: Val,
    /// Inferred type as a core term (for display)
    pub ty_term: Term,
}

/// Result of evaluating a phox expression to normal form.
pub struct EvalResult {
    /// Normalized core term (for display/serialization)
    pub term: Term,
    /// Evaluated semantic value (for programmatic translation)
    pub val: Val,
    /// Type as a semantic value
    pub ty: Val,
    /// Type as a normalized core term (for display)
    pub ty_term: Term,
    /// The metacontext used during elaboration. Closures captured in `val`
    /// reference metas in this context; host code applying those closures
    /// (via `Phox::apply`) must thread this same context through, otherwise
    /// `Term::InsertedMeta` lookups in the body panic.
    pub mcxt: MetaCxt,
}

// ---------------------------------------------------------------------------
// Module cache
// ---------------------------------------------------------------------------

struct CachedModule {
    val: Val,
    ty: Val,
}

// ---------------------------------------------------------------------------
// High-level engine
// ---------------------------------------------------------------------------

/// High-level phox pipeline engine.
///
/// Provides convenient methods that compose the parse → elaborate → evaluate
/// pipeline. Library users can use this to evaluate phox expressions and
/// translate the resulting `Val` to their domain types.
///
/// Holds a module cache so that imported modules are only elaborated once.
///
/// # Example
///
/// ```
/// use phox::{Phox, TermPrinter};
///
/// let phox = Phox::new();
/// let result = phox.eval("let x = 42 in x").unwrap();
/// assert_eq!(format!("{}", TermPrinter(&result.term)), "42");
/// ```
pub struct Phox {
    cache: RefCell<HashMap<PathBuf, CachedModule>>,
    loading: RefCell<HashSet<PathBuf>>,
    /// Extra embedded modules registered by external tools.
    /// Key is the import path (e.g. "mylib/schema"), value is the source.
    extra_modules: HashMap<String, String>,
}

impl Default for Phox {
    fn default() -> Self {
        Self::new()
    }
}

impl Phox {
    pub fn new() -> Self {
        Phox {
            cache: RefCell::new(HashMap::new()),
            loading: RefCell::new(HashSet::new()),
            extra_modules: HashMap::new(),
        }
    }

    /// Register an embedded module that can be imported by path.
    ///
    /// DSL tools use this to provide their own stdlib:
    /// ```
    /// use phox::Phox;
    ///
    /// let phox = Phox::new()
    ///     .with_module("mylib/types", r#"{ Col = < 'Int | 'Text > }"#);
    /// // Users can now: import "mylib/types"
    /// ```
    pub fn with_module(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        self.extra_modules.insert(path.into(), source.into());
        self
    }

    /// Parse source into a presyntax AST.
    pub fn parse(&self, source: &str) -> Result<Expr, PhoxError> {
        parser::parse(source).map_err(PhoxError::Parse)
    }

    /// Parse and elaborate (type-infer), returning the core term and its type.
    pub fn elaborate(&self, source: &str) -> Result<ElabResult, PhoxError> {
        let expr = self.parse(source)?;
        let mcxt = MetaCxt::new();
        let cxt = Cxt::new(&mcxt);
        let (term, ty) = elaborate::infer(&cxt, &expr)?;
        let ty_term = elaborate::quote(&mcxt, cxt.lvl, &ty);
        Ok(ElabResult { term, ty, ty_term })
    }

    /// Parse, elaborate, and evaluate to normal form (no import support).
    pub fn eval(&self, source: &str) -> Result<EvalResult, PhoxError> {
        let expr = self.parse(source)?;
        let mcxt = MetaCxt::new();
        let (tm, ty, env, lvl) = {
            let cxt = Cxt::new(&mcxt);
            let (tm, ty) = elaborate::infer(&cxt, &expr)?;
            (tm, ty, cxt.env.clone(), cxt.lvl)
        };
        Self::normalize(mcxt, env, lvl, tm, ty)
    }

    /// Evaluate a file, resolving imports relative to its directory.
    pub fn eval_file(&self, path: &Path) -> Result<EvalResult, PhoxError> {
        let path = std::fs::canonicalize(path)
            .map_err(|e| PhoxError::Io(path.display().to_string(), e))?;
        let source = std::fs::read_to_string(&path)
            .map_err(|e| PhoxError::Io(path.display().to_string(), e))?;
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        self.eval_with_imports(&source, &dir)
    }

    /// Evaluate source with import resolution from a base directory.
    pub fn eval_with_imports(&self, source: &str, base_dir: &Path) -> Result<EvalResult, PhoxError> {
        let expr = self.parse(source)?;
        let mcxt = MetaCxt::new();
        let resolver = |import_path: &str| -> Result<(Val, Val), String> {
            self.resolve_import(import_path, base_dir)
                .map_err(|e| format!("{e}"))
        };
        let (tm, ty, env, lvl) = {
            let cxt = Cxt::with_resolver(&mcxt, &resolver);
            let (tm, ty) = elaborate::infer(&cxt, &expr)?;
            (tm, ty, cxt.env.clone(), cxt.lvl)
        };
        Self::normalize(mcxt, env, lvl, tm, ty)
    }

    /// Parse, elaborate against an expected type (given as phox source),
    /// and evaluate to normal form.
    ///
    /// # Example
    ///
    /// ```
    /// use phox::Phox;
    ///
    /// let phox = Phox::new();
    /// let result = phox.eval_checked(
    ///     r#"{ name = "my-app", port = 8080 }"#,
    ///     "Rec { name : String, port : Integer }",
    /// ).unwrap();
    /// ```
    pub fn eval_checked(&self, source: &str, schema: &str) -> Result<EvalResult, PhoxError> {
        let schema_expr = parser::parse(schema).map_err(PhoxError::Parse)?;

        let mcxt = MetaCxt::new();
        let (tm, schema_val, env, lvl) = {
            let cxt = Cxt::new(&mcxt);
            let schema_tm = elaborate::check(&cxt, &schema_expr, &Val::U)?;
            let schema_val = elaborate::eval(&mcxt, &cxt.env, &schema_tm)?;
            let expr = parser::parse(source).map_err(PhoxError::Parse)?;
            let tm = elaborate::check(&cxt, &expr, &schema_val)?;
            (tm, schema_val, cxt.env.clone(), cxt.lvl)
        };
        Self::normalize(mcxt, env, lvl, tm, schema_val)
    }

    /// Format phox source code.
    #[cfg(feature = "format")]
    pub fn format(&self, source: &str) -> Result<String, PhoxError> {
        format::format(source).map_err(PhoxError::Parse)
    }

    /// Apply a function-valued `Val` to an argument `Val`. Used by host DSLs
    /// to call into phox lambdas after the spec has been evaluated.
    ///
    /// `mcxt` must be the same metacontext that produced `func` — pull it
    /// from `EvalResult::mcxt`. Closure bodies typically reference
    /// `Term::InsertedMeta` entries that only exist there.
    ///
    /// # Example
    ///
    /// ```
    /// use phox::Phox;
    /// let phox = Phox::new();
    /// let result = phox.eval("\\x => x").unwrap();
    /// let id_at_42 = phox
    ///     .apply(&result.mcxt, &result.val, phox::Val::Lit(phox::Lit::Integer(42)))
    ///     .unwrap();
    /// # let _ = id_at_42;
    /// ```
    pub fn apply(
        &self,
        mcxt: &MetaCxt,
        func: &Val,
        arg: Val,
    ) -> Result<Val, PhoxError> {
        elaborate::v_app(mcxt, func.clone(), arg, Icity::Explicit)
            .map_err(|e| PhoxError::Elab(SpannedError { error: e, span: None }))
    }

    fn resolve_import(&self, import_path: &str, from_dir: &Path) -> Result<(Val, Val), PhoxError> {
        // Stdlib resolution
        if let Some(std_path) = import_path.strip_prefix("std/") {
            return self.resolve_stdlib(std_path, import_path);
        }

        // Extra embedded modules (registered by DSL tools)
        if let Some(source) = self.extra_modules.get(import_path) {
            return self.resolve_embedded(import_path, source);
        }

        // File resolution
        let full_path = from_dir.join(import_path);
        let full_path = std::fs::canonicalize(&full_path)
            .map_err(|e| PhoxError::Io(import_path.to_string(), e))?;

        // Cache check
        if let Some(cached) = self.cache.borrow().get(&full_path) {
            return Ok((cached.val.clone(), cached.ty.clone()));
        }

        // Cycle detection
        if !self.loading.borrow_mut().insert(full_path.clone()) {
            return Err(PhoxError::CyclicImport(import_path.to_string()));
        }

        // Load and evaluate
        let source = std::fs::read_to_string(&full_path)
            .map_err(|e| PhoxError::Io(import_path.to_string(), e))?;
        let dir = full_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let result = self.eval_with_imports(&source, &dir)
            .map_err(|e| PhoxError::Import {
                path: import_path.to_string(),
                source: Box::new(e),
            })?;

        // Cache and clean up loading set
        self.loading.borrow_mut().remove(&full_path);
        self.cache.borrow_mut().insert(full_path, CachedModule {
            val: result.val.clone(),
            ty: result.ty.clone(),
        });

        Ok((result.val, result.ty))
    }

    fn resolve_embedded(&self, import_path: &str, source: &str) -> Result<(Val, Val), PhoxError> {
        let cache_key = PathBuf::from(format!("<{import_path}>"));

        if let Some(cached) = self.cache.borrow().get(&cache_key) {
            return Ok((cached.val.clone(), cached.ty.clone()));
        }

        let result = self.eval(source)
            .map_err(|e| PhoxError::Import {
                path: import_path.to_string(),
                source: Box::new(e),
            })?;

        self.cache.borrow_mut().insert(cache_key, CachedModule {
            val: result.val.clone(),
            ty: result.ty.clone(),
        });

        Ok((result.val, result.ty))
    }

    fn resolve_stdlib(&self, std_path: &str, full_import_path: &str) -> Result<(Val, Val), PhoxError> {
        // Use a synthetic path for caching
        let cache_key = PathBuf::from(format!("<std/{std_path}>"));

        if let Some(cached) = self.cache.borrow().get(&cache_key) {
            return Ok((cached.val.clone(), cached.ty.clone()));
        }

        let source = match std_path {
            "prelude" => STD_PRELUDE,
            _ => return Err(PhoxError::Io(
                full_import_path.to_string(),
                std::io::Error::new(std::io::ErrorKind::NotFound, format!("unknown stdlib module: {std_path}")),
            )),
        };

        // Evaluate stdlib module (no further imports for now)
        let result = self.eval(source)
            .map_err(|e| PhoxError::Import {
                path: full_import_path.to_string(),
                source: Box::new(e),
            })?;

        self.cache.borrow_mut().insert(cache_key, CachedModule {
            val: result.val.clone(),
            ty: result.ty.clone(),
        });

        Ok((result.val, result.ty))
    }

    /// `env` and `lvl` are owned/copied out of the elaboration `Cxt` so the
    /// `Cxt`'s borrow of `mcxt` can be dropped before we move `mcxt` into
    /// `EvalResult`.
    fn normalize(
        mcxt: MetaCxt,
        env: Env,
        lvl: elaborate::Lvl,
        tm: Term,
        ty: Val,
    ) -> Result<EvalResult, PhoxError> {
        let nf = elaborate::nf(&mcxt, &env, &tm)?;
        let val = elaborate::eval(&mcxt, &env, &tm)?;
        let ty_tm = elaborate::quote(&mcxt, lvl, &ty);
        let ty_term = elaborate::nf(&mcxt, &env, &ty_tm).unwrap_or(ty_tm);
        Ok(EvalResult {
            term: nf,
            val,
            ty,
            ty_term,
            mcxt,
        })
    }
}

#[cfg(test)]
mod operator_tests {
    use super::*;

    fn eval_to_string(src: &str) -> String {
        let phox = Phox::new();
        let result = phox.eval(src).expect("eval should succeed");
        format!("{}", TermPrinter(&result.term))
    }

    #[test]
    fn eq_reduces() {
        assert_eq!(eval_to_string("1 == 1"), "True");
        assert_eq!(eval_to_string("1 == 2"), "False");
        assert_eq!(eval_to_string("\"a\" == \"a\""), "True");
        assert_eq!(eval_to_string("True == True"), "True");
    }

    #[test]
    fn neq_reduces() {
        assert_eq!(eval_to_string("1 != 2"), "True");
        assert_eq!(eval_to_string("\"a\" != \"a\""), "False");
    }

    #[test]
    fn ordering_reduces() {
        assert_eq!(eval_to_string("1 < 2"), "True");
        assert_eq!(eval_to_string("2 < 2"), "False");
        assert_eq!(eval_to_string("2 <= 2"), "True");
        assert_eq!(eval_to_string("3 > 1"), "True");
        assert_eq!(eval_to_string("3 >= 3"), "True");
    }

    #[test]
    fn logical_reduces() {
        assert_eq!(eval_to_string("True && True"), "True");
        assert_eq!(eval_to_string("True && False"), "False");
        assert_eq!(eval_to_string("False || True"), "True");
        assert_eq!(eval_to_string("False || False"), "False");
        assert_eq!(eval_to_string("!True"), "False");
        assert_eq!(eval_to_string("!False"), "True");
    }

    #[test]
    fn precedence_holds() {
        // a || b && c => a || (b && c)
        assert_eq!(eval_to_string("False || True && True"), "True");
        // !a && b => (!a) && b
        assert_eq!(eval_to_string("!False && True"), "True");
        // x < y && z parses as (x < y) && z
        assert_eq!(eval_to_string("1 < 2 && 3 > 2"), "True");
    }

    #[test]
    fn operators_in_let_bindings() {
        assert_eq!(
            eval_to_string("let x = 5 in let y = 10 in x < y"),
            "True"
        );
    }

    #[test]
    fn operators_in_lambdas() {
        // Apply a lambda that uses ==
        assert_eq!(
            eval_to_string("(\\x => x == 0) 0"),
            "True"
        );
        // Two-arg lambda combining && and >
        assert_eq!(
            eval_to_string("(\\x y => x > 0 && y > 0) 1 2"),
            "True"
        );
    }

    #[test]
    fn variant_type_unaffected() {
        // The variant-type syntax `< ... >` must still parse alongside `<` /
        // `>` as comparison operators.
        let phox = Phox::new();
        phox.eval("let f : Integer -> < 'A | 'B > = \\_ => 'A in f 0")
            .expect("variant type and arrow should coexist");
    }
}

#[cfg(test)]
mod row_poly_tests {
    use super::*;

    fn eval_ok(src: &str) -> EvalResult {
        Phox::new().eval(src).expect("eval should succeed")
    }

    fn eval_err(src: &str) -> PhoxError {
        match Phox::new().eval(src) {
            Ok(_) => panic!("expected eval failure on: {src}"),
            Err(e) => e,
        }
    }

    fn eval_to_string(src: &str) -> String {
        format!("{}", TermPrinter(&eval_ok(src).term))
    }

    // ── Closed-record regression ──────────────────────────────────────────

    #[test]
    fn closed_record_unifies_with_itself() {
        // Annotating with the same closed type should typecheck and project.
        assert_eq!(
            eval_to_string("({x = 1, y = True} : Rec {x : Integer, y : Bool}).x"),
            "1"
        );
    }

    #[test]
    fn closed_record_missing_field_errors() {
        // Literal {x = 1} cannot be checked against Rec {x : Integer, y : Bool}.
        let _ = eval_err("({x = 1} : Rec {x : Integer, y : Bool})");
    }

    #[test]
    fn closed_record_extra_field_errors() {
        // Closed type rejects an extra field on the literal.
        let _ = eval_err("({x = 1, y = True} : Rec {x : Integer})");
    }

    // ── Open record literal: extras absorbed into tail ────────────────────

    #[test]
    fn open_record_absorbs_extras_via_pattern() {
        // \r => r.x typechecks on any record with at least field `x`.
        assert_eq!(
            eval_to_string("(\\r => r.x) {x = 7, y = 99}"),
            "7"
        );
    }

    #[test]
    fn open_record_missing_required_field_errors() {
        // The lambda requires `foo`; passing {bar = 1} must error.
        let _ = eval_err("(\\r => r.foo) {bar = 1}");
    }

    // ── Multi-access accumulation ─────────────────────────────────────────

    #[test]
    fn multi_access_accumulates_required_fields() {
        // Lambda body references both .x and .y; both must be present.
        assert_eq!(
            eval_to_string("(\\r => r.x == r.y) {x = 1, y = 1, z = 99}"),
            "True"
        );
        assert_eq!(
            eval_to_string("(\\r => r.x == r.y) {x = 1, y = 2, z = 99}"),
            "False"
        );
    }

    #[test]
    fn multi_access_rejects_missing_one_field() {
        // r.x AND r.y required — supplying only x errors.
        let _ = eval_err("(\\r => r.x == r.y) {x = 1, z = 99}");
    }

    // ── Field reordering at the value level still works ───────────────────

    #[test]
    fn record_field_order_is_irrelevant_for_projection() {
        // Same shape, fields written in different orders at the literal site.
        assert_eq!(
            eval_to_string("({y = True, x = 5} : Rec {x : Integer, y : Bool}).x"),
            "5"
        );
    }

    // ── Nested lambda + record access ─────────────────────────────────────

    #[test]
    fn two_param_lambda_with_field_access_applies() {
        // Multi-param lambda with field access:
        //   stateful = \refs model => ...refs.foo... model.bar...
        assert_eq!(
            eval_to_string(
                "(\\refs model => refs.foo) \
                 {foo = 42, bar = 99} \
                 {user = 1, post = 2}"
            ),
            "42"
        );
    }

    #[test]
    fn two_param_lambda_uses_both_records() {
        let r = eval_to_string(
            "(\\refs model => refs.create == model.user) \
             {create = 1} \
             {user = 1}",
        );
        assert_eq!(r, "True");
    }

    // ── Open record type literal: Rec {x : Int ; r} parses + elaborates ───

    #[test]
    fn open_record_type_literal_in_annotation() {
        // `Rec {x : T ; tail}` syntax must elaborate — previously this
        // errored with "record type tail not yet supported".
        eval_ok(
            "let f : Rec {x : Integer ; Rec {}} -> Integer = \\r => r.x \
             in f {x = 5}",
        );
    }

    // ── Variant: explicit case still works ───────────────────────────────

    #[test]
    fn variant_closed_case_works() {
        // Phox case uses layout `| pat => body` branches.
        let src = "let f : < 'A | 'B > -> Integer = \\v => case v of\n  'A => 1\n  'B => 2\nin f 'A";
        assert_eq!(eval_to_string(src), "1");
    }
}
