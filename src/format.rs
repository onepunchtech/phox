use crate::ast::*;
use crate::parser::{parse_with_comments, ParseError};

pub fn format(source: &str) -> Result<String, Vec<ParseError>> {
    let (expr, comments) = parse_with_comments(source)?;
    let mut fmt = Formatter::new(comments);
    fmt.fmt_expr(&expr);
    fmt.trailing_newline();
    Ok(fmt.finish())
}

struct Formatter {
    out: String,
    indent: usize,
    comments: Vec<(Span, String)>,
    comment_idx: usize,
}

impl Formatter {
    fn new(comments: Vec<(Span, String)>) -> Self {
        Self {
            out: String::new(),
            indent: 0,
            comments,
            comment_idx: 0,
        }
    }

    fn finish(self) -> String {
        self.out
    }

    fn trailing_newline(&mut self) {
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push(' ');
        }
    }

    fn space(&mut self) {
        self.out.push(' ');
    }

    fn emit_comments_before(&mut self, offset: usize) {
        while self.comment_idx < self.comments.len() {
            let (span, _) = &self.comments[self.comment_idx];
            if span.0 < offset {
                let text = self.comments[self.comment_idx].1.clone();
                self.write(&text);
                self.newline();
                self.comment_idx += 1;
            } else {
                break;
            }
        }
    }

    fn fmt_expr(&mut self, expr: &Spanned<ExprKind>) {
        self.emit_comments_before(expr.span.0);
        match &expr.node {
            ExprKind::Var(name) => self.write(name),
            ExprKind::Lit(lit) => self.fmt_lit(lit),
            ExprKind::TypeLit(tl) => self.write(&tl.to_string()),
            ExprKind::Undefined => self.write("undefined"),

            ExprKind::Lam(pats, body) => {
                self.write("\\");
                for (i, pat) in pats.iter().enumerate() {
                    if i > 0 {
                        self.space();
                    }
                    self.fmt_pattern(pat);
                }
                self.write(" => ");
                self.fmt_expr(body);
            }

            ExprKind::App(func, args) => {
                self.fmt_expr(func);
                for arg in args {
                    self.space();
                    match arg.icity {
                        Icity::Explicit => self.fmt_expr_atom(&arg.expr),
                        Icity::Implicit => {
                            self.write("?{");
                            self.fmt_expr(&arg.expr);
                            self.write("}");
                        }
                    }
                }
            }

            ExprKind::Let(bindings, body) => self.fmt_let(bindings, body),
            ExprKind::If(cond, then_e, else_e) => self.fmt_if(cond, then_e, else_e),
            ExprKind::Case(scrut, branches) => self.fmt_case(scrut, branches),
            ExprKind::Ann(expr, ty) => {
                self.write("(");
                self.fmt_expr(expr);
                self.write(" : ");
                self.fmt_expr(ty);
                self.write(")");
            }

            ExprKind::Record(fields) => self.fmt_record(fields),
            ExprKind::RecordUpdate(base, fields) => {
                self.write("{...");
                self.fmt_expr(base);
                for f in fields {
                    self.write(", ");
                    self.write(&f.name);
                    self.write(" = ");
                    self.fmt_expr(&f.value);
                }
                self.write("}");
            }
            ExprKind::RecordType(fields, tail) => self.fmt_record_type(fields, tail),
            ExprKind::RecordAccess(expr, field) => {
                self.fmt_expr(expr);
                self.write(".");
                self.write(field);
            }

            ExprKind::Variant(tag, payload) => {
                self.write("'");
                self.write(tag);
                if let Some(p) = payload {
                    self.space();
                    self.fmt_expr_atom(p);
                }
            }

            ExprKind::VariantType(tags, tail) => self.fmt_variant_type(tags, tail),

            ExprKind::Pi(params, ret) => self.fmt_pi(params, ret),
            ExprKind::Arrow(a, b) => {
                // LHS of arrow needs parens if it's itself an arrow (right-assoc)
                match &a.node {
                    ExprKind::Arrow(..) | ExprKind::Pi(..) => {
                        self.write("(");
                        self.fmt_expr(a);
                        self.write(")");
                    }
                    _ => self.fmt_expr_app(a),
                }
                self.write(" -> ");
                self.fmt_expr(b);
            }

            ExprKind::List(elems) => {
                self.write("[");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(e);
                }
                self.write("]");
            }

            ExprKind::Hole(name) => {
                self.write("?");
                self.write(name);
            }

            ExprKind::Import(path) => {
                self.write("import \"");
                self.write(path);
                self.write("\"");
            }

            ExprKind::Mu(name, body) => {
                self.write("Mu ");
                self.write(name);
                self.write(". ");
                self.fmt_expr(body);
            }

            ExprKind::Fold(inner) => {
                self.write("fold ");
                self.fmt_expr_atom(inner);
            }

            ExprKind::Unfold(inner) => {
                self.write("unfold ");
                self.fmt_expr_atom(inner);
            }

            // Not yet implemented constructs — print as-is
            ExprKind::ListType(e) => {
                self.write("List ");
                self.fmt_expr_atom(e);
            }
            ExprKind::ArrayLit(elems) => {
                self.write("Array(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(e);
                }
                self.write(")");
            }
            ExprKind::ArrayType(e) => {
                self.write("Array ");
                self.fmt_expr_atom(e);
            }
            ExprKind::Lazy(e) => {
                self.write("Lazy ");
                self.fmt_expr_atom(e);
            }
            ExprKind::Force(e) => {
                self.write("force ");
                self.fmt_expr_atom(e);
            }
            ExprKind::Pipe(lhs, rhs) => {
                self.fmt_expr(lhs);
                self.write(" |> ");
                self.fmt_expr(rhs);
            }
            ExprKind::BinOp(op, lhs, rhs) => {
                self.fmt_expr_atom(lhs);
                self.write(" ");
                self.write(op.symbol());
                self.write(" ");
                self.fmt_expr_atom(rhs);
            }
            ExprKind::UnOp(op, inner) => {
                self.write(op.symbol());
                self.fmt_expr_atom(inner);
            }
        }
    }

    /// Format an expression in atom position (wrap in parens if needed)
    fn fmt_expr_atom(&mut self, expr: &Spanned<ExprKind>) {
        if self.needs_parens(&expr.node) {
            self.write("(");
            self.fmt_expr(expr);
            self.write(")");
        } else {
            self.fmt_expr(expr);
        }
    }

    /// Format an expression in application position (wrap let/case/if/lambda)
    fn fmt_expr_app(&mut self, expr: &Spanned<ExprKind>) {
        match &expr.node {
            ExprKind::Let(..) | ExprKind::Case(..) | ExprKind::If(..)
            | ExprKind::Lam(..) | ExprKind::Mu(..) => {
                self.write("(");
                self.fmt_expr(expr);
                self.write(")");
            }
            _ => self.fmt_expr(expr),
        }
    }

    fn needs_parens(&self, node: &ExprKind) -> bool {
        matches!(
            node,
            ExprKind::App(..)
                | ExprKind::Let(..)
                | ExprKind::Case(..)
                | ExprKind::If(..)
                | ExprKind::Lam(..)
                | ExprKind::Arrow(..)
                | ExprKind::Pi(..)
                | ExprKind::Mu(..)
                | ExprKind::Fold(..)
                | ExprKind::Unfold(..)
                | ExprKind::BinOp(..)
                | ExprKind::UnOp(..)
                | ExprKind::Pipe(..)
        )
    }

    fn fmt_lit(&mut self, lit: &Lit) {
        self.write(&lit.to_string());
    }

    fn fmt_let(&mut self, bindings: &[LetBinding], body: &Expr) {
        self.write("let");
        self.indent += 2;
        for (i, binding) in bindings.iter().enumerate() {
            if i > 0 && binding.ty.is_some() {
                // Blank line between annotated bindings
                self.newline();
            }
            self.newline();
            self.emit_comments_before(binding.span.0);
            if let Some(ty_expr) = &binding.ty {
                if binding.params.is_empty() {
                    // Inline form: name : type = value
                    self.write(&binding.name);
                    self.write(" : ");
                    self.fmt_expr(ty_expr);
                    self.write(" = ");
                    self.fmt_expr(&binding.value);
                } else {
                    // Two-line form: name : type\n name params = value
                    self.write(&binding.name);
                    self.write(" : ");
                    self.fmt_expr(ty_expr);
                    self.newline();
                    self.write(&binding.name);
                    for param in &binding.params {
                        self.space();
                        self.fmt_pattern(param);
                    }
                    self.write(" = ");
                    self.fmt_expr(&binding.value);
                }
            } else {
                self.write(&binding.name);
                for param in &binding.params {
                    self.space();
                    self.fmt_pattern(param);
                }
                self.write(" = ");
                self.fmt_expr(&binding.value);
            }
        }
        self.indent -= 2;
        self.newline();
        self.write("in ");
        self.fmt_expr(body);
    }

    fn fmt_if(&mut self, cond: &Expr, then_e: &Expr, else_e: &Expr) {
        self.write("if ");
        self.fmt_expr(cond);
        self.write(" then ");
        self.fmt_expr(then_e);
        self.write(" else ");
        self.fmt_expr(else_e);
    }

    fn fmt_case(&mut self, scrut: &Expr, branches: &[CaseBranch]) {
        self.write("case ");
        self.fmt_expr(scrut);
        self.write(" of");
        self.indent += 2;
        for branch in branches {
            self.newline();
            self.emit_comments_before(branch.pattern.span.0);
            self.fmt_pattern(&branch.pattern);
            self.write(" => ");
            self.fmt_expr(&branch.body);
        }
        self.indent -= 2;
    }

    fn fmt_pi(&mut self, params: &[PiParam], ret: &Expr) {
        for param in params {
            match param.icity {
                Icity::Explicit => {
                    self.write("(");
                    if let Some(name) = &param.name {
                        self.write(name);
                        self.write(" : ");
                    }
                    self.fmt_expr(&param.ty);
                    self.write(") -> ");
                }
                Icity::Implicit => {
                    self.write("?{");
                    if let Some(name) = &param.name {
                        self.write(name);
                        self.write(" : ");
                    }
                    self.fmt_expr(&param.ty);
                    self.write("} -> ");
                }
            }
        }
        self.fmt_expr(ret);
    }

    fn is_complex(expr: &ExprKind) -> bool {
        matches!(
            expr,
            ExprKind::Record(_)
                | ExprKind::RecordUpdate(_, _)
                | ExprKind::RecordType(_, _)
                | ExprKind::VariantType(_, _)
                | ExprKind::Let(_, _)
                | ExprKind::Case(_, _)
                | ExprKind::If(_, _, _)
                | ExprKind::Mu(_, _)
        )
    }

    fn fmt_record(&mut self, fields: &[RecordField]) {
        if fields.is_empty() {
            self.write("{}");
            return;
        }
        let multiline = fields.len() > 1
            && fields.iter().any(|f| Self::is_complex(&f.value.node));
        if multiline {
            self.write("{");
            self.indent += 2;
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.newline();
                self.write(&f.name);
                self.write(" = ");
                self.fmt_expr(&f.value);
            }
            self.indent -= 2;
            self.newline();
            self.write("}");
        } else {
            self.write("{");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&f.name);
                self.write(" = ");
                self.fmt_expr(&f.value);
            }
            self.write("}");
        }
    }

    fn fmt_record_type(&mut self, fields: &[RecordTypeField], tail: &Option<Expr>) {
        let multiline = fields.len() > 3
            || fields.iter().any(|f| Self::is_complex(&f.ty.node));
        if multiline {
            self.write("Rec {");
            self.indent += 2;
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.newline();
                self.write(&f.name);
                self.write(" : ");
                self.fmt_expr(&f.ty);
            }
            if let Some(t) = tail {
                self.write("; ");
                self.fmt_expr(t);
            }
            self.indent -= 2;
            self.newline();
            self.write("}");
        } else {
            self.write("Rec {");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&f.name);
                self.write(" : ");
                self.fmt_expr(&f.ty);
            }
            if let Some(t) = tail {
                self.write("; ");
                self.fmt_expr(t);
            }
            self.write("}");
        }
    }

    fn fmt_variant_type(&mut self, tags: &[VariantTag], tail: &Option<Expr>) {
        self.write("< ");
        for (i, tag) in tags.iter().enumerate() {
            if i > 0 {
                self.write(" | ");
            }
            self.write("'");
            self.write(&tag.name);
            if let Some(payload) = &tag.payload {
                self.space();
                self.fmt_expr_atom(payload);
            }
        }
        if let Some(t) = tail {
            self.write("; ");
            self.fmt_expr(t);
        }
        self.write(" >");
    }

    fn fmt_pattern(&mut self, pat: &Pattern) {
        match &pat.node {
            PatternKind::Var(name) => self.write(name),
            PatternKind::Wildcard => self.write("_"),
            PatternKind::Lit(lit) => self.fmt_lit(lit),
            PatternKind::Variant(tag, inner) => {
                self.write("'");
                self.write(tag);
                if let Some(p) = inner {
                    self.space();
                    self.fmt_pattern(p);
                }
            }
            PatternKind::Record(fields) => {
                self.write("{");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    match f {
                        RecordPatternField::Pun(spanned) => self.write(&spanned.node),
                        RecordPatternField::Match(name, pat) => {
                            self.write(name);
                            self.write(" = ");
                            self.fmt_pattern(pat);
                        }
                    }
                }
                self.write("}");
            }
            PatternKind::Ann(pat, ty) => {
                self.write("(");
                self.fmt_pattern(pat);
                self.write(" : ");
                self.fmt_expr(ty);
                self.write(")");
            }
        }
    }
}
