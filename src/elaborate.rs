// Core language types: elaborated syntax, semantic values, and evaluation machinery.
//
// Following the patterns from AndrasKovacs/elaboration-zoo (04-implicit-args):
//   Presyntax (ast.rs)  →  Core Term (de Bruijn indices)  →  Val (semantic domain)
//
// Extensions beyond elaboration-zoo:
//   - Literal types (Integer, String, Bool, Double, Char, Unit)
//   - Extensible records with row polymorphism
//   - Extensible variants with row polymorphism
//   - Pattern matching

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use crate::ast::{BinOp, Icity, Lit, Span, TypeLit, UnOp};

// ---------------------------------------------------------------------------
// Names and indices
// ---------------------------------------------------------------------------

pub type Name = String;

/// De Bruijn index (counts from the binder inward)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ix(pub usize);

/// De Bruijn level (counts from the outermost binder)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lvl(pub usize);

impl Lvl {
    pub fn to_ix(self, depth: Lvl) -> Ix {
        Ix(depth.0 - self.0 - 1)
    }
}

impl std::ops::Add<usize> for Lvl {
    type Output = Lvl;
    fn add(self, rhs: usize) -> Lvl {
        Lvl(self.0 + rhs)
    }
}

/// Meta variable identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetaVar(pub usize);

/// Bound/Defined — tracks how each context entry was introduced.
/// Used in InsertedMeta to know which bound vars to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BD {
    Bound,
    Defined,
}

// ---------------------------------------------------------------------------
// Core syntax (Term) — elaborated, with de Bruijn indices
// ---------------------------------------------------------------------------

pub type Ty = Term;

#[derive(Debug, Clone)]
pub enum Term {
    Var(Ix),
    Lam(Name, Icity, Box<Term>),
    App(Box<Term>, Box<Term>, Icity),
    U,  // Type : Type (universe)
    Pi(Name, Icity, Box<Ty>, Box<Ty>),
    Let(Name, Box<Ty>, Box<Term>, Box<Term>),
    Meta(MetaVar),
    InsertedMeta(MetaVar, Vec<BD>),

    // Literals
    Lit(Lit),
    LitTy(TypeLit),

    // Records
    Record(Vec<(String, Term)>),          // {x = t, y = u}
    RecordTy(Vec<(String, Ty)>, Option<Box<Ty>>),
                                          // Rec {x : A, y : B} (closed: tail None)
                                          // Rec {x : A ; r}    (open: tail Some)
    RecordProj(Box<Term>, String),        // t.field

    // Variants
    Variant(String, Box<Term>),           // 'Tag t
    VariantTy(Vec<(String, Ty)>, Option<Box<Ty>>),
                                          // < 'A T | 'B U >    (closed)
                                          // < 'A T ; r >       (open)

    // Case / pattern matching
    Case(Box<Term>, Vec<(Pat, Term)>),

    // If-then-else (sugar for case on Bool, but kept for now)
    If(Box<Term>, Box<Term>, Box<Term>),

    // Fix point for recursive definitions
    Fix(Name, Box<Term>),

    // Isorecursive types
    Mu(Box<Term>),        // Mu type: body under one binder (the self-reference)
    Fold(Box<Term>),      // fold: wrap value into Mu type
    Unfold(Box<Term>),    // unfold: unwrap Mu type

    // Primitive operators on literals. Reduce in eval when both operands
    // are literals; otherwise represented stuck via Val::BinOp/UnOp.
    BinOp(BinOp, Box<Term>, Box<Term>),
    UnOp(UnOp, Box<Term>),
}

/// Core patterns (simplified from presyntax patterns during elaboration)
#[derive(Debug, Clone)]
pub enum Pat {
    Var(Name),
    Wildcard,
    Lit(Lit),
    Variant(String, Box<Pat>),
    Record(Vec<RecordPat>),
}

#[derive(Debug, Clone)]
pub enum RecordPat {
    Pun(Name),
    Match(String, Pat),
}

// ---------------------------------------------------------------------------
// Values (semantic domain for NbE)
// ---------------------------------------------------------------------------

pub type Env = Vec<Val>;
pub type Spine = Vec<(Val, Icity)>;
pub type VTy = Val;

#[derive(Clone)]
pub struct Closure {
    pub env: Env,
    pub body: Term,
}

impl fmt::Debug for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure(..)")
    }
}

#[derive(Debug, Clone)]
pub enum Val {
    Flex(MetaVar, Spine),           // unsolved meta applied to spine
    Rigid(Lvl, Spine),              // bound variable applied to spine

    Lam(Name, Icity, Closure),
    Pi(Name, Icity, Box<VTy>, Closure),
    U,

    // Literals
    Lit(Lit),
    LitTy(TypeLit),

    // Records
    Record(Vec<(String, Val)>),
    RecordTy(Vec<(String, Val)>, Option<Box<Val>>),
    RecordProj(Box<Val>, String),   // stuck projection

    // Variants
    Variant(String, Box<Val>),
    VariantTy(Vec<(String, Val)>, Option<Box<Val>>),

    // Stuck case
    Case(Box<Val>, Vec<(Pat, Val)>),

    // If (stuck)
    If(Box<Val>, Box<Val>, Box<Val>),

    // Fix
    Fix(Name, Closure),

    // Isorecursive
    Mu(Closure),          // Mu type value
    Fold(Box<Val>),       // folded value

    // Primitive operators stuck on non-literal operands. When both operands
    // are `Val::Lit`, eval reduces them to a `Val::Lit` directly.
    BinOp(BinOp, Box<Val>, Box<Val>),
    UnOp(UnOp, Box<Val>),
}

impl Val {
    pub fn var(lvl: Lvl) -> Val {
        Val::Rigid(lvl, Vec::new())
    }

    pub fn meta(m: MetaVar) -> Val {
        Val::Flex(m, Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Metacontext
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum MetaEntry {
    Solved(Val),
    Unsolved,
}

#[derive(Debug)]
pub struct MetaCxt {
    entries: RefCell<Vec<MetaEntry>>,
}

impl MetaCxt {
    pub fn new() -> Self {
        MetaCxt {
            entries: RefCell::new(Vec::new()),
        }
    }

    pub fn fresh(&self) -> MetaVar {
        let mut es = self.entries.borrow_mut();
        let m = MetaVar(es.len());
        es.push(MetaEntry::Unsolved);
        m
    }

    pub fn lookup(&self, MetaVar(m): MetaVar) -> MetaEntry {
        // Out-of-bounds means the meta was created in a different metacontext
        // (e.g. an imported module's mcxt). Treat as Unsolved so the value
        // stays stuck rather than panicking — `quote` / `force` handle stuck
        // metas gracefully, and any genuine inference errors will surface
        // earlier during elaboration.
        self.entries
            .borrow()
            .get(m)
            .cloned()
            .unwrap_or(MetaEntry::Unsolved)
    }

    pub fn solve(&self, MetaVar(m): MetaVar, val: Val) {
        // Out-of-bounds means the meta was created in a different metacontext
        // (e.g. an imported module's mcxt or a host-side apply call). We
        // can't persist solutions there from here; silently skip.
        let mut entries = self.entries.borrow_mut();
        if m < entries.len() {
            entries[m] = MetaEntry::Solved(val);
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation (NbE)
// ---------------------------------------------------------------------------

impl Closure {
    pub fn apply(&self, mcxt: &MetaCxt, arg: Val) -> Result<Val, ElabError> {
        let mut env = self.env.clone();
        env.push(arg);
        eval(mcxt, &env, &self.body)
    }
}

pub fn v_app(mcxt: &MetaCxt, func: Val, arg: Val, icit: Icity) -> Result<Val, ElabError> {
    match func {
        Val::Lam(_, _, cl) => cl.apply(mcxt, arg),
        Val::Flex(m, mut sp) => {
            sp.push((arg, icit));
            Ok(Val::Flex(m, sp))
        }
        Val::Rigid(x, mut sp) => {
            sp.push((arg, icit));
            Ok(Val::Rigid(x, sp))
        }
        Val::Fix(name, cl) => {
            // Evaluate the body to get the inner lambda, apply fix to it, then apply arg
            let lam = cl.apply(mcxt, Val::Fix(name.clone(), cl.clone()))?;
            v_app(mcxt, lam, arg, icit)
        }
        _ => Err(ElabError::Internal(format!(
            "v_app: cannot apply {:?}",
            func
        ))),
    }
}

fn v_app_sp(mcxt: &MetaCxt, mut val: Val, sp: &Spine) -> Result<Val, ElabError> {
    for (arg, icit) in sp {
        val = v_app(mcxt, val, arg.clone(), *icit)?;
    }
    Ok(val)
}

fn v_app_bds(mcxt: &MetaCxt, env: &Env, mut val: Val, bds: &[BD]) -> Result<Val, ElabError> {
    for (i, bd) in bds.iter().enumerate() {
        if *bd == BD::Bound {
            val = v_app(mcxt, val, env[i].clone(), Icity::Explicit)?;
        }
    }
    Ok(val)
}

fn v_meta(mcxt: &MetaCxt, m: MetaVar) -> Val {
    match mcxt.lookup(m) {
        MetaEntry::Solved(v) => v,
        MetaEntry::Unsolved => Val::meta(m),
    }
}

pub fn force(mcxt: &MetaCxt, val: &Val) -> Val {
    match val {
        Val::Flex(m, sp) => match mcxt.lookup(*m) {
            MetaEntry::Solved(v) => {
                let result = v_app_sp(mcxt, v, sp);
                match result {
                    Ok(v) => force(mcxt, &v),
                    Err(_) => val.clone(),
                }
            }
            MetaEntry::Unsolved => val.clone(),
        },
        _ => val.clone(),
    }
}

pub fn eval(mcxt: &MetaCxt, env: &Env, tm: &Term) -> Result<Val, ElabError> {
    match tm {
        Term::Var(Ix(x)) => Ok(env[env.len() - 1 - x].clone()),

        Term::App(t, u, icit) => {
            let tv = eval(mcxt, env, t)?;
            let uv = eval(mcxt, env, u)?;
            v_app(mcxt, tv, uv, *icit)
        }

        Term::Lam(x, icit, body) => Ok(Val::Lam(
            x.clone(),
            *icit,
            Closure {
                env: env.clone(),
                body: *body.clone(),
            },
        )),

        Term::Pi(x, icit, a, b) => {
            let av = eval(mcxt, env, a)?;
            Ok(Val::Pi(
                x.clone(),
                *icit,
                Box::new(av),
                Closure {
                    env: env.clone(),
                    body: *b.clone(),
                },
            ))
        }

        Term::Let(_, _, t, u) => {
            let tv = eval(mcxt, env, t)?;
            let mut env2 = env.clone();
            env2.push(tv);
            eval(mcxt, &env2, u)
        }

        Term::U => Ok(Val::U),

        Term::Meta(m) => Ok(v_meta(mcxt, *m)),

        Term::InsertedMeta(m, bds) => {
            let v = v_meta(mcxt, *m);
            v_app_bds(mcxt, env, v, bds)
        }

        Term::Lit(lit) => Ok(Val::Lit(lit.clone())),
        Term::LitTy(tl) => match tl {
            TypeLit::Type => Ok(Val::U),
            _ => Ok(Val::LitTy(*tl)),
        },

        Term::Record(fields) => {
            let mut vals = Vec::new();
            for (name, tm) in fields {
                vals.push((name.clone(), eval(mcxt, env, tm)?));
            }
            Ok(Val::Record(vals))
        }

        Term::RecordTy(fields, tail) => {
            let mut vals = Vec::new();
            for (name, ty) in fields {
                vals.push((name.clone(), eval(mcxt, env, ty)?));
            }
            let tail_v = match tail {
                Some(t) => Some(Box::new(eval(mcxt, env, t)?)),
                None => None,
            };
            Ok(Val::RecordTy(vals, tail_v))
        }

        Term::RecordProj(t, field) => {
            let tv = eval(mcxt, env, t)?;
            eval_proj(mcxt, tv, field)
        }

        Term::Variant(tag, t) => {
            let tv = eval(mcxt, env, t)?;
            Ok(Val::Variant(tag.clone(), Box::new(tv)))
        }

        Term::VariantTy(tags, tail) => {
            let mut vals = Vec::new();
            for (name, ty) in tags {
                vals.push((name.clone(), eval(mcxt, env, ty)?));
            }
            let tail_v = match tail {
                Some(t) => Some(Box::new(eval(mcxt, env, t)?)),
                None => None,
            };
            Ok(Val::VariantTy(vals, tail_v))
        }

        Term::Case(scrut, branches) => {
            let sv = eval(mcxt, env, scrut)?;
            eval_case(mcxt, env, sv, branches)
        }

        Term::If(c, t, e) => {
            let cv = eval(mcxt, env, c)?;
            let cv = force(mcxt, &cv);
            match cv {
                Val::Lit(Lit::Bool(true)) => eval(mcxt, env, t),
                Val::Lit(Lit::Bool(false)) => eval(mcxt, env, e),
                other => Ok(Val::If(
                    Box::new(other),
                    Box::new(eval(mcxt, env, t)?),
                    Box::new(eval(mcxt, env, e)?),
                )),
            }
        }

        Term::Fix(name, body) => Ok(Val::Fix(
            name.clone(),
            Closure {
                env: env.clone(),
                body: *body.clone(),
            },
        )),

        Term::Mu(body) => Ok(Val::Mu(Closure {
            env: env.clone(),
            body: *body.clone(),
        })),

        Term::Fold(t) => {
            let tv = eval(mcxt, env, t)?;
            Ok(Val::Fold(Box::new(tv)))
        }

        Term::Unfold(t) => {
            let tv = eval(mcxt, env, t)?;
            let tv = force(mcxt, &tv);
            match tv {
                Val::Fold(v) => Ok(*v),
                // Stuck: variable or meta under unfold
                Val::Rigid(_, _) | Val::Flex(_, _) => Ok(Val::Rigid(Lvl(0), Vec::new())), // shouldn't happen in practice
                _ => Err(ElabError::Internal(format!(
                    "unfold: expected folded value, got {:?}",
                    tv
                ))),
            }
        }

        Term::BinOp(op, a, b) => {
            let av = eval(mcxt, env, a)?;
            let bv = eval(mcxt, env, b)?;
            Ok(eval_binop(mcxt, *op, av, bv))
        }

        Term::UnOp(op, a) => {
            let av = eval(mcxt, env, a)?;
            Ok(eval_unop(mcxt, *op, av))
        }
    }
}

/// Reduce a binary operator if both sides are literals; otherwise stay stuck.
/// The elaborator has already type-checked the operands, so the match arms
/// are exhaustive on the literal kinds an operator accepts.
fn eval_binop(mcxt: &MetaCxt, op: BinOp, a: Val, b: Val) -> Val {
    let av = force(mcxt, &a);
    let bv = force(mcxt, &b);
    match (&av, &bv) {
        (Val::Lit(la), Val::Lit(lb)) => Val::Lit(apply_binop(op, la, lb)),
        _ => Val::BinOp(op, Box::new(av), Box::new(bv)),
    }
}

fn eval_unop(mcxt: &MetaCxt, op: UnOp, a: Val) -> Val {
    let av = force(mcxt, &a);
    match (op, &av) {
        (UnOp::Not, Val::Lit(Lit::Bool(b))) => Val::Lit(Lit::Bool(!b)),
        _ => Val::UnOp(op, Box::new(av)),
    }
}

fn apply_binop(op: BinOp, a: &Lit, b: &Lit) -> Lit {
    use Lit::*;
    match (op, a, b) {
        // Equality on each comparable primitive.
        (BinOp::Eq, Integer(x), Integer(y)) => Bool(x == y),
        (BinOp::Eq, Double(x), Double(y)) => Bool(x == y),
        (BinOp::Eq, String(x), String(y)) => Bool(x == y),
        (BinOp::Eq, Bool(x), Bool(y)) => Bool(x == y),
        (BinOp::Eq, Char(x), Char(y)) => Bool(x == y),
        (BinOp::Eq, Unit, Unit) => Bool(true),

        (BinOp::Neq, Integer(x), Integer(y)) => Bool(x != y),
        (BinOp::Neq, Double(x), Double(y)) => Bool(x != y),
        (BinOp::Neq, String(x), String(y)) => Bool(x != y),
        (BinOp::Neq, Bool(x), Bool(y)) => Bool(x != y),
        (BinOp::Neq, Char(x), Char(y)) => Bool(x != y),
        (BinOp::Neq, Unit, Unit) => Bool(false),

        // Ordering on ordered primitives.
        (BinOp::Lt, Integer(x), Integer(y)) => Bool(x < y),
        (BinOp::Lt, Double(x), Double(y)) => Bool(x < y),
        (BinOp::Lt, Char(x), Char(y)) => Bool(x < y),

        (BinOp::Gt, Integer(x), Integer(y)) => Bool(x > y),
        (BinOp::Gt, Double(x), Double(y)) => Bool(x > y),
        (BinOp::Gt, Char(x), Char(y)) => Bool(x > y),

        (BinOp::Lte, Integer(x), Integer(y)) => Bool(x <= y),
        (BinOp::Lte, Double(x), Double(y)) => Bool(x <= y),
        (BinOp::Lte, Char(x), Char(y)) => Bool(x <= y),

        (BinOp::Gte, Integer(x), Integer(y)) => Bool(x >= y),
        (BinOp::Gte, Double(x), Double(y)) => Bool(x >= y),
        (BinOp::Gte, Char(x), Char(y)) => Bool(x >= y),

        // Logical ops are short-circuited in the typechecker too, but at this
        // point both sides have been evaluated already.
        (BinOp::And, Bool(x), Bool(y)) => Bool(*x && *y),
        (BinOp::Or, Bool(x), Bool(y)) => Bool(*x || *y),

        // Unreachable if the elaborator type-checked the operands. Reaching
        // here means a synthesis bug; fall back to false rather than panicking
        // so the evaluator stays total.
        _ => Bool(false),
    }
}

fn eval_proj(mcxt: &MetaCxt, val: Val, field: &str) -> Result<Val, ElabError> {
    let v = force(mcxt, &val);
    match v {
        Val::Record(fields) => {
            for (name, fv) in &fields {
                if name == field {
                    return Ok(fv.clone());
                }
            }
            Err(ElabError::Internal(format!(
                "record field not found: {field}"
            )))
        }
        // stuck
        other => Ok(Val::RecordProj(Box::new(other), field.to_string())),
    }
}

fn eval_case(
    mcxt: &MetaCxt,
    env: &Env,
    scrut: Val,
    branches: &[(Pat, Term)],
) -> Result<Val, ElabError> {
    let sv = force(mcxt, &scrut);

    // Try each branch
    for (pat, body) in branches {
        if let Some(bindings) = match_pat(&sv, pat) {
            let mut env2 = env.clone();
            env2.extend(bindings);
            return eval(mcxt, &env2, body);
        }
    }

    // If stuck (scrutinee is a variable/meta), produce stuck case
    match &sv {
        Val::Rigid(..) | Val::Flex(..) => {
            let mut val_branches = Vec::new();
            for (pat, body) in branches {
                val_branches.push((pat.clone(), eval(mcxt, env, body)?));
            }
            Ok(Val::Case(Box::new(sv), val_branches))
        }
        _ => Err(ElabError::Internal(format!(
            "case: no matching branch for {:?}",
            sv
        ))),
    }
}

/// Try to match a value against a pattern, returning bindings if successful.
fn match_pat(val: &Val, pat: &Pat) -> Option<Vec<Val>> {
    match pat {
        Pat::Var(_) => Some(vec![val.clone()]),
        Pat::Wildcard => Some(vec![]),
        Pat::Lit(l) => match val {
            Val::Lit(l2) if l == l2 => Some(vec![]),
            _ => None,
        },
        Pat::Variant(tag, inner_pat) => match val {
            Val::Variant(tag2, inner_val) if tag == tag2 => match_pat(inner_val, inner_pat),
            _ => None,
        },
        Pat::Record(rpats) => match val {
            Val::Record(fields) => {
                let mut bindings = Vec::new();
                for rpat in rpats {
                    match rpat {
                        RecordPat::Pun(name) => {
                            let fv = fields.iter().find(|(n, _)| n == name)?.1.clone();
                            bindings.push(fv);
                        }
                        RecordPat::Match(name, inner_pat) => {
                            let fv = fields.iter().find(|(n, _)| n == name)?.1.clone();
                            bindings.extend(match_pat(&fv, inner_pat)?);
                        }
                    }
                }
                Some(bindings)
            }
            _ => None,
        },
    }
}

// ---------------------------------------------------------------------------
// Meta freshening (cross-mcxt boundary)
// ---------------------------------------------------------------------------

/// Walk a Term and replace every meta reference with a fresh meta in the
/// given metacontext. Used at import boundaries: a quoted Val from a foreign
/// mcxt has `Term::Meta(m)` references whose m only makes sense in that
/// foreign mcxt. After freshening, all references live in `host` mcxt.
///
/// The `remap` hash keeps the freshening idempotent across the value+type
/// pair so the two stay structurally aligned.
pub fn freshen_metas_in_term(
    host: &MetaCxt,
    tm: &Term,
    remap: &mut std::collections::HashMap<MetaVar, MetaVar>,
) -> Term {
    let f = |t: &Term, r: &mut std::collections::HashMap<MetaVar, MetaVar>| {
        freshen_metas_in_term(host, t, r)
    };
    match tm {
        Term::Meta(m) => Term::Meta(freshen_one(host, *m, remap)),
        Term::InsertedMeta(m, bds) => {
            Term::InsertedMeta(freshen_one(host, *m, remap), bds.clone())
        }
        Term::Var(ix) => Term::Var(*ix),
        Term::Lam(n, ic, body) => Term::Lam(n.clone(), *ic, Box::new(f(body, remap))),
        Term::App(t, u, ic) => Term::App(Box::new(f(t, remap)), Box::new(f(u, remap)), *ic),
        Term::U => Term::U,
        Term::Pi(n, ic, a, b) => Term::Pi(
            n.clone(),
            *ic,
            Box::new(f(a, remap)),
            Box::new(f(b, remap)),
        ),
        Term::Let(n, ty, t, u) => Term::Let(
            n.clone(),
            Box::new(f(ty, remap)),
            Box::new(f(t, remap)),
            Box::new(f(u, remap)),
        ),
        Term::Lit(l) => Term::Lit(l.clone()),
        Term::LitTy(tl) => Term::LitTy(*tl),
        Term::Record(fields) => Term::Record(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), f(t, remap)))
                .collect(),
        ),
        Term::RecordTy(fields, tail) => Term::RecordTy(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), f(t, remap)))
                .collect(),
            tail.as_ref().map(|t| Box::new(f(t, remap))),
        ),
        Term::RecordProj(t, field) => {
            Term::RecordProj(Box::new(f(t, remap)), field.clone())
        }
        Term::Variant(tag, t) => Term::Variant(tag.clone(), Box::new(f(t, remap))),
        Term::VariantTy(tags, tail) => Term::VariantTy(
            tags.iter()
                .map(|(n, t)| (n.clone(), f(t, remap)))
                .collect(),
            tail.as_ref().map(|t| Box::new(f(t, remap))),
        ),
        Term::Case(scrut, branches) => Term::Case(
            Box::new(f(scrut, remap)),
            branches
                .iter()
                .map(|(p, body)| (p.clone(), f(body, remap)))
                .collect(),
        ),
        Term::If(c, t, e) => Term::If(
            Box::new(f(c, remap)),
            Box::new(f(t, remap)),
            Box::new(f(e, remap)),
        ),
        Term::Fix(n, body) => Term::Fix(n.clone(), Box::new(f(body, remap))),
        Term::Mu(body) => Term::Mu(Box::new(f(body, remap))),
        Term::Fold(t) => Term::Fold(Box::new(f(t, remap))),
        Term::Unfold(t) => Term::Unfold(Box::new(f(t, remap))),
        Term::BinOp(op, a, b) => {
            Term::BinOp(*op, Box::new(f(a, remap)), Box::new(f(b, remap)))
        }
        Term::UnOp(op, a) => Term::UnOp(*op, Box::new(f(a, remap))),
    }
}

fn freshen_one(
    host: &MetaCxt,
    m: MetaVar,
    remap: &mut std::collections::HashMap<MetaVar, MetaVar>,
) -> MetaVar {
    if let Some(&fresh) = remap.get(&m) {
        return fresh;
    }
    let fresh = host.fresh();
    remap.insert(m, fresh);
    fresh
}

// ---------------------------------------------------------------------------
// Quoting (Val → Term)
// ---------------------------------------------------------------------------

pub fn quote(mcxt: &MetaCxt, lvl: Lvl, val: &Val) -> Term {
    let v = force(mcxt, val);
    match v {
        Val::Flex(m, sp) => quote_sp(mcxt, lvl, Term::Meta(m), &sp),

        Val::Rigid(x, sp) => quote_sp(mcxt, lvl, Term::Var(x.to_ix(lvl)), &sp),

        Val::Lam(x, icit, cl) => {
            let body_val = cl.apply(mcxt, Val::var(lvl)).unwrap_or(Val::U);
            Term::Lam(x, icit, Box::new(quote(mcxt, lvl + 1, &body_val)))
        }

        Val::Pi(x, icit, a, cl) => {
            let a_tm = quote(mcxt, lvl, &a);
            let body_val = cl.apply(mcxt, Val::var(lvl)).unwrap_or(Val::U);
            Term::Pi(x, icit, Box::new(a_tm), Box::new(quote(mcxt, lvl + 1, &body_val)))
        }

        Val::U => Term::U,

        Val::Lit(l) => Term::Lit(l),
        Val::LitTy(tl) => Term::LitTy(tl),

        Val::Record(fields) => Term::Record(
            fields
                .iter()
                .map(|(n, v)| (n.clone(), quote(mcxt, lvl, v)))
                .collect(),
        ),

        Val::RecordTy(fields, tail) => Term::RecordTy(
            fields
                .iter()
                .map(|(n, v)| (n.clone(), quote(mcxt, lvl, v)))
                .collect(),
            tail.map(|t| Box::new(quote(mcxt, lvl, &t))),
        ),

        Val::RecordProj(t, field) => {
            Term::RecordProj(Box::new(quote(mcxt, lvl, &t)), field)
        }

        Val::Variant(tag, t) => Term::Variant(tag, Box::new(quote(mcxt, lvl, &t))),

        Val::VariantTy(tags, tail) => Term::VariantTy(
            tags.iter()
                .map(|(n, v)| (n.clone(), quote(mcxt, lvl, v)))
                .collect(),
            tail.map(|t| Box::new(quote(mcxt, lvl, &t))),
        ),

        Val::Case(scrut, branches) => Term::Case(
            Box::new(quote(mcxt, lvl, &scrut)),
            branches
                .iter()
                .map(|(p, v)| (p.clone(), quote(mcxt, lvl, v)))
                .collect(),
        ),

        Val::If(c, t, e) => Term::If(
            Box::new(quote(mcxt, lvl, &c)),
            Box::new(quote(mcxt, lvl, &t)),
            Box::new(quote(mcxt, lvl, &e)),
        ),

        Val::Fix(name, cl) => {
            // Quote as Fix(name, body) — apply fix variable at current level
            let body_val = cl.apply(mcxt, Val::var(lvl)).unwrap_or(Val::U);
            Term::Fix(name, Box::new(quote(mcxt, lvl + 1, &body_val)))
        }

        Val::Mu(cl) => {
            let body_val = cl.apply(mcxt, Val::var(lvl)).unwrap_or(Val::U);
            Term::Mu(Box::new(quote(mcxt, lvl + 1, &body_val)))
        }

        Val::Fold(v) => Term::Fold(Box::new(quote(mcxt, lvl, &v))),

        Val::BinOp(op, a, b) => Term::BinOp(
            op,
            Box::new(quote(mcxt, lvl, &a)),
            Box::new(quote(mcxt, lvl, &b)),
        ),

        Val::UnOp(op, a) => Term::UnOp(op, Box::new(quote(mcxt, lvl, &a))),
    }
}

fn quote_sp(mcxt: &MetaCxt, lvl: Lvl, mut head: Term, sp: &Spine) -> Term {
    for (arg, icit) in sp {
        head = Term::App(Box::new(head), Box::new(quote(mcxt, lvl, arg)), *icit);
    }
    head
}

pub fn nf(mcxt: &MetaCxt, env: &Env, tm: &Term) -> Result<Term, ElabError> {
    let val = eval(mcxt, env, tm)?;
    Ok(quote(mcxt, Lvl(env.len()), &val))
}

// ---------------------------------------------------------------------------
// Unification
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct UnifyError(pub String);

impl fmt::Display for UnifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unification error: {}", self.0)
    }
}

/// Partial renaming from Γ to Δ
struct PartialRenaming {
    dom: Lvl,
    cod: Lvl,
    ren: std::collections::HashMap<usize, Lvl>,
}

impl PartialRenaming {
    fn lift(&self) -> PartialRenaming {
        let mut ren = self.ren.clone();
        ren.insert(self.cod.0, self.dom);
        PartialRenaming {
            dom: self.dom + 1,
            cod: self.cod + 1,
            ren,
        }
    }
}

fn invert(mcxt: &MetaCxt, gamma: Lvl, sp: &Spine) -> Result<PartialRenaming, UnifyError> {
    let mut dom = Lvl(0);
    let mut ren = std::collections::HashMap::new();

    for (arg, _) in sp {
        let v = force(mcxt, arg);
        match v {
            Val::Rigid(Lvl(x), ref sp2) if sp2.is_empty() => {
                if ren.contains_key(&x) {
                    return Err(UnifyError("non-linear spine".into()));
                }
                ren.insert(x, dom);
                dom = dom + 1;
            }
            _ => return Err(UnifyError("non-variable in spine".into())),
        }
    }

    Ok(PartialRenaming {
        dom,
        cod: gamma,
        ren,
    })
}

fn rename(
    mcxt: &MetaCxt,
    m: MetaVar,
    pren: &PartialRenaming,
    val: &Val,
) -> Result<Term, UnifyError> {
    let v = force(mcxt, val);
    match v {
        Val::Flex(m2, sp) => {
            if m == m2 {
                return Err(UnifyError("occurs check".into()));
            }
            let mut head = Term::Meta(m2);
            for (arg, icit) in &sp {
                head = Term::App(
                    Box::new(head),
                    Box::new(rename(mcxt, m, pren, arg)?),
                    *icit,
                );
            }
            Ok(head)
        }

        Val::Rigid(Lvl(x), sp) => {
            let x2 = pren
                .ren
                .get(&x)
                .ok_or_else(|| UnifyError("escaping variable".into()))?;
            let mut head = Term::Var(x2.to_ix(pren.dom));
            for (arg, icit) in &sp {
                head = Term::App(
                    Box::new(head),
                    Box::new(rename(mcxt, m, pren, arg)?),
                    *icit,
                );
            }
            Ok(head)
        }

        Val::Lam(x, icit, cl) => {
            let body = cl
                .apply(mcxt, Val::var(pren.cod))
                .map_err(|e| UnifyError(format!("rename lam: {e}")))?;
            Ok(Term::Lam(
                x,
                icit,
                Box::new(rename(mcxt, m, &pren.lift(), &body)?),
            ))
        }

        Val::Pi(x, icit, a, cl) => {
            let a_tm = rename(mcxt, m, pren, &a)?;
            let body = cl
                .apply(mcxt, Val::var(pren.cod))
                .map_err(|e| UnifyError(format!("rename pi: {e}")))?;
            Ok(Term::Pi(
                x,
                icit,
                Box::new(a_tm),
                Box::new(rename(mcxt, m, &pren.lift(), &body)?),
            ))
        }

        Val::U => Ok(Term::U),
        Val::Lit(l) => Ok(Term::Lit(l)),
        Val::LitTy(tl) => Ok(Term::LitTy(tl)),

        Val::Record(fields) => {
            let mut fs = Vec::new();
            for (name, v) in &fields {
                fs.push((name.clone(), rename(mcxt, m, pren, v)?));
            }
            Ok(Term::Record(fs))
        }

        Val::RecordTy(fields, tail) => {
            let mut fs = Vec::new();
            for (name, v) in &fields {
                fs.push((name.clone(), rename(mcxt, m, pren, v)?));
            }
            let tail_t = match tail {
                Some(t) => Some(Box::new(rename(mcxt, m, pren, &t)?)),
                None => None,
            };
            Ok(Term::RecordTy(fs, tail_t))
        }

        Val::RecordProj(t, field) => Ok(Term::RecordProj(
            Box::new(rename(mcxt, m, pren, &t)?),
            field,
        )),

        Val::Variant(tag, t) => Ok(Term::Variant(tag, Box::new(rename(mcxt, m, pren, &t)?))),

        Val::VariantTy(tags, tail) => {
            let mut ts = Vec::new();
            for (name, v) in &tags {
                ts.push((name.clone(), rename(mcxt, m, pren, v)?));
            }
            let tail_t = match tail {
                Some(t) => Some(Box::new(rename(mcxt, m, pren, &t)?)),
                None => None,
            };
            Ok(Term::VariantTy(ts, tail_t))
        }

        Val::Mu(cl) => {
            let body = cl
                .apply(mcxt, Val::var(pren.cod))
                .map_err(|e| UnifyError(format!("rename mu: {e}")))?;
            Ok(Term::Mu(Box::new(rename(mcxt, m, &pren.lift(), &body)?)))
        }

        Val::Fold(v) => Ok(Term::Fold(Box::new(rename(mcxt, m, pren, &v)?))),

        // For stuck terms, we just fail — they shouldn't appear during unification
        _ => Err(UnifyError(format!(
            "rename: unexpected value {:?}",
            val
        ))),
    }
}

fn lams(icits: &[Icity], body: Term) -> Term {
    let mut t = body;
    for (i, icit) in icits.iter().enumerate().rev() {
        t = Term::Lam(format!("x{}", i), *icit, Box::new(t));
    }
    t
}

fn solve(mcxt: &MetaCxt, gamma: Lvl, m: MetaVar, sp: &Spine, rhs: &Val) -> Result<(), UnifyError> {
    let pren = invert(mcxt, gamma, sp)?;
    let rhs_tm = rename(mcxt, m, &pren, rhs)?;
    let icits: Vec<Icity> = sp.iter().map(|(_, i)| *i).collect();
    let solution = eval(mcxt, &Vec::new(), &lams(&icits, rhs_tm))
        .map_err(|e| UnifyError(format!("solve eval: {e}")))?;
    mcxt.solve(m, solution);
    Ok(())
}

fn unify_sp(mcxt: &MetaCxt, lvl: Lvl, sp1: &Spine, sp2: &Spine) -> Result<(), UnifyError> {
    if sp1.len() != sp2.len() {
        return Err(UnifyError("spine length mismatch".into()));
    }
    for ((a, _), (b, _)) in sp1.iter().zip(sp2.iter()) {
        unify(mcxt, lvl, a, b)?;
    }
    Ok(())
}

pub fn unify(mcxt: &MetaCxt, lvl: Lvl, t: &Val, u: &Val) -> Result<(), UnifyError> {
    let t = force(mcxt, t);
    let u = force(mcxt, u);

    match (&t, &u) {
        (Val::U, Val::U) => Ok(()),

        (Val::LitTy(a), Val::LitTy(b)) if a == b => Ok(()),

        (Val::Lit(a), Val::Lit(b)) if a == b => Ok(()),

        (Val::Pi(_, i1, a1, b1), Val::Pi(_, i2, a2, b2)) if i1 == i2 => {
            unify(mcxt, lvl, a1, a2)?;
            let v1 = b1
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            let v2 = b2
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            unify(mcxt, lvl + 1, &v1, &v2)
        }

        (Val::Lam(_, _, cl1), Val::Lam(_, _, cl2)) => {
            let v1 = cl1
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            let v2 = cl2
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            unify(mcxt, lvl + 1, &v1, &v2)
        }

        // Eta expansion for lambdas
        (Val::Lam(_, icit, cl), other) | (other, Val::Lam(_, icit, cl)) => {
            let v1 = cl
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            let v2 = v_app(mcxt, other.clone(), Val::var(lvl), *icit)
                .map_err(|e| UnifyError(format!("{e}")))?;
            unify(mcxt, lvl + 1, &v1, &v2)
        }

        (Val::Rigid(x1, sp1), Val::Rigid(x2, sp2)) if x1 == x2 => unify_sp(mcxt, lvl, sp1, sp2),

        (Val::Flex(m1, sp1), Val::Flex(m2, sp2)) if m1 == m2 => unify_sp(mcxt, lvl, sp1, sp2),

        (Val::Flex(m, sp), other) | (other, Val::Flex(m, sp)) => solve(mcxt, lvl, *m, sp, other),

        // Record types: row-polymorphic unification (handles tails).
        (Val::RecordTy(fs1, t1), Val::RecordTy(fs2, t2)) => {
            unify_record_ty(mcxt, lvl, fs1, t1.as_deref(), fs2, t2.as_deref())
        }

        // Variant types: row-polymorphic unification (handles tails).
        (Val::VariantTy(ts1, t1), Val::VariantTy(ts2, t2)) => {
            unify_variant_ty(mcxt, lvl, ts1, t1.as_deref(), ts2, t2.as_deref())
        }

        // Mu types: unify bodies under a shared binder
        (Val::Mu(cl1), Val::Mu(cl2)) => {
            let v1 = cl1
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            let v2 = cl2
                .apply(mcxt, Val::var(lvl))
                .map_err(|e| UnifyError(format!("{e}")))?;
            unify(mcxt, lvl + 1, &v1, &v2)
        }

        _ => Err(UnifyError(format!(
            "cannot unify: {:?} with {:?}",
            quote(mcxt, lvl, &t),
            quote(mcxt, lvl, &u)
        ))),
    }
}

/// Unify two record types with optional row tails.
///
/// Algorithm (Rémy/Wand):
/// 1. Force-flatten each side's tail (collapse chains of `Some(Rec{...; ...})`
///    into a single flat field list + a single terminal tail).
/// 2. Compute `common` (shared fields, unify pointwise), `only_left`, `only_right`.
/// 3. Branch on tail openness:
///    - Both closed:   require both extras empty (else `MissingField`).
///    - Left open:     solve left's tail := `Rec{only_right; None}` and require only_left empty.
///                     If left's tail is itself flex, this drives meta-solving.
///    - Right open:    dual.
///    - Both open:     mint shared fresh meta `rfresh`; solve left's tail := `Rec{only_right; rfresh}`
///                     and right's tail := `Rec{only_left; rfresh}`.
fn unify_record_ty(
    mcxt: &MetaCxt,
    lvl: Lvl,
    fs1: &[(String, Val)],
    t1: Option<&Val>,
    fs2: &[(String, Val)],
    t2: Option<&Val>,
) -> Result<(), UnifyError> {
    let (lfs, ltail) = flatten_row(mcxt, fs1.to_vec(), t1.cloned());
    let (rfs, rtail) = flatten_row(mcxt, fs2.to_vec(), t2.cloned());

    // Unify common fields and partition extras.
    let mut only_right: Vec<(String, Val)> = Vec::new();
    for (name, ty2) in &rfs {
        if let Some((_, ty1)) = lfs.iter().find(|(n, _)| n == name) {
            unify(mcxt, lvl, ty1, ty2)?;
        } else {
            only_right.push((name.clone(), ty2.clone()));
        }
    }
    let only_left: Vec<(String, Val)> = lfs
        .iter()
        .filter(|(n, _)| !rfs.iter().any(|(rn, _)| rn == n))
        .cloned()
        .collect();

    match (ltail, rtail) {
        (None, None) => {
            if !only_left.is_empty() {
                return Err(UnifyError(format!(
                    "missing record field on right: {}",
                    only_left[0].0
                )));
            }
            if !only_right.is_empty() {
                return Err(UnifyError(format!(
                    "missing record field on left: {}",
                    only_right[0].0
                )));
            }
            Ok(())
        }
        (Some(lt), None) => {
            if !only_left.is_empty() {
                return Err(UnifyError(format!(
                    "right record missing field: {}",
                    only_left[0].0
                )));
            }
            // Solve left tail to absorb the right's extras.
            unify(mcxt, lvl, &lt, &Val::RecordTy(only_right, None))
        }
        (None, Some(rt)) => {
            if !only_right.is_empty() {
                return Err(UnifyError(format!(
                    "left record missing field: {}",
                    only_right[0].0
                )));
            }
            unify(mcxt, lvl, &rt, &Val::RecordTy(only_left, None))
        }
        (Some(lt), Some(rt)) => {
            // Shared fresh meta keeps the two open tails connected so a
            // second unification pass terminates (Rémy/Wand).
            let rfresh_tm = {
                let mut es = mcxt.entries.borrow_mut();
                let m = MetaVar(es.len());
                es.push(MetaEntry::Unsolved);
                m
            };
            let rfresh = Val::meta(rfresh_tm);
            unify(
                mcxt,
                lvl,
                &lt,
                &Val::RecordTy(only_right, Some(Box::new(rfresh.clone()))),
            )?;
            unify(
                mcxt,
                lvl,
                &rt,
                &Val::RecordTy(only_left, Some(Box::new(rfresh))),
            )?;
            Ok(())
        }
    }
}

/// Unify two variant types with optional row tails. Mirrors `unify_record_ty`.
/// Notably this DROPS the prior `smaller ⊆ larger` subset rule, which was
/// unsound (case patterns weren't required to be exhaustive).
fn unify_variant_ty(
    mcxt: &MetaCxt,
    lvl: Lvl,
    ts1: &[(String, Val)],
    t1: Option<&Val>,
    ts2: &[(String, Val)],
    t2: Option<&Val>,
) -> Result<(), UnifyError> {
    let (lts, ltail) = flatten_variant_row(mcxt, ts1.to_vec(), t1.cloned());
    let (rts, rtail) = flatten_variant_row(mcxt, ts2.to_vec(), t2.cloned());

    let mut only_right: Vec<(String, Val)> = Vec::new();
    for (name, ty2) in &rts {
        if let Some((_, ty1)) = lts.iter().find(|(n, _)| n == name) {
            unify(mcxt, lvl, ty1, ty2)?;
        } else {
            only_right.push((name.clone(), ty2.clone()));
        }
    }
    let only_left: Vec<(String, Val)> = lts
        .iter()
        .filter(|(n, _)| !rts.iter().any(|(rn, _)| rn == n))
        .cloned()
        .collect();

    match (ltail, rtail) {
        (None, None) => {
            if !only_left.is_empty() {
                return Err(UnifyError(format!(
                    "missing variant tag on right: {}",
                    only_left[0].0
                )));
            }
            if !only_right.is_empty() {
                return Err(UnifyError(format!(
                    "missing variant tag on left: {}",
                    only_right[0].0
                )));
            }
            Ok(())
        }
        (Some(lt), None) => {
            if !only_left.is_empty() {
                return Err(UnifyError(format!(
                    "right variant missing tag: {}",
                    only_left[0].0
                )));
            }
            unify(mcxt, lvl, &lt, &Val::VariantTy(only_right, None))
        }
        (None, Some(rt)) => {
            if !only_right.is_empty() {
                return Err(UnifyError(format!(
                    "left variant missing tag: {}",
                    only_right[0].0
                )));
            }
            unify(mcxt, lvl, &rt, &Val::VariantTy(only_left, None))
        }
        (Some(lt), Some(rt)) => {
            let rfresh_tm = {
                let mut es = mcxt.entries.borrow_mut();
                let m = MetaVar(es.len());
                es.push(MetaEntry::Unsolved);
                m
            };
            let rfresh = Val::meta(rfresh_tm);
            unify(
                mcxt,
                lvl,
                &lt,
                &Val::VariantTy(only_right, Some(Box::new(rfresh.clone()))),
            )?;
            unify(
                mcxt,
                lvl,
                &rt,
                &Val::VariantTy(only_left, Some(Box::new(rfresh))),
            )?;
            Ok(())
        }
    }
}

/// Flatten a row chain: walk a record-type tail; if the tail forces to
/// another `Val::RecordTy`, splice its fields into the outer list and
/// recurse. Returns (flat field list, terminal tail).
fn flatten_row(
    mcxt: &MetaCxt,
    mut fields: Vec<(String, Val)>,
    tail: Option<Val>,
) -> (Vec<(String, Val)>, Option<Val>) {
    match tail {
        None => (fields, None),
        Some(t) => match force(mcxt, &t) {
            Val::RecordTy(more, more_tail) => {
                for (n, v) in more {
                    if !fields.iter().any(|(fn_, _)| fn_ == &n) {
                        fields.push((n, v));
                    }
                }
                flatten_row(mcxt, fields, more_tail.map(|b| *b))
            }
            other => (fields, Some(other)),
        },
    }
}

/// Variant counterpart to `flatten_row`.
fn flatten_variant_row(
    mcxt: &MetaCxt,
    mut tags: Vec<(String, Val)>,
    tail: Option<Val>,
) -> (Vec<(String, Val)>, Option<Val>) {
    match tail {
        None => (tags, None),
        Some(t) => match force(mcxt, &t) {
            Val::VariantTy(more, more_tail) => {
                for (n, v) in more {
                    if !tags.iter().any(|(fn_, _)| fn_ == &n) {
                        tags.push((n, v));
                    }
                }
                flatten_variant_row(mcxt, tags, more_tail.map(|b| *b))
            }
            other => (tags, Some(other)),
        },
    }
}

// ---------------------------------------------------------------------------
// Elaboration errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SpannedError {
    pub error: ElabError,
    pub span: Option<Span>,
}

#[derive(Debug, Clone)]
pub enum ElabError {
    NameNotInScope(String),
    CantUnify(String, String),
    CantInfer(String),
    IcitMismatch(Icity, Icity),
    NoNamedImplicit(String),
    DuplicateField(String),
    MissingField(String),
    ExtraField(String),
    Internal(String),
}

impl fmt::Display for ElabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElabError::NameNotInScope(n) => write!(f, "name not in scope: {n}"),
            ElabError::CantUnify(a, b) => {
                write!(f, "cannot unify\n  expected: {a}\n  inferred: {b}")
            }
            ElabError::CantInfer(s) => write!(f, "cannot infer type of: {s}"),
            ElabError::IcitMismatch(a, b) => {
                write!(f, "icity mismatch: expected {a:?}, got {b:?}")
            }
            ElabError::NoNamedImplicit(n) => write!(f, "no named implicit: {n}"),
            ElabError::DuplicateField(n) => write!(f, "duplicate field: {n}"),
            ElabError::MissingField(n) => write!(f, "missing field: {n}"),
            ElabError::ExtraField(n) => write!(f, "extra field: {n}"),
            ElabError::Internal(s) => write!(f, "internal error: {s}"),
        }
    }
}

impl fmt::Display for SpannedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

// ---------------------------------------------------------------------------
// Elaboration context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameOrigin {
    Source,
    Inserted,
}

/// Import resolver callback: given an import path string, returns (value, type).
pub type ImportResolver<'a> = dyn Fn(&str) -> Result<(Val, Val), String> + 'a;

pub struct Cxt<'a> {
    pub env: Env,
    pub lvl: Lvl,
    pub types: Vec<(Name, NameOrigin, VTy)>,
    pub bds: Vec<BD>,
    pub mcxt: &'a MetaCxt,
    pub span: Option<Span>,
    /// Optional span→type map for LSP hover. Shared across all cloned contexts.
    pub hover_map: Option<Rc<RefCell<Vec<(Span, String)>>>>,
    /// Optional import resolver for module system.
    pub resolver: Option<&'a ImportResolver<'a>>,
}

impl<'a> Cxt<'a> {
    pub fn new(mcxt: &'a MetaCxt) -> Self {
        Cxt {
            env: Vec::new(),
            lvl: Lvl(0),
            types: Vec::new(),
            bds: Vec::new(),
            mcxt,
            span: None,
            hover_map: None,
            resolver: None,
        }
    }

    pub fn new_with_hover(mcxt: &'a MetaCxt) -> Self {
        Cxt {
            env: Vec::new(),
            lvl: Lvl(0),
            types: Vec::new(),
            bds: Vec::new(),
            mcxt,
            span: None,
            hover_map: Some(Rc::new(RefCell::new(Vec::new()))),
            resolver: None,
        }
    }

    pub fn with_resolver(mcxt: &'a MetaCxt, resolver: &'a ImportResolver<'a>) -> Self {
        Cxt {
            env: Vec::new(),
            lvl: Lvl(0),
            types: Vec::new(),
            bds: Vec::new(),
            mcxt,
            span: None,
            hover_map: None,
            resolver: Some(resolver),
        }
    }

    pub fn bind(&self, name: Name, ty: VTy) -> Cxt<'a> {
        let mut env = self.env.clone();
        env.push(Val::var(self.lvl));
        let mut types = self.types.clone();
        types.push((name, NameOrigin::Source, ty));
        let mut bds = self.bds.clone();
        bds.push(BD::Bound);
        Cxt {
            env,
            lvl: self.lvl + 1,
            types,
            bds,
            mcxt: self.mcxt,
            span: self.span,
            hover_map: self.hover_map.clone(),
            resolver: self.resolver,
        }
    }

    pub fn new_binder(&self, name: Name, ty: VTy) -> Cxt<'a> {
        let mut env = self.env.clone();
        env.push(Val::var(self.lvl));
        let mut types = self.types.clone();
        types.push((name, NameOrigin::Inserted, ty));
        let mut bds = self.bds.clone();
        bds.push(BD::Bound);
        Cxt {
            env,
            lvl: self.lvl + 1,
            types,
            bds,
            mcxt: self.mcxt,
            span: self.span,
            hover_map: self.hover_map.clone(),
            resolver: self.resolver,
        }
    }

    pub fn define(&self, name: Name, val: Val, ty: VTy) -> Cxt<'a> {
        let mut env = self.env.clone();
        env.push(val);
        let mut types = self.types.clone();
        types.push((name, NameOrigin::Source, ty));
        let mut bds = self.bds.clone();
        bds.push(BD::Defined);
        Cxt {
            env,
            lvl: self.lvl + 1,
            types,
            bds,
            mcxt: self.mcxt,
            span: self.span,
            hover_map: self.hover_map.clone(),
            resolver: self.resolver,
        }
    }

    pub fn with_span(&self, span: Span) -> Cxt<'a> {
        Cxt {
            env: self.env.clone(),
            lvl: self.lvl,
            types: self.types.clone(),
            bds: self.bds.clone(),
            mcxt: self.mcxt,
            span: Some(span),
            hover_map: self.hover_map.clone(),
            resolver: self.resolver,
        }
    }

    pub fn fresh_meta(&self) -> Term {
        let m = self.mcxt.fresh();
        Term::InsertedMeta(m, self.bds.clone())
    }

    /// Mint a meta WITHOUT applying the current bound-variable spine. Used
    /// when refining a flex value to a structured value: the structured
    /// value's inner metas must NOT reference scope-local vars that aren't
    /// in the outer meta's spine, else the solver's renaming step fails.
    pub fn fresh_meta_unbound(&self) -> Term {
        let m = self.mcxt.fresh();
        Term::Meta(m)
    }

    pub fn eval(&self, tm: &Term) -> Result<Val, ElabError> {
        eval(self.mcxt, &self.env, tm)
    }

    pub fn quote(&self, val: &Val) -> Term {
        quote(self.mcxt, self.lvl, val)
    }

    pub fn close_val(&self, val: &Val) -> Closure {
        Closure {
            env: self.env.clone(),
            body: quote(self.mcxt, self.lvl + 1, val),
        }
    }

    fn err(&self, e: ElabError) -> SpannedError {
        SpannedError {
            error: e,
            span: self.span,
        }
    }

    fn unify_catch(&self, expected: &Val, inferred: &Val) -> Result<(), SpannedError> {
        unify(self.mcxt, self.lvl, expected, inferred).map_err(|_| {
            let exp_tm = self.quote(expected);
            let inf_tm = self.quote(inferred);
            self.err(ElabError::CantUnify(
                format!("{}", TermPrinter(&exp_tm)),
                format!("{}", TermPrinter(&inf_tm)),
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// Elaboration (bidirectional type checking)
// ---------------------------------------------------------------------------

use crate::ast::{ExprKind, Spanned};

/// Insert fresh implicit applications
fn insert(cxt: &Cxt, tm: Term, ty: VTy) -> Result<(Term, VTy), SpannedError> {
    // Don't insert into implicit lambdas
    if matches!(&tm, Term::Lam(_, Icity::Implicit, _)) {
        return Ok((tm, ty));
    }
    insert_inner(cxt, tm, ty)
}

fn insert_inner(cxt: &Cxt, mut tm: Term, mut ty: VTy) -> Result<(Term, VTy), SpannedError> {
    loop {
        let fty = force(cxt.mcxt, &ty);
        match fty {
            Val::Pi(_, Icity::Implicit, _, ref b) => {
                let meta = cxt.fresh_meta();
                let mv = cxt.eval(&meta).map_err(|e| cxt.err(e))?;
                tm = Term::App(Box::new(tm), Box::new(meta), Icity::Implicit);
                ty = b.apply(cxt.mcxt, mv).map_err(|e| cxt.err(e))?;
            }
            _ => return Ok((tm, fty)),
        }
    }
}

pub fn check(cxt: &Cxt, expr: &Spanned<ExprKind>, expected: &VTy) -> Result<Term, SpannedError> {
    let cxt = &cxt.with_span(expr.span);
    let exp = force(cxt.mcxt, expected);

    match (&expr.node, &exp) {
        // Lambda checking against Pi
        (ExprKind::Lam(pats, body), Val::Pi(x, icit, a, b)) if pats.len() == 1 && *icit == Icity::Explicit => {
            match &pats[0].node {
                crate::ast::PatternKind::Var(pname) => {
                    let bv = b
                        .apply(cxt.mcxt, Val::var(cxt.lvl))
                        .map_err(|e| cxt.err(e))?;
                    let inner_cxt = cxt.bind(pname.clone(), *a.clone());
                    let body_tm = check(&inner_cxt, body, &bv)?;
                    Ok(Term::Lam(pname.clone(), *icit, Box::new(body_tm)))
                }
                crate::ast::PatternKind::Wildcard => {
                    let bv = b
                        .apply(cxt.mcxt, Val::var(cxt.lvl))
                        .map_err(|e| cxt.err(e))?;
                    let inner_cxt = cxt.bind(x.clone(), *a.clone());
                    let body_tm = check(&inner_cxt, body, &bv)?;
                    Ok(Term::Lam(x.clone(), *icit, Box::new(body_tm)))
                }
                _ => {
                    // Fall through to infer
                    check_fallback(cxt, expr, &exp)
                }
            }
        }

        // Insert implicit lambda if Pi is implicit but expr is not implicit lambda
        (_, Val::Pi(x, Icity::Implicit, a, b)) => {
            let bv = b
                .apply(cxt.mcxt, Val::var(cxt.lvl))
                .map_err(|e| cxt.err(e))?;
            let inner_cxt = cxt.new_binder(x.clone(), *a.clone());
            let body_tm = check(&inner_cxt, expr, &bv)?;
            Ok(Term::Lam(x.clone(), Icity::Implicit, Box::new(body_tm)))
        }

        // Multi-param lambda: desugar
        (ExprKind::Lam(pats, body), _) if pats.len() > 1 => {
            // Desugar \x y => body  into  \x => \y => body
            let mut result = body.clone();
            for pat in pats[1..].iter().rev() {
                result = Box::new(Spanned::new(
                    ExprKind::Lam(vec![pat.clone()], result),
                    expr.span,
                ));
            }
            let desugared = Spanned::new(
                ExprKind::Lam(vec![pats[0].clone()], result),
                expr.span,
            );
            check(cxt, &desugared, &exp)
        }

        // Let
        (ExprKind::Let(bindings, body), _) => check_let(cxt, bindings, body, &exp),

        // Hole
        (ExprKind::Hole(_), _) => Ok(cxt.fresh_meta()),

        // Variant checked against variant type — check payload against the tag's type.
        // With row poly we also accept tags hidden behind a still-open tail:
        // if the explicit tag list doesn't contain the constructed tag, we
        // unify the expected type against `< 'tag <fresh> ; <fresh row> >`
        // (the tag joins the row).
        (ExprKind::Variant(tag, payload), Val::VariantTy(tags, tail)) => {
            let payload_ty = if let Some((_, ty)) = tags.iter().find(|(n, _)| n == tag) {
                ty.clone()
            } else if tail.is_some() {
                let fld_meta = cxt.fresh_meta();
                let fld_ty = cxt.eval(&fld_meta).map_err(|e| cxt.err(e))?;
                let row_meta = cxt.fresh_meta();
                let row_ty = cxt.eval(&row_meta).map_err(|e| cxt.err(e))?;
                let extended = Val::VariantTy(
                    vec![(tag.clone(), fld_ty.clone())],
                    Some(Box::new(row_ty)),
                );
                cxt.unify_catch(&Val::VariantTy(tags.clone(), tail.clone()), &extended)?;
                fld_ty
            } else {
                return Err(cxt.err(ElabError::Internal(format!(
                    "variant tag '{tag} not in expected type"
                ))));
            };
            let payload_tm = match payload {
                Some(e) => check(cxt, e, &payload_ty)?,
                None => {
                    cxt.unify_catch(&payload_ty, &Val::LitTy(TypeLit::Unit))?;
                    Term::Lit(Lit::Unit)
                }
            };
            Ok(Term::Variant(tag.clone(), Box::new(payload_tm)))
        }

        // Fold into Mu type
        (ExprKind::Fold(inner), Val::Mu(cl)) => {
            // F[Mu F / self] — substitute the Mu type for the bound variable
            let mu_val = exp.clone();
            let unfolded_ty = cl.apply(cxt.mcxt, mu_val).map_err(|e| cxt.err(e))?;
            let inner_tm = check(cxt, inner, &unfolded_ty)?;
            Ok(Term::Fold(Box::new(inner_tm)))
        }

        // Record literal — checked against an expected record type. If the
        // expected type has an open tail, extra literal fields are absorbed
        // into the tail (solved to a RecordTy of those extras).
        (ExprKind::Record(fields), Val::RecordTy(type_fields, tail)) => {
            check_record_lit(cxt, fields, type_fields, tail.as_deref())
        }

        // Record update: { ...base, field = value }
        (ExprKind::RecordUpdate(base, overrides), Val::RecordTy(type_fields, _tail)) => {
            check_record_update(cxt, base, overrides, type_fields)
        }

        // Fallback: infer and unify
        _ => check_fallback(cxt, expr, &exp),
    }
}

fn check_fallback(
    cxt: &Cxt,
    expr: &Spanned<ExprKind>,
    expected: &Val,
) -> Result<Term, SpannedError> {
    let (tm, inferred) = infer(cxt, expr)?;
    let (tm, inferred) = insert(cxt, tm, inferred)?;
    cxt.unify_catch(expected, &inferred)?;
    Ok(tm)
}

fn check_record_lit(
    cxt: &Cxt,
    fields: &[crate::ast::RecordField],
    type_fields: &[(String, Val)],
    tail: Option<&Val>,
) -> Result<Term, SpannedError> {
    let mut result_fields = Vec::new();

    for (name, ty) in type_fields {
        let field = fields
            .iter()
            .find(|f| &f.name == name)
            .ok_or_else(|| cxt.err(ElabError::MissingField(name.clone())))?;
        let tm = check(cxt, &field.value, ty)?;
        result_fields.push((name.clone(), tm));
    }

    // Extras: forbidden when the expected type is closed; absorbed into the
    // row tail when open (solve tail := Rec{extras}).
    let mut extras = Vec::new();
    for f in fields {
        if !type_fields.iter().any(|(n, _)| n == &f.name) {
            extras.push(f);
        }
    }
    if !extras.is_empty() {
        match tail {
            None => return Err(cxt.err(ElabError::ExtraField(extras[0].name.clone()))),
            Some(tail_v) => {
                // Each extra field's value is inferred; build a record type
                // of those inferred field types and unify against the tail.
                let mut extra_tys: Vec<(String, Val)> = Vec::new();
                for f in &extras {
                    let (tm, ty) = infer(cxt, &f.value)?;
                    result_fields.push((f.name.clone(), tm));
                    extra_tys.push((f.name.clone(), ty));
                }
                cxt.unify_catch(tail_v, &Val::RecordTy(extra_tys, None))?;
            }
        }
    }

    Ok(Term::Record(result_fields))
}

fn check_record_update(
    cxt: &Cxt,
    base: &crate::ast::Expr,
    overrides: &[crate::ast::RecordField],
    type_fields: &[(String, Val)],
) -> Result<Term, SpannedError> {
    // Infer the base record
    let (base_tm, base_ty) = infer(cxt, base)?;
    let (base_tm, base_ty) = insert(cxt, base_tm, base_ty)?;
    let base_ty = force(cxt.mcxt, &base_ty);

    // Base must be a record type
    match &base_ty {
        Val::RecordTy(base_fields, _base_tail) => {
            // Check that base has all fields we need (that aren't overridden)
            let mut result_fields = Vec::new();
            for (name, ty) in type_fields {
                if let Some(field) = overrides.iter().find(|f| &f.name == name) {
                    // Override field: check against expected type
                    let tm = check(cxt, &field.value, ty)?;
                    result_fields.push((name.clone(), tm));
                } else if base_fields.iter().any(|(n, _)| n == name) {
                    // Project from base
                    result_fields.push((
                        name.clone(),
                        Term::RecordProj(Box::new(base_tm.clone()), name.clone()),
                    ));
                } else {
                    return Err(cxt.err(ElabError::MissingField(name.clone())));
                }
            }

            // Check no extra override fields
            for f in overrides {
                if !type_fields.iter().any(|(n, _)| n == &f.name) {
                    return Err(cxt.err(ElabError::ExtraField(f.name.clone())));
                }
            }

            Ok(Term::Record(result_fields))
        }
        _ => Err(cxt.err(ElabError::CantInfer(
            "record spread base must be a record".into(),
        ))),
    }
}

pub fn infer(cxt: &Cxt, expr: &Spanned<ExprKind>) -> Result<(Term, VTy), SpannedError> {
    let result = infer_inner(cxt, expr)?;
    if let Some(ref map) = cxt.hover_map {
        let ty_tm = cxt.quote(&result.1);
        let ty_nf = nf(cxt.mcxt, &cxt.env, &ty_tm).unwrap_or(ty_tm);
        map.borrow_mut().push((expr.span, format!("{}", TermPrinter(&ty_nf))));
    }
    Ok(result)
}

fn infer_inner(cxt: &Cxt, expr: &Spanned<ExprKind>) -> Result<(Term, VTy), SpannedError> {
    let cxt = &cxt.with_span(expr.span);

    match &expr.node {
        ExprKind::Var(name) => {
            // Search context from the end (most recent binding first)
            for (ix, (n, origin, ty)) in cxt.types.iter().enumerate().rev() {
                if n == name && *origin == NameOrigin::Source {
                    let db_ix = Ix(cxt.types.len() - 1 - ix);
                    return Ok((Term::Var(db_ix), ty.clone()));
                }
            }
            Err(cxt.err(ElabError::NameNotInScope(name.clone())))
        }

        ExprKind::Lit(lit) => Ok(infer_lit(lit)),

        ExprKind::TypeLit(tl) => Ok((Term::LitTy(*tl), Val::U)),

        ExprKind::Undefined => {
            Err(cxt.err(ElabError::CantInfer("undefined".into())))
        }

        ExprKind::Lam(pats, body) if pats.len() == 1 => {
            match &pats[0].node {
                crate::ast::PatternKind::Var(name) => {
                    // Parameter type uses an UNBOUND meta (no spine of the
                    // current bound-vars). This keeps the meta in the
                    // pattern fragment for the unifier, which matters when
                    // the parameter is later refined by record-access
                    // inference (the partial-record introduces unbound
                    // field metas that need to substitute in cleanly).
                    // Dependent-typed callers needing the binder in the
                    // domain meta's spine should annotate the parameter.
                    let a_meta = cxt.fresh_meta_unbound();
                    let a = cxt.eval(&a_meta).map_err(|e| cxt.err(e))?;
                    let inner_cxt = cxt.bind(name.clone(), a.clone());
                    let (body_tm, body_ty) = infer(&inner_cxt, body)?;
                    let (body_tm, body_ty) = insert(&inner_cxt, body_tm, body_ty)?;
                    let cl = cxt.close_val(&body_ty);
                    Ok((
                        Term::Lam(name.clone(), Icity::Explicit, Box::new(body_tm)),
                        Val::Pi(name.clone(), Icity::Explicit, Box::new(a), cl),
                    ))
                }
                _ => Err(cxt.err(ElabError::CantInfer("lambda with complex pattern".into()))),
            }
        }

        ExprKind::Lam(pats, body) if pats.len() > 1 => {
            // Desugar to nested single-param lambdas, innermost binding
            // wrapping the body and the outermost being pats[0].
            //
            // Walk patterns in reverse order. After the loop the outermost
            // wrap (pats[0]) is in place — no separate manual wrap needed.
            let mut result = body.clone();
            for pat in pats.iter().rev() {
                result = Box::new(Spanned::new(
                    ExprKind::Lam(vec![pat.clone()], result),
                    expr.span,
                ));
            }
            infer(cxt, &result)
        }

        ExprKind::App(func, args) => {
            let (mut t, mut tty) = infer(cxt, func)?;

            for arg in args {
                // Insert implicit args before explicit ones
                if arg.icity == Icity::Explicit {
                    let res = insert_inner(cxt, t, tty)?;
                    t = res.0;
                    tty = res.1;
                }

                let fty = force(cxt.mcxt, &tty);
                match fty {
                    Val::Pi(_x, pi_icit, a, b) => {
                        if arg.icity != pi_icit {
                            return Err(cxt.err(ElabError::IcitMismatch(pi_icit, arg.icity)));
                        }
                        let u = check(cxt, &arg.expr, &a)?;
                        let uv = cxt.eval(&u).map_err(|e| cxt.err(e))?;
                        tty = b.apply(cxt.mcxt, uv).map_err(|e| cxt.err(e))?;
                        t = Term::App(Box::new(t), Box::new(u), arg.icity);
                    }
                    _ => {
                        // Try to create a Pi via unification
                        let a_meta = cxt.fresh_meta();
                        let a = cxt.eval(&a_meta).map_err(|e| cxt.err(e))?;
                        let b_cl = {
                            let inner_cxt = cxt.bind("x".into(), a.clone());
                            let b_meta = inner_cxt.fresh_meta();
                            Closure {
                                env: cxt.env.clone(),
                                body: b_meta,
                            }
                        };
                        let pi = Val::Pi("x".into(), arg.icity, Box::new(a.clone()), b_cl.clone());
                        cxt.unify_catch(&fty, &pi)?;

                        let u = check(cxt, &arg.expr, &a)?;
                        let uv = cxt.eval(&u).map_err(|e| cxt.err(e))?;
                        tty = b_cl.apply(cxt.mcxt, uv).map_err(|e| cxt.err(e))?;
                        t = Term::App(Box::new(t), Box::new(u), arg.icity);
                    }
                }
            }

            Ok((t, tty))
        }

        ExprKind::Ann(expr_inner, ty_expr) => {
            let ty_tm = check(cxt, ty_expr, &Val::U)?;
            let ty_val = cxt.eval(&ty_tm).map_err(|e| cxt.err(e))?;
            let tm = check(cxt, expr_inner, &ty_val)?;
            Ok((tm, ty_val))
        }

        ExprKind::Pi(params, ret) => {
            check_pi(cxt, params, ret)
        }

        ExprKind::Arrow(a, b) => {
            let a_tm = check(cxt, a, &Val::U)?;
            let a_val = cxt.eval(&a_tm).map_err(|e| cxt.err(e))?;
            let inner_cxt = cxt.bind("_".into(), a_val);
            let b_tm = check(&inner_cxt, b, &Val::U)?;
            Ok((
                Term::Pi("_".into(), Icity::Explicit, Box::new(a_tm), Box::new(b_tm)),
                Val::U,
            ))
        }

        ExprKind::Let(bindings, body) => infer_let(cxt, bindings, body),

        ExprKind::If(cond, then_e, else_e) => {
            let c = check(cxt, cond, &Val::LitTy(TypeLit::Bool))?;
            let (t, tty) = infer(cxt, then_e)?;
            let e = check(cxt, else_e, &tty)?;
            Ok((Term::If(Box::new(c), Box::new(t), Box::new(e)), tty))
        }

        ExprKind::Case(scrut, branches) => infer_case(cxt, scrut, branches),

        ExprKind::Record(fields) => {
            let mut tm_fields = Vec::new();
            let mut ty_fields = Vec::new();
            for f in fields {
                let (ftm, fty) = infer(cxt, &f.value)?;
                tm_fields.push((f.name.clone(), ftm));
                ty_fields.push((f.name.clone(), fty));
            }
            // Inferred record literals are closed (no tail).
            Ok((Term::Record(tm_fields), Val::RecordTy(ty_fields, None)))
        }

        ExprKind::RecordUpdate(base, overrides) => {
            // Infer base, must be a record type
            let (base_tm, base_ty) = infer(cxt, base)?;
            let (base_tm, base_ty) = insert(cxt, base_tm, base_ty)?;
            let base_ty = force(cxt.mcxt, &base_ty);
            match &base_ty {
                Val::RecordTy(base_fields, base_tail) => {
                    let mut tm_fields = Vec::new();
                    let mut ty_fields = Vec::new();

                    // Start with base fields, overriding as needed
                    for (name, fty) in base_fields {
                        if let Some(field) = overrides.iter().find(|f| &f.name == name) {
                            let (ftm, inferred_ty) = infer(cxt, &field.value)?;
                            tm_fields.push((name.clone(), ftm));
                            ty_fields.push((name.clone(), inferred_ty));
                        } else {
                            tm_fields.push((
                                name.clone(),
                                Term::RecordProj(Box::new(base_tm.clone()), name.clone()),
                            ));
                            ty_fields.push((name.clone(), fty.clone()));
                        }
                    }

                    // Add any new fields from overrides not in base
                    for f in overrides {
                        if !base_fields.iter().any(|(n, _)| n == &f.name) {
                            let (ftm, fty) = infer(cxt, &f.value)?;
                            tm_fields.push((f.name.clone(), ftm));
                            ty_fields.push((f.name.clone(), fty));
                        }
                    }

                    // The update result preserves the base's openness.
                    Ok((Term::Record(tm_fields), Val::RecordTy(ty_fields, base_tail.clone())))
                }
                _ => Err(cxt.err(ElabError::CantInfer(
                    "record spread base must be a record".into(),
                ))),
            }
        }

        ExprKind::RecordType(fields, tail) => {
            let mut ty_fields = Vec::new();
            for f in fields {
                let ftm = check(cxt, &f.ty, &Val::U)?;
                ty_fields.push((f.name.clone(), ftm));
            }
            let tail_tm = match tail {
                Some(t) => Some(Box::new(check(cxt, t, &Val::U)?)),
                None => None,
            };
            Ok((Term::RecordTy(ty_fields, tail_tm), Val::U))
        }

        ExprKind::RecordAccess(record, field) => {
            let (r, rty) = infer(cxt, record)?;
            let rty = force(cxt.mcxt, &rty);
            match &rty {
                Val::RecordTy(type_fields, tail) => {
                    if let Some((_, fty)) = type_fields.iter().find(|(n, _)| n == field) {
                        return Ok((Term::RecordProj(Box::new(r), field.clone()), fty.clone()));
                    }
                    // Field not in the explicit list: if the type is open,
                    // extend the row with `{field : ?fld_ty ; ?row}` so the
                    // tail meta grows to include this field. Closed: error.
                    //
                    // The fresh metas are unbound (no current-scope spine)
                    // so they can be substituted into the outer record-type
                    // meta without renaming failing on scope-local vars.
                    match tail {
                        Some(_) => {
                            let fld_meta = cxt.fresh_meta_unbound();
                            let fld_ty = cxt.eval(&fld_meta).map_err(|e| cxt.err(e))?;
                            let row_meta = cxt.fresh_meta_unbound();
                            let row_ty = cxt.eval(&row_meta).map_err(|e| cxt.err(e))?;
                            let extended = Val::RecordTy(
                                vec![(field.clone(), fld_ty.clone())],
                                Some(Box::new(row_ty)),
                            );
                            cxt.unify_catch(
                                &Val::RecordTy(type_fields.clone(), tail.clone()),
                                &extended,
                            )?;
                            Ok((Term::RecordProj(Box::new(r), field.clone()), fld_ty))
                        }
                        None => Err(cxt.err(ElabError::MissingField(field.clone()))),
                    }
                }
                // The record's type is still an unsolved meta. Mint a fresh
                // partial record type `Rec{field : ?fld ; ?row}` with
                // *unbound* metas (no spine) and unify the parameter-type
                // meta against it. Subsequent accesses accumulate fields by
                // re-running this branch — but only the FIRST access lands
                // here; the rest find the partial RecordTy and extend it.
                Val::Flex(_, _) => {
                    let fld_meta = cxt.fresh_meta_unbound();
                    let fld_ty = cxt.eval(&fld_meta).map_err(|e| cxt.err(e))?;
                    let row_meta = cxt.fresh_meta_unbound();
                    let row_ty = cxt.eval(&row_meta).map_err(|e| cxt.err(e))?;
                    let partial = Val::RecordTy(
                        vec![(field.clone(), fld_ty.clone())],
                        Some(Box::new(row_ty)),
                    );
                    cxt.unify_catch(&rty, &partial)?;
                    Ok((Term::RecordProj(Box::new(r), field.clone()), fld_ty))
                }
                _ => Err(cxt.err(ElabError::Internal(format!(
                    "record access on non-record type"
                )))),
            }
        }

        ExprKind::Variant(tag, payload) => {
            let (payload_tm, payload_ty) = match payload {
                Some(e) => infer(cxt, e)?,
                None => (Term::Lit(Lit::Unit), Val::LitTy(TypeLit::Unit)),
            };
            // Inferred variant value: a one-tag variant type with an open
            // tail so subsequent unifications can extend it. The tail meta
            // is *unbound* (no current-scope spine) so unifying it against
            // a closed variant type in an outer scope stays in the pattern
            // fragment.
            let row_meta = cxt.fresh_meta_unbound();
            let row_ty = cxt.eval(&row_meta).map_err(|e| cxt.err(e))?;
            let vty = Val::VariantTy(
                vec![(tag.clone(), payload_ty)],
                Some(Box::new(row_ty)),
            );
            Ok((Term::Variant(tag.clone(), Box::new(payload_tm)), vty))
        }

        ExprKind::VariantType(tags, tail) => {
            let mut ty_tags = Vec::new();
            for t in tags {
                let payload_ty = match &t.payload {
                    Some(e) => check(cxt, e, &Val::U)?,
                    None => Term::LitTy(TypeLit::Unit),
                };
                ty_tags.push((t.name.clone(), payload_ty));
            }
            let tail_tm = match tail {
                Some(t) => Some(Box::new(check(cxt, t, &Val::U)?)),
                None => None,
            };
            Ok((Term::VariantTy(ty_tags, tail_tm), Val::U))
        }

        ExprKind::Mu(name, body) => {
            // Mu binds one type variable (the self-reference)
            let inner_cxt = cxt.bind(name.clone(), Val::U);
            let body_tm = check(&inner_cxt, body, &Val::U)?;
            Ok((Term::Mu(Box::new(body_tm)), Val::U))
        }

        ExprKind::Unfold(inner) => {
            let (inner_tm, inner_ty) = infer(cxt, inner)?;
            let (inner_tm, inner_ty) = insert(cxt, inner_tm, inner_ty)?;
            let inner_ty = force(cxt.mcxt, &inner_ty);
            match inner_ty {
                Val::Mu(cl) => {
                    // F[Mu F / self]
                    let mu_val = Val::Mu(cl.clone());
                    let unfolded_ty = cl.apply(cxt.mcxt, mu_val).map_err(|e| cxt.err(e))?;
                    Ok((Term::Unfold(Box::new(inner_tm)), unfolded_ty))
                }
                _ => Err(cxt.err(ElabError::Internal(
                    format!("unfold: expected Mu type, got {:?}", cxt.quote(&inner_ty)),
                ))),
            }
        }

        ExprKind::Fold(_) => {
            Err(cxt.err(ElabError::CantInfer("fold (needs type annotation)".into())))
        }

        ExprKind::Hole(_name) => {
            let a = cxt.eval(&cxt.fresh_meta()).map_err(|e| cxt.err(e))?;
            let t = cxt.fresh_meta();
            Ok((t, a))
        }

        ExprKind::Pipe(lhs, rhs) => {
            // Desugar: lhs |> rhs => rhs lhs
            let app = Spanned::new(
                ExprKind::App(
                    rhs.clone(),
                    vec![crate::ast::AppArg {
                        icity: Icity::Explicit,
                        expr: lhs.clone(),
                    }],
                ),
                expr.span,
            );
            infer(cxt, &app)
        }

        ExprKind::BinOp(op, lhs, rhs) => infer_binop(cxt, *op, lhs, rhs),
        ExprKind::UnOp(op, inner) => infer_unop(cxt, *op, inner),

        ExprKind::Import(path) => {
            let resolver = cxt.resolver.ok_or_else(||
                cxt.err(ElabError::Internal("imports not available in this context".into()))
            )?;
            let (val, ty) = resolver(path).map_err(|e| cxt.err(ElabError::Internal(e)))?;
            // The resolver evaluates the imported module in its OWN
            // metacontext. The returned Vals reference metas in that
            // foreign mcxt — if we used them directly, lookups against
            // the host mcxt would return whatever value happened to be
            // at the same index (typically wrong, sometimes panicking).
            //
            // We freshen meta references during quote: every foreign
            // `Term::Meta(m)` / `Term::InsertedMeta(m, _)` is replaced
            // with a fresh host meta, then the freshened term is
            // re-evaluated in the host context.
            let mut remap = std::collections::HashMap::new();
            let raw_tm = quote(cxt.mcxt, cxt.lvl, &val);
            let tm = freshen_metas_in_term(cxt.mcxt, &raw_tm, &mut remap);
            let raw_ty_tm = quote(cxt.mcxt, cxt.lvl, &ty);
            let ty_tm_fresh = freshen_metas_in_term(cxt.mcxt, &raw_ty_tm, &mut remap);
            let ty_fresh = eval(cxt.mcxt, &cxt.env, &ty_tm_fresh).map_err(|e| cxt.err(e))?;
            Ok((tm, ty_fresh))
        }

        ExprKind::List(_) | ExprKind::ListType(_) | ExprKind::ArrayLit(_)
        | ExprKind::ArrayType(_) | ExprKind::Lazy(_) | ExprKind::Force(_) => {
            Err(cxt.err(ElabError::Internal(format!(
                "not yet implemented: {:?}",
                expr.node
            ))))
        }

        ExprKind::Lam(pats, _) if pats.is_empty() => {
            Err(cxt.err(ElabError::CantInfer("empty lambda".into())))
        }

        _ => Err(cxt.err(ElabError::CantInfer(format!("{:?}", expr.node)))),
    }
}

/// Type-check a binary operator. Equality/Neq accepts any matching pair of
/// primitive types; ordering defaults to Integer when the operand types are
/// unconstrained (typical case in untyped lambda bodies); logical ops accept
/// Bool. All ops synthesize Bool.
fn infer_binop(
    cxt: &Cxt,
    op: BinOp,
    lhs: &Spanned<ExprKind>,
    rhs: &Spanned<ExprKind>,
) -> Result<(Term, VTy), SpannedError> {
    let bool_ty = Val::LitTy(TypeLit::Bool);

    if op.is_logical() {
        let lhs_tm = check(cxt, lhs, &bool_ty)?;
        let rhs_tm = check(cxt, rhs, &bool_ty)?;
        return Ok((
            Term::BinOp(op, Box::new(lhs_tm), Box::new(rhs_tm)),
            bool_ty,
        ));
    }

    if op.is_ordering() {
        // Default ordering to Integer so untyped lambdas like
        // `\x => x > 0` resolve `x : Integer`. Users wanting Double or Char
        // can annotate the operand explicitly: `(x : Double) > 0.0`.
        let int_ty = Val::LitTy(TypeLit::Integer);
        let lhs_tm = check(cxt, lhs, &int_ty)?;
        let rhs_tm = check(cxt, rhs, &int_ty)?;
        return Ok((
            Term::BinOp(op, Box::new(lhs_tm), Box::new(rhs_tm)),
            bool_ty,
        ));
    }

    // Equality: infer lhs, then check rhs against the same type. Works for
    // any equality-comparable primitive once lhs's type is known.
    let (lhs_tm, lhs_ty) = infer(cxt, lhs)?;
    let rhs_tm = check(cxt, rhs, &lhs_ty)?;
    Ok((
        Term::BinOp(op, Box::new(lhs_tm), Box::new(rhs_tm)),
        bool_ty,
    ))
}

fn infer_unop(
    cxt: &Cxt,
    op: UnOp,
    inner: &Spanned<ExprKind>,
) -> Result<(Term, VTy), SpannedError> {
    let bool_ty = Val::LitTy(TypeLit::Bool);
    match op {
        UnOp::Not => {
            let inner_tm = check(cxt, inner, &bool_ty)?;
            Ok((Term::UnOp(op, Box::new(inner_tm)), bool_ty))
        }
    }
}

fn infer_lit(lit: &Lit) -> (Term, VTy) {
    let (tm, ty) = match lit {
        Lit::Integer(n) => (Lit::Integer(*n), TypeLit::Integer),
        Lit::Double(n) => (Lit::Double(*n), TypeLit::Double),
        Lit::String(s) => (Lit::String(s.clone()), TypeLit::String),
        Lit::Char(c) => (Lit::Char(*c), TypeLit::Char),
        Lit::Bool(b) => (Lit::Bool(*b), TypeLit::Bool),
        Lit::Unit => (Lit::Unit, TypeLit::Unit),
    };
    (Term::Lit(tm), Val::LitTy(ty))
}

fn check_pi(
    cxt: &Cxt,
    params: &[crate::ast::PiParam],
    ret: &crate::ast::Expr,
) -> Result<(Term, VTy), SpannedError> {
    if params.is_empty() {
        let ret_tm = check(cxt, ret, &Val::U)?;
        return Ok((ret_tm, Val::U));
    }

    let param = &params[0];
    let a_tm = check(cxt, &param.ty, &Val::U)?;
    let a_val = cxt.eval(&a_tm).map_err(|e| cxt.err(e))?;
    let name = param.name.clone().unwrap_or_else(|| "_".into());

    let inner_cxt = cxt.bind(name.clone(), a_val);

    let (b_tm, _) = if params.len() > 1 {
        check_pi(&inner_cxt, &params[1..], ret)?
    } else {
        let b = check(&inner_cxt, ret, &Val::U)?;
        (b, Val::U)
    };

    Ok((
        Term::Pi(name, param.icity, Box::new(a_tm), Box::new(b_tm)),
        Val::U,
    ))
}

// ---------------------------------------------------------------------------
// Auto-implicit quantification
// ---------------------------------------------------------------------------

/// Collect free lowercase variables from a type expression that are not in scope.
/// Returns them in left-to-right first-occurrence order.
fn collect_free_type_vars(expr: &crate::ast::Expr, cxt: &Cxt) -> Vec<String> {
    use std::collections::HashSet;
    let mut free = Vec::new();
    let mut seen = HashSet::new();
    let mut local = HashSet::new();
    collect_free_vars_inner(&expr.node, cxt, &mut local, &mut seen, &mut free);
    free
}

fn collect_free_vars_inner(
    expr: &crate::ast::ExprKind,
    cxt: &Cxt,
    local: &mut std::collections::HashSet<String>,
    seen: &mut std::collections::HashSet<String>,
    free: &mut Vec<String>,
) {
    use crate::ast::ExprKind;
    match expr {
        ExprKind::Var(name) => {
            if name.chars().next().is_some_and(|c| c.is_lowercase())
                && !local.contains(name)
                && !cxt.types.iter().any(|(n, _, _)| n == name)
                && seen.insert(name.clone())
            {
                free.push(name.clone());
            }
        }
        ExprKind::App(f, args) => {
            collect_free_vars_inner(&f.node, cxt, local, seen, free);
            for arg in args {
                collect_free_vars_inner(&arg.expr.node, cxt, local, seen, free);
            }
        }
        ExprKind::Lam(pats, body) => {
            let mut local = local.clone();
            for pat in pats {
                collect_pat_bindings(&pat.node, &mut local);
            }
            collect_free_vars_inner(&body.node, cxt, &mut local, seen, free);
        }
        ExprKind::Pi(params, body) => {
            let mut local = local.clone();
            for param in params {
                collect_free_vars_inner(&param.ty.node, cxt, &mut local, seen, free);
                if let Some(name) = &param.name {
                    local.insert(name.clone());
                }
            }
            collect_free_vars_inner(&body.node, cxt, &mut local, seen, free);
        }
        ExprKind::Arrow(a, b) => {
            collect_free_vars_inner(&a.node, cxt, local, seen, free);
            collect_free_vars_inner(&b.node, cxt, local, seen, free);
        }
        ExprKind::Let(bindings, body) => {
            let mut local = local.clone();
            for b in bindings {
                if let Some(ty) = &b.ty {
                    collect_free_vars_inner(&ty.node, cxt, &mut local, seen, free);
                }
                collect_free_vars_inner(&b.value.node, cxt, &mut local, seen, free);
                local.insert(b.name.clone());
            }
            collect_free_vars_inner(&body.node, cxt, &mut local, seen, free);
        }
        ExprKind::Ann(e, ty) => {
            collect_free_vars_inner(&e.node, cxt, local, seen, free);
            collect_free_vars_inner(&ty.node, cxt, local, seen, free);
        }
        ExprKind::If(c, t, f) => {
            collect_free_vars_inner(&c.node, cxt, local, seen, free);
            collect_free_vars_inner(&t.node, cxt, local, seen, free);
            collect_free_vars_inner(&f.node, cxt, local, seen, free);
        }
        ExprKind::Case(scrut, branches) => {
            collect_free_vars_inner(&scrut.node, cxt, local, seen, free);
            for branch in branches {
                let mut local = local.clone();
                collect_pat_bindings(&branch.pattern.node, &mut local);
                collect_free_vars_inner(&branch.body.node, cxt, &mut local, seen, free);
            }
        }
        ExprKind::Record(fields) => {
            for field in fields {
                collect_free_vars_inner(&field.value.node, cxt, local, seen, free);
            }
        }
        ExprKind::RecordUpdate(base, fields) => {
            collect_free_vars_inner(&base.node, cxt, local, seen, free);
            for field in fields {
                collect_free_vars_inner(&field.value.node, cxt, local, seen, free);
            }
        }
        ExprKind::RecordType(fields, tail) => {
            for field in fields {
                collect_free_vars_inner(&field.ty.node, cxt, local, seen, free);
            }
            if let Some(t) = tail {
                collect_free_vars_inner(&t.node, cxt, local, seen, free);
            }
        }
        ExprKind::RecordAccess(e, _) => {
            collect_free_vars_inner(&e.node, cxt, local, seen, free);
        }
        ExprKind::Variant(_, payload) => {
            if let Some(p) = payload {
                collect_free_vars_inner(&p.node, cxt, local, seen, free);
            }
        }
        ExprKind::VariantType(tags, tail) => {
            for tag in tags {
                if let Some(p) = &tag.payload {
                    collect_free_vars_inner(&p.node, cxt, local, seen, free);
                }
            }
            if let Some(t) = tail {
                collect_free_vars_inner(&t.node, cxt, local, seen, free);
            }
        }
        ExprKind::Mu(name, body) => {
            let mut local = local.clone();
            local.insert(name.clone());
            collect_free_vars_inner(&body.node, cxt, &mut local, seen, free);
        }
        ExprKind::Fold(e) | ExprKind::Unfold(e) | ExprKind::Lazy(e) | ExprKind::Force(e) => {
            collect_free_vars_inner(&e.node, cxt, local, seen, free);
        }
        ExprKind::List(es) | ExprKind::ArrayLit(es) => {
            for e in es {
                collect_free_vars_inner(&e.node, cxt, local, seen, free);
            }
        }
        ExprKind::ListType(e) | ExprKind::ArrayType(e) => {
            collect_free_vars_inner(&e.node, cxt, local, seen, free);
        }
        ExprKind::Pipe(lhs, rhs) => {
            collect_free_vars_inner(&lhs.node, cxt, local, seen, free);
            collect_free_vars_inner(&rhs.node, cxt, local, seen, free);
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            collect_free_vars_inner(&lhs.node, cxt, local, seen, free);
            collect_free_vars_inner(&rhs.node, cxt, local, seen, free);
        }
        ExprKind::UnOp(_, inner) => {
            collect_free_vars_inner(&inner.node, cxt, local, seen, free);
        }
        ExprKind::Lit(_) | ExprKind::TypeLit(_) | ExprKind::Hole(_) | ExprKind::Import(_)
        | ExprKind::Undefined => {}
    }
}

fn collect_pat_bindings(pat: &crate::ast::PatternKind, bound: &mut std::collections::HashSet<String>) {
    use crate::ast::PatternKind;
    match pat {
        PatternKind::Var(name) => { bound.insert(name.clone()); }
        PatternKind::Variant(_, Some(inner)) => collect_pat_bindings(&inner.node, bound),
        PatternKind::Record(fields) => {
            for field in fields {
                match field {
                    crate::ast::RecordPatternField::Pun(s) => { bound.insert(s.node.clone()); }
                    crate::ast::RecordPatternField::Match(_, p) => collect_pat_bindings(&p.node, bound),
                }
            }
        }
        PatternKind::Ann(inner, _) => collect_pat_bindings(&inner.node, bound),
        PatternKind::Lit(_) | PatternKind::Wildcard | PatternKind::Variant(_, None) => {}
    }
}

/// Wrap a type expression with implicit Pi bindings for the given free type variables.
fn auto_quantify(expr: &crate::ast::Expr, vars: &[String]) -> crate::ast::Expr {
    use crate::ast::{ExprKind, PiParam, Spanned, TypeLit};
    let span = expr.span;
    let mut result = expr.clone();
    for var in vars.iter().rev() {
        result = Box::new(Spanned::new(
            ExprKind::Pi(
                vec![PiParam {
                    icity: Icity::Implicit,
                    name: Some(var.clone()),
                    ty: Box::new(Spanned::new(ExprKind::TypeLit(TypeLit::Type), span)),
                }],
                result,
            ),
            span,
        ));
    }
    result
}

fn check_let(
    cxt: &Cxt,
    bindings: &[crate::ast::LetBinding],
    body: &crate::ast::Expr,
    expected: &Val,
) -> Result<Term, SpannedError> {
    let (tm, _) = elab_let(cxt, bindings, body, Some(expected))?;
    Ok(tm)
}

fn infer_let(
    cxt: &Cxt,
    bindings: &[crate::ast::LetBinding],
    body: &crate::ast::Expr,
) -> Result<(Term, VTy), SpannedError> {
    elab_let(cxt, bindings, body, None)
}

fn elab_let(
    cxt: &Cxt,
    bindings: &[crate::ast::LetBinding],
    body: &crate::ast::Expr,
    expected: Option<&Val>,
) -> Result<(Term, VTy), SpannedError> {
    if bindings.is_empty() {
        return match expected {
            Some(exp) => {
                let tm = check(cxt, body, exp)?;
                Ok((tm, exp.clone()))
            }
            None => infer(cxt, body),
        };
    }

    let binding = &bindings[0];
    let rest = &bindings[1..];

    // Desugar function params: `f x y = body` → `f = \x => \y => body`
    let value_expr = if binding.params.is_empty() {
        binding.value.clone()
    } else {
        let mut result = binding.value.clone();
        for pat in binding.params.iter().rev() {
            result = Box::new(Spanned::new(
                ExprKind::Lam(vec![pat.clone()], result),
                binding.span,
            ));
        }
        result
    };

    if let Some(ty_expr) = &binding.ty {
        // Typed binding — auto-quantify free lowercase type variables
        let free_vars = collect_free_type_vars(ty_expr, cxt);
        let ty_expr = if free_vars.is_empty() {
            ty_expr.clone()
        } else {
            auto_quantify(ty_expr, &free_vars)
        };
        let ty_tm = check(cxt, &ty_expr, &Val::U)?;
        let ty_val = cxt.eval(&ty_tm).map_err(|e| cxt.err(e))?;
        let val_tm = check(cxt, &value_expr, &ty_val)?;
        let val_v = cxt.eval(&val_tm).map_err(|e| cxt.err(e))?;

        let inner_cxt = cxt.define(binding.name.clone(), val_v, ty_val);
        let (body_tm, body_ty) = elab_let(&inner_cxt, rest, body, expected)?;

        Ok((
            Term::Let(
                binding.name.clone(),
                Box::new(ty_tm),
                Box::new(val_tm),
                Box::new(body_tm),
            ),
            body_ty,
        ))
    } else {
        // Untyped binding: infer
        let (val_tm, ty_val) = infer(cxt, &value_expr)?;
        let (val_tm, ty_val) = insert(cxt, val_tm, ty_val)?;
        let ty_tm = cxt.quote(&ty_val);
        let val_v = cxt.eval(&val_tm).map_err(|e| cxt.err(e))?;

        let inner_cxt = cxt.define(binding.name.clone(), val_v, ty_val);
        let (body_tm, body_ty) = elab_let(&inner_cxt, rest, body, expected)?;

        Ok((
            Term::Let(
                binding.name.clone(),
                Box::new(ty_tm),
                Box::new(val_tm),
                Box::new(body_tm),
            ),
            body_ty,
        ))
    }
}

fn infer_case(
    cxt: &Cxt,
    scrut: &crate::ast::Expr,
    branches: &[crate::ast::CaseBranch],
) -> Result<(Term, VTy), SpannedError> {
    if branches.is_empty() {
        return Err(cxt.err(ElabError::CantInfer("empty case".into())));
    }

    let (scrut_tm, scrut_ty) = infer(cxt, scrut)?;
    let (scrut_tm, scrut_ty) = insert(cxt, scrut_tm, scrut_ty)?;

    // Elaborate first branch to get the result type
    let mut core_branches = Vec::new();
    let mut result_ty: Option<VTy> = None;

    for branch in branches {
        let (pat, binders) = elab_pattern(cxt, &branch.pattern, &scrut_ty)?;

        let mut branch_cxt = cxt.clone_with_bindings();
        for (name, ty) in &binders {
            branch_cxt = branch_cxt.bind(name.clone(), ty.clone());
        }

        match &result_ty {
            None => {
                let (body_tm, body_ty) = infer(&branch_cxt, &branch.body)?;
                result_ty = Some(body_ty);
                core_branches.push((pat, body_tm));
            }
            Some(rty) => {
                let body_tm = check(&branch_cxt, &branch.body, rty)?;
                core_branches.push((pat, body_tm));
            }
        }
    }

    Ok((
        Term::Case(Box::new(scrut_tm), core_branches),
        result_ty.unwrap(),
    ))
}

fn elab_pattern(
    cxt: &Cxt,
    pat: &crate::ast::Pattern,
    expected_ty: &VTy,
) -> Result<(Pat, Vec<(Name, VTy)>), SpannedError> {
    match &pat.node {
        crate::ast::PatternKind::Var(name) => {
            Ok((Pat::Var(name.clone()), vec![(name.clone(), expected_ty.clone())]))
        }
        crate::ast::PatternKind::Wildcard => Ok((Pat::Wildcard, vec![])),
        crate::ast::PatternKind::Lit(lit) => {
            Ok((Pat::Lit(lit.clone()), vec![]))
        }
        crate::ast::PatternKind::Variant(tag, inner) => {
            // Look up the variant tag type from expected_ty
            let exp = force(cxt.mcxt, expected_ty);
            let payload_ty = match &exp {
                Val::VariantTy(tags, _) => {
                    tags.iter()
                        .find(|(n, _)| n == tag)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Val::LitTy(TypeLit::Unit))
                }
                _ => Val::LitTy(TypeLit::Unit),
            };

            let (inner_pat, inner_binders) = match inner {
                Some(p) => elab_pattern(cxt, p, &payload_ty)?,
                None => (Pat::Wildcard, vec![]),
            };

            Ok((Pat::Variant(tag.clone(), Box::new(inner_pat)), inner_binders))
        }
        crate::ast::PatternKind::Record(fields) => {
            let exp = force(cxt.mcxt, expected_ty);
            let type_fields = match &exp {
                Val::RecordTy(fs, _) => fs.clone(),
                _ => Vec::new(),
            };

            let mut binders = Vec::new();
            let mut core_fields = Vec::new();

            for field in fields {
                match field {
                    crate::ast::RecordPatternField::Pun(spanned_name) => {
                        let name = &spanned_name.node;
                        let fty = type_fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Val::U);
                        core_fields.push(RecordPat::Pun(name.clone()));
                        binders.push((name.clone(), fty));
                    }
                    crate::ast::RecordPatternField::Match(name, inner_pat) => {
                        let fty = type_fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Val::U);
                        let (cpat, inner_binders) = elab_pattern(cxt, inner_pat, &fty)?;
                        core_fields.push(RecordPat::Match(name.clone(), cpat));
                        binders.extend(inner_binders);
                    }
                }
            }

            Ok((Pat::Record(core_fields), binders))
        }
        crate::ast::PatternKind::Ann(_, _) => {
            Err(cxt.err(ElabError::Internal("pattern annotations not yet supported".into())))
        }
    }
}

// Helper for case elaboration — we need to extend cxt with bindings
impl<'a> Cxt<'a> {
    fn clone_with_bindings(&self) -> Cxt<'a> {
        Cxt {
            env: self.env.clone(),
            lvl: self.lvl,
            types: self.types.clone(),
            bds: self.bds.clone(),
            mcxt: self.mcxt,
            span: self.span,
            hover_map: self.hover_map.clone(),
            resolver: self.resolver,
        }
    }
}

// ---------------------------------------------------------------------------
// Pretty printing (minimal, for error messages and eval output)
// ---------------------------------------------------------------------------

pub struct TermPrinter<'a>(pub &'a Term);

impl<'a> fmt::Display for TermPrinter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        print_term(f, self.0, 0)
    }
}

fn print_term(f: &mut fmt::Formatter<'_>, tm: &Term, prec: usize) -> fmt::Result {
    match tm {
        Term::Var(Ix(x)) => write!(f, "#{x}"),
        Term::U => write!(f, "Type"),
        Term::Lit(lit) => write!(f, "{lit}"),
        Term::LitTy(tl) => write!(f, "{tl}"),

        Term::Lam(x, icit, body) => {
            if prec > 0 {
                write!(f, "(")?;
            }
            match icit {
                Icity::Explicit => write!(f, "\\{x} => ")?,
                Icity::Implicit => write!(f, "\\{{{x}}} => ")?,
            }
            print_term(f, body, 0)?;
            if prec > 0 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::App(func, arg, icit) => {
            if prec > 1 {
                write!(f, "(")?;
            }
            print_term(f, func, 1)?;
            match icit {
                Icity::Explicit => {
                    write!(f, " ")?;
                    print_term(f, arg, 2)?;
                }
                Icity::Implicit => {
                    write!(f, " ?{{")?;
                    print_term(f, arg, 0)?;
                    write!(f, "}}")?;
                }
            }
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::Pi(x, icit, a, b) => {
            if prec > 0 {
                write!(f, "(")?;
            }
            if x == "_" {
                print_term(f, a, 2)?;
                write!(f, " -> ")?;
            } else {
                match icit {
                    Icity::Explicit => {
                        write!(f, "({x} : ")?;
                        print_term(f, a, 0)?;
                        write!(f, ") -> ")?;
                    }
                    Icity::Implicit => {
                        write!(f, "{{{x} : ")?;
                        print_term(f, a, 0)?;
                        write!(f, "}} -> ")?;
                    }
                }
            }
            print_term(f, b, 0)?;
            if prec > 0 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::Let(x, ty, val, body) => {
            if prec > 0 {
                write!(f, "(")?;
            }
            write!(f, "let {x} : ")?;
            print_term(f, ty, 0)?;
            write!(f, " = ")?;
            print_term(f, val, 0)?;
            write!(f, " in ")?;
            print_term(f, body, 0)?;
            if prec > 0 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::Meta(MetaVar(m)) => write!(f, "?{m}"),
        Term::InsertedMeta(MetaVar(m), _) => write!(f, "?{m}"),

        Term::Record(fields) => {
            write!(f, "{{")?;
            for (i, (name, val)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{name} = ")?;
                print_term(f, val, 0)?;
            }
            write!(f, "}}")
        }

        Term::RecordTy(fields, tail) => {
            write!(f, "Rec {{")?;
            for (i, (name, ty)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{name} : ")?;
                print_term(f, ty, 0)?;
            }
            if let Some(t) = tail {
                if !fields.is_empty() {
                    write!(f, " ")?;
                }
                write!(f, "; ")?;
                print_term(f, t, 0)?;
            }
            write!(f, "}}")
        }

        Term::RecordProj(t, field) => {
            print_term(f, t, 2)?;
            write!(f, ".{field}")
        }

        Term::Variant(tag, payload) => {
            write!(f, "'{tag}")?;
            match payload.as_ref() {
                Term::Lit(Lit::Unit) => Ok(()),
                _ => {
                    write!(f, " ")?;
                    print_term(f, payload, 2)
                }
            }
        }

        Term::VariantTy(tags, tail) => {
            write!(f, "< ")?;
            for (i, (name, ty)) in tags.iter().enumerate() {
                if i > 0 {
                    write!(f, " | ")?;
                }
                write!(f, "'{name}")?;
                match ty {
                    Term::LitTy(TypeLit::Unit) => {}
                    _ => {
                        write!(f, " ")?;
                        print_term(f, ty, 2)?;
                    }
                }
            }
            if let Some(t) = tail {
                write!(f, " ; ")?;
                print_term(f, t, 0)?;
            }
            write!(f, " >")
        }

        Term::Case(scrut, branches) => {
            write!(f, "case ")?;
            print_term(f, scrut, 0)?;
            write!(f, " of")?;
            for (pat, body) in branches {
                write!(f, " | ")?;
                print_pat(f, pat)?;
                write!(f, " => ")?;
                print_term(f, body, 0)?;
            }
            Ok(())
        }

        Term::If(c, t, e) => {
            write!(f, "if ")?;
            print_term(f, c, 0)?;
            write!(f, " then ")?;
            print_term(f, t, 0)?;
            write!(f, " else ")?;
            print_term(f, e, 0)
        }

        Term::Fix(name, body) => {
            write!(f, "fix {name} ")?;
            print_term(f, body, 0)
        }

        Term::Mu(body) => {
            if prec > 0 {
                write!(f, "(")?;
            }
            write!(f, "Mu _. ")?;
            print_term(f, body, 0)?;
            if prec > 0 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::Fold(t) => {
            if prec > 1 {
                write!(f, "(")?;
            }
            write!(f, "fold ")?;
            print_term(f, t, 2)?;
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::Unfold(t) => {
            if prec > 1 {
                write!(f, "(")?;
            }
            write!(f, "unfold ")?;
            print_term(f, t, 2)?;
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::BinOp(op, a, b) => {
            if prec > 0 {
                write!(f, "(")?;
            }
            print_term(f, a, 1)?;
            write!(f, " {} ", op.symbol())?;
            print_term(f, b, 1)?;
            if prec > 0 {
                write!(f, ")")?;
            }
            Ok(())
        }

        Term::UnOp(op, a) => {
            if prec > 1 {
                write!(f, "(")?;
            }
            write!(f, "{}", op.symbol())?;
            print_term(f, a, 2)?;
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }
    }
}

fn print_pat(f: &mut fmt::Formatter<'_>, pat: &Pat) -> fmt::Result {
    match pat {
        Pat::Var(n) => write!(f, "{n}"),
        Pat::Wildcard => write!(f, "_"),
        Pat::Lit(l) => write!(f, "{l}"),
        Pat::Variant(tag, inner) => {
            write!(f, "'{tag}")?;
            match inner.as_ref() {
                Pat::Wildcard => Ok(()),
                _ => {
                    write!(f, " ")?;
                    print_pat(f, inner)
                }
            }
        }
        Pat::Record(fields) => {
            write!(f, "{{")?;
            for (i, rp) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match rp {
                    RecordPat::Pun(n) => write!(f, "{n}")?,
                    RecordPat::Match(n, p) => {
                        write!(f, "{n} = ")?;
                        print_pat(f, p)?;
                    }
                }
            }
            write!(f, "}}")
        }
    }
}
