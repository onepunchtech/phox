use crate::ast::*;
use crate::lexer::{self, Token};

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

struct Parser<'src> {
    tokens: Vec<(Token, Span)>,
    pos: usize,
    src: &'src str,
    /// Stack of minimum columns for layout-sensitive blocks.
    /// When parsing let bindings or case branches, entries at or beyond this
    /// column are part of the block.
    min_col: Vec<usize>,
    /// Collected comments with their spans (for the formatter).
    comments: Vec<(Span, String)>,
    /// When > 0, `<` and `>` (and their `<=` / `>=` siblings) are NOT treated
    /// as comparison operators. Set while parsing the tail of variant types
    /// (`< ... ; r >`) so that the closing `>` isn't accidentally consumed as
    /// a binary operator.
    no_angle_op_depth: usize,
}

impl<'src> Parser<'src> {
    fn new(tokens: Vec<(Token, Span)>, src: &'src str) -> Self {
        Self {
            tokens,
            pos: 0,
            src,
            min_col: Vec::new(),
            comments: Vec::new(),
            no_angle_op_depth: 0,
        }
    }

    /// Get the column (0-based) for a byte offset
    fn col_of(&self, offset: usize) -> usize {
        let before = &self.src[..offset];
        match before.rfind('\n') {
            Some(nl) => offset - nl - 1,
            None => offset,
        }
    }

    /// Get the column of the current token
    fn current_col(&self) -> usize {
        self.col_of(self.peek_span().0)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|(_, s)| *s)
            .unwrap_or_else(|| {
                self.tokens
                    .last()
                    .map(|(_, s)| (s.1, s.1))
                    .unwrap_or((0, 0))
            })
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn advance(&mut self) -> (Token, Span) {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Token::Newline | Token::Comment(_))) {
            if let Some(Token::Comment(_)) = self.peek() {
                let (tok, span) = self.tokens[self.pos].clone();
                if let Token::Comment(text) = tok {
                    self.comments.push((span, text));
                }
            }
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<Span, ParseError> {
        self.skip_newlines();
        match self.peek() {
            Some(t) if t == expected => {
                let (_, span) = self.advance();
                Ok(span)
            }
            Some(t) => Err(ParseError {
                message: format!("expected `{expected}`, found `{t}`"),
                span: self.peek_span(),
            }),
            None => Err(ParseError {
                message: format!("expected `{expected}`, found end of input"),
                span: self.peek_span(),
            }),
        }
    }

    fn expect_lower(&mut self) -> Result<(String, Span), ParseError> {
        self.skip_newlines();
        match self.peek() {
            Some(Token::Lower(_)) => {
                let (tok, span) = self.advance();
                if let Token::Lower(s) = tok {
                    Ok((s, span))
                } else {
                    unreachable!()
                }
            }
            Some(t) => Err(ParseError {
                message: format!("expected identifier, found `{t}`"),
                span: self.peek_span(),
            }),
            None => Err(ParseError {
                message: "expected identifier, found end of input".into(),
                span: self.peek_span(),
            }),
        }
    }

    fn expect_upper(&mut self) -> Result<(String, Span), ParseError> {
        self.skip_newlines();
        match self.peek() {
            Some(Token::Upper(_)) => {
                let (tok, span) = self.advance();
                if let Token::Upper(s) = tok {
                    Ok((s, span))
                } else {
                    unreachable!()
                }
            }
            Some(t) => Err(ParseError {
                message: format!("expected uppercase identifier, found `{t}`"),
                span: self.peek_span(),
            }),
            None => Err(ParseError {
                message: "expected uppercase identifier, found end of input".into(),
                span: self.peek_span(),
            }),
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), ParseError> {
        self.skip_newlines();
        match self.peek() {
            Some(Token::Lower(_) | Token::Upper(_)) => {
                let (tok, span) = self.advance();
                match tok {
                    Token::Lower(s) | Token::Upper(s) => Ok((s, span)),
                    _ => unreachable!(),
                }
            }
            // Allow some keywords to be used as identifiers in binding positions
            Some(Token::KwList | Token::KwArray | Token::KwLazy | Token::KwMu) => {
                let (tok, span) = self.advance();
                Ok((format!("{tok}"), span))
            }
            Some(t) => Err(ParseError {
                message: format!("expected identifier, found `{t}`"),
                span: self.peek_span(),
            }),
            None => Err(ParseError {
                message: "expected identifier, found end of input".into(),
                span: self.peek_span(),
            }),
        }
    }

    fn check(&self, expected: &Token) -> bool {
        self.peek() == Some(expected)
    }

    fn eat(&mut self, expected: &Token) -> bool {
        let saved = self.pos;
        self.skip_newlines();
        if self.check(expected) {
            self.advance();
            true
        } else {
            self.pos = saved;
            false
        }
    }

    fn span_from(&self, start: usize) -> Span {
        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].1 .1
        } else {
            start
        };
        (start, end)
    }

    /// Check if the next non-newline token is at or beyond the current layout column
    fn in_layout_block(&self) -> bool {
        let min = match self.min_col.last() {
            Some(c) => *c,
            None => return true,
        };
        // Find next non-newline token
        let mut i = self.pos;
        while i < self.tokens.len() {
            if !matches!(self.tokens[i].0, Token::Newline) {
                let col = self.col_of(self.tokens[i].1 .0);
                return col >= min;
            }
            i += 1;
        }
        false
    }

    // === Patterns ===

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.skip_newlines();
        self.parse_pattern_atom()
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        let start = self.peek_span().0;

        match self.peek() {
            Some(Token::Underscore) => {
                let (_, span) = self.advance();
                Ok(Pattern {
                    node: PatternKind::Wildcard,
                    span,
                })
            }

            Some(Token::Lower(_)) => {
                let (name, span) = self.expect_lower()?;
                Ok(Pattern {
                    node: PatternKind::Var(name),
                    span,
                })
            }

            Some(Token::Integer(_)) => {
                let (tok, span) = self.advance();
                if let Token::Integer(n) = tok {
                    Ok(Pattern {
                        node: PatternKind::Lit(Lit::Integer(n)),
                        span,
                    })
                } else {
                    unreachable!()
                }
            }

            Some(Token::String(_)) => {
                let (tok, span) = self.advance();
                if let Token::String(s) = tok {
                    Ok(Pattern {
                        node: PatternKind::Lit(Lit::String(s)),
                        span,
                    })
                } else {
                    unreachable!()
                }
            }

            Some(Token::Char(_)) => {
                let (tok, span) = self.advance();
                if let Token::Char(c) = tok {
                    Ok(Pattern {
                        node: PatternKind::Lit(Lit::Char(c)),
                        span,
                    })
                } else {
                    unreachable!()
                }
            }

            Some(Token::True) => {
                let (_, span) = self.advance();
                Ok(Pattern {
                    node: PatternKind::Lit(Lit::Bool(true)),
                    span,
                })
            }

            Some(Token::False) => {
                let (_, span) = self.advance();
                Ok(Pattern {
                    node: PatternKind::Lit(Lit::Bool(false)),
                    span,
                })
            }

            Some(Token::Unit) => {
                let (_, span) = self.advance();
                Ok(Pattern {
                    node: PatternKind::Lit(Lit::Unit),
                    span,
                })
            }

            Some(Token::Tick) => {
                self.advance();
                let (name, _) = self.expect_upper()?;
                let inner = if self.can_start_pattern_atom() {
                    Some(Box::new(self.parse_pattern_atom()?))
                } else {
                    None
                };
                let span = self.span_from(start);
                Ok(Pattern {
                    node: PatternKind::Variant(name, inner),
                    span,
                })
            }

            Some(Token::LBrace) => {
                self.advance();
                let mut fields = Vec::new();
                while !self.check(&Token::RBrace) && !self.at_end() {
                    self.skip_newlines();
                    if self.check(&Token::RBrace) {
                        break;
                    }
                    let (name, name_span) = self.expect_lower()?;
                    if self.eat(&Token::Equals) {
                        let p = self.parse_pattern()?;
                        fields.push(RecordPatternField::Match(name, p));
                    } else {
                        fields.push(RecordPatternField::Pun(Spanned::new(name, name_span)));
                    }
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RBrace)?;
                let span = self.span_from(start);
                Ok(Pattern {
                    node: PatternKind::Record(fields),
                    span,
                })
            }

            Some(Token::LParen) => {
                self.advance();
                let p = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(p)
            }

            _ => Err(ParseError {
                message: format!(
                    "expected pattern, found `{}`",
                    self.peek()
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ),
                span: self.peek_span(),
            }),
        }
    }

    fn can_start_pattern_atom(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Underscore
                    | Token::Lower(_)
                    | Token::Integer(_)
                    | Token::String(_)
                    | Token::Char(_)
                    | Token::True
                    | Token::False
                    | Token::Unit
                    | Token::Tick
                    | Token::LBrace
                    | Token::LParen
            )
        )
    }

    // === Expressions ===

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.skip_newlines();
        self.parse_expr_top()
    }

    fn parse_expr_top(&mut self) -> Result<Expr, ParseError> {
        self.skip_newlines();
        match self.peek() {
            Some(Token::Let) => self.parse_let(),
            Some(Token::Backslash) => self.parse_lambda(),
            Some(Token::Case) => self.parse_case(),
            Some(Token::If) => self.parse_if(),
            Some(Token::Import) => self.parse_import(),
            Some(Token::KwMu) => self.parse_mu(),
            _ => self.parse_pipe(),
        }
    }

    fn parse_pipe(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut lhs = self.parse_or()?;

        while self.eat(&Token::PipeRight) {
            let rhs = self.parse_or()?;
            let span = self.span_from(start);
            lhs = spanned(ExprKind::Pipe(lhs, rhs), span);
        }

        Ok(lhs)
    }

    // Operator precedence (lowest → highest):
    //   |>   (pipe — handled above)
    //   ||
    //   &&
    //   == !=
    //   <  >  <= >=
    //   ->   (type arrow)
    //   !    (unary prefix)
    //   application
    //
    // Each level is left-associative. The `<`/`>` comparisons are
    // disambiguated from variant-type opener `< ... >` by position: variant
    // types only appear in atom position, so once we're at parse_cmp looking
    // for an operator after a parsed expression, `<` is unambiguously the
    // comparison.

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut lhs = self.parse_and()?;
        while self.eat(&Token::PipePipe) {
            let rhs = self.parse_and()?;
            let span = self.span_from(start);
            lhs = spanned(ExprKind::BinOp(BinOp::Or, lhs, rhs), span);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut lhs = self.parse_eq()?;
        while self.eat(&Token::AmpAmp) {
            let rhs = self.parse_eq()?;
            let span = self.span_from(start);
            lhs = spanned(ExprKind::BinOp(BinOp::And, lhs, rhs), span);
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut lhs = self.parse_cmp()?;
        loop {
            let op = if self.eat(&Token::EqEq) {
                BinOp::Eq
            } else if self.eat(&Token::BangEq) {
                BinOp::Neq
            } else {
                break;
            };
            let rhs = self.parse_cmp()?;
            let span = self.span_from(start);
            lhs = spanned(ExprKind::BinOp(op, lhs, rhs), span);
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut lhs = self.parse_arrow()?;
        // Suppress comparisons entirely when we're inside `< ... >` syntax
        // (variant types). The trailing `>` of the variant must not be eaten
        // as a binary operator.
        if self.no_angle_op_depth > 0 {
            return Ok(lhs);
        }
        loop {
            let op = if self.eat(&Token::LAngleEq) {
                BinOp::Lte
            } else if self.eat(&Token::RAngleEq) {
                BinOp::Gte
            } else if self.eat(&Token::LAngle) {
                BinOp::Lt
            } else if self.eat(&Token::RAngle) {
                BinOp::Gt
            } else {
                break;
            };
            let rhs = self.parse_arrow()?;
            let span = self.span_from(start);
            lhs = spanned(ExprKind::BinOp(op, lhs, rhs), span);
        }
        Ok(lhs)
    }

    fn parse_arrow(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let lhs = self.parse_unary()?;

        if self.eat(&Token::ThinArrow) {
            let rhs = self.parse_expr_top()?;
            let span = self.span_from(start);
            Ok(spanned(ExprKind::Arrow(lhs, rhs), span))
        } else {
            Ok(lhs)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        if self.eat(&Token::Bang) {
            let inner = self.parse_unary()?;
            let span = self.span_from(start);
            Ok(spanned(ExprKind::UnOp(UnOp::Not, inner), span))
        } else {
            self.parse_spine()
        }
    }

    fn parse_spine(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let func = self.parse_access()?;

        let mut args = Vec::new();
        while self.can_start_arg() {
            if self.check(&Token::QBrace) {
                self.advance();
                let fields = self.parse_record_fields()?;
                self.expect(&Token::RBrace)?;
                let span = self.span_from(start);
                args.push(AppArg {
                    icity: Icity::Implicit,
                    expr: spanned(ExprKind::Record(fields), span),
                });
            } else {
                let arg = self.parse_access()?;
                args.push(AppArg {
                    icity: Icity::Explicit,
                    expr: arg,
                });
            }
        }

        if args.is_empty() {
            Ok(func)
        } else {
            let span = self.span_from(start);
            Ok(spanned(ExprKind::App(func, args), span))
        }
    }

    fn can_start_arg(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Lower(_)
                    | Token::Upper(_)
                    | Token::Integer(_)
                    | Token::Double(_)
                    | Token::String(_)
                    | Token::Char(_)
                    | Token::True
                    | Token::False
                    | Token::Unit
                    | Token::LParen
                    | Token::LBrace
                    | Token::LBracket
                    | Token::QBrace
                    | Token::Question
                    | Token::Undefined
                    | Token::KwType
                    | Token::KwString
                    | Token::KwInteger
                    | Token::KwDouble
                    | Token::KwChar
                    | Token::KwBool
                    | Token::KwUnit
                    | Token::KwRow
                    | Token::KwRec
                    | Token::KwMu
                    | Token::Tick
                    | Token::Fold
                    | Token::Unfold
            )
        )
    }

    fn parse_access(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        let mut expr = self.parse_atom()?;

        while self.check(&Token::Dot) {
            self.advance();
            let (field, _) = self.expect_lower()?;
            let span = self.span_from(start);
            expr = spanned(ExprKind::RecordAccess(expr, field), span);
        }

        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;

        match self.peek() {
            Some(Token::Lower(_)) => {
                let (name, span) = self.expect_lower()?;
                Ok(spanned(ExprKind::Var(name), span))
            }
            Some(Token::Upper(_)) => {
                let (name, span) = self.expect_upper()?;
                Ok(spanned(ExprKind::Var(name), span))
            }

            Some(Token::Integer(_)) => {
                let (tok, span) = self.advance();
                if let Token::Integer(n) = tok {
                    Ok(spanned(ExprKind::Lit(Lit::Integer(n)), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::Double(_)) => {
                let (tok, span) = self.advance();
                if let Token::Double(n) = tok {
                    Ok(spanned(ExprKind::Lit(Lit::Double(n)), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::String(_)) => {
                let (tok, span) = self.advance();
                if let Token::String(s) = tok {
                    Ok(spanned(ExprKind::Lit(Lit::String(s)), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::Char(_)) => {
                let (tok, span) = self.advance();
                if let Token::Char(c) = tok {
                    Ok(spanned(ExprKind::Lit(Lit::Char(c)), span))
                } else {
                    unreachable!()
                }
            }
            Some(Token::True) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Lit(Lit::Bool(true)), span))
            }
            Some(Token::False) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Lit(Lit::Bool(false)), span))
            }
            Some(Token::Unit) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Lit(Lit::Unit), span))
            }
            Some(Token::Undefined) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Undefined, span))
            }

            Some(
                Token::KwType | Token::KwString | Token::KwInteger | Token::KwDouble
                | Token::KwChar | Token::KwBool | Token::KwUnit | Token::KwRow,
            ) => {
                let (tok, span) = self.advance();
                let tl = match tok {
                    Token::KwType => TypeLit::Type,
                    Token::KwString => TypeLit::String,
                    Token::KwInteger => TypeLit::Integer,
                    Token::KwDouble => TypeLit::Double,
                    Token::KwChar => TypeLit::Char,
                    Token::KwBool => TypeLit::Bool,
                    Token::KwUnit => TypeLit::Unit,
                    Token::KwRow => TypeLit::Row,
                    _ => unreachable!(),
                };
                Ok(spanned(ExprKind::TypeLit(tl), span))
            }

            Some(Token::KwMu) => {
                return self.parse_mu();
            }

            Some(Token::KwRec) => {
                self.advance();
                self.expect(&Token::LBrace)?;
                let (fields, tail) = self.parse_record_type_fields()?;
                self.expect(&Token::RBrace)?;
                let span = self.span_from(start);
                Ok(spanned(ExprKind::RecordType(fields, tail), span))
            }

            Some(Token::KwList) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Var("List".into()), span))
            }
            Some(Token::KwArray) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Var("Array".into()), span))
            }
            Some(Token::KwLazy) => {
                let (_, span) = self.advance();
                Ok(spanned(ExprKind::Var("Lazy".into()), span))
            }

            Some(Token::Question) => {
                self.advance();
                let (name, _) = self.expect_lower()?;
                let span = self.span_from(start);
                Ok(spanned(ExprKind::Hole(name), span))
            }

            Some(Token::Tick) => {
                self.advance();
                let (name, _) = self.expect_upper()?;
                let payload = if self.can_start_arg() && !self.check(&Token::Tick) {
                    Some(self.parse_access()?)
                } else {
                    None
                };
                let span = self.span_from(start);
                Ok(spanned(ExprKind::Variant(name, payload), span))
            }

            Some(Token::Fold) => {
                self.advance();
                let arg = self.parse_access()?;
                let span = self.span_from(start);
                Ok(spanned(ExprKind::Fold(arg), span))
            }

            Some(Token::Unfold) => {
                self.advance();
                let arg = self.parse_access()?;
                let span = self.span_from(start);
                Ok(spanned(ExprKind::Unfold(arg), span))
            }

            Some(Token::LParen) => self.parse_paren_expr(),
            Some(Token::LBrace) => self.parse_brace_expr(),

            Some(Token::LBracket) => {
                self.advance();
                let mut elems = Vec::new();
                self.skip_newlines();
                while !self.check(&Token::RBracket) && !self.at_end() {
                    elems.push(self.parse_expr_top()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect(&Token::RBracket)?;
                let span = self.span_from(start);
                Ok(spanned(ExprKind::List(elems), span))
            }

            Some(Token::LAngle) => self.parse_variant_type(),

            _ => Err(ParseError {
                message: format!(
                    "expected expression, found `{}`",
                    self.peek()
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ),
                span: self.peek_span(),
            }),
        }
    }

    fn parse_paren_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // (
        self.skip_newlines();

        let inner = self.parse_expr_top()?;

        if self.eat(&Token::Colon) {
            let ty = self.parse_expr_top()?;
            self.expect(&Token::RParen)?;

            if self.eat(&Token::ThinArrow) {
                let ret = self.parse_expr_top()?;
                let span = self.span_from(start);
                let name = if let ExprKind::Var(ref n) = inner.node {
                    Some(n.clone())
                } else {
                    None
                };
                Ok(spanned(
                    ExprKind::Pi(
                        vec![PiParam {
                            icity: Icity::Explicit,
                            name,
                            ty,
                        }],
                        ret,
                    ),
                    span,
                ))
            } else {
                let span = self.span_from(start);
                Ok(spanned(ExprKind::Ann(inner, ty), span))
            }
        } else {
            self.expect(&Token::RParen)?;
            Ok(inner)
        }
    }

    fn parse_brace_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // {
        self.skip_newlines();

        if self.check(&Token::RBrace) {
            self.advance();
            let span = self.span_from(start);
            return Ok(spanned(ExprKind::Record(vec![]), span));
        }

        // Record spread/update: { ...base, field = value, ... }
        if self.check(&Token::DotDotDot) {
            self.advance();
            let base = self.parse_atom()?;
            let fields = if self.eat(&Token::Comma) {
                self.skip_newlines();
                self.parse_record_fields()?
            } else {
                vec![]
            };
            self.skip_newlines();
            self.expect(&Token::RBrace)?;
            let span = self.span_from(start);
            return Ok(spanned(ExprKind::RecordUpdate(base, fields), span));
        }

        if let Some(Token::Lower(_)) = self.peek() {
            let saved = self.pos;
            let _ = self.expect_lower();

            if self.check(&Token::Colon) {
                self.pos = saved;
                let (fields, tail) = self.parse_record_type_fields()?;
                self.expect(&Token::RBrace)?;
                let span = self.span_from(start);
                return Ok(spanned(ExprKind::RecordType(fields, tail), span));
            } else if self.check(&Token::Equals) {
                self.pos = saved;
                let fields = self.parse_record_fields()?;
                self.expect(&Token::RBrace)?;
                let span = self.span_from(start);
                return Ok(spanned(ExprKind::Record(fields), span));
            } else {
                self.pos = saved;
            }
        }

        let fields = self.parse_record_fields()?;
        self.expect(&Token::RBrace)?;
        let span = self.span_from(start);
        Ok(spanned(ExprKind::Record(fields), span))
    }

    fn parse_record_fields(&mut self) -> Result<Vec<RecordField>, ParseError> {
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            self.skip_newlines();
            if self.check(&Token::RBrace) {
                break;
            }
            let (name, _) = self.expect_lower()?;
            self.expect(&Token::Equals)?;
            let value = self.parse_expr_top()?;
            fields.push(RecordField { name, value });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(fields)
    }

    fn parse_record_type_fields(
        &mut self,
    ) -> Result<(Vec<RecordTypeField>, Option<Expr>), ParseError> {
        let mut fields = Vec::new();
        let mut tail = None;

        while !self.check(&Token::RBrace) && !self.at_end() {
            self.skip_newlines();
            if self.check(&Token::RBrace) {
                break;
            }
            let (name, _) = self.expect_lower()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_expr_top()?;
            fields.push(RecordTypeField { name, ty });

            if self.eat(&Token::Semicolon) {
                tail = Some(self.parse_expr_top()?);
                break;
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok((fields, tail))
    }

    fn parse_variant_type(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // <
        self.skip_newlines();

        let mut tags = Vec::new();
        let mut tail = None;

        // Disable `<`/`>` comparison parsing until the matching `>`. Otherwise
        // the closing `>` would be consumed as a binary operator by parse_cmp
        // when parsing the tail or any tag payload's inner expressions.
        self.no_angle_op_depth += 1;
        let mut parse_err: Option<ParseError> = None;
        while !self.check(&Token::RAngle) && !self.at_end() {
            self.skip_newlines();
            if self.check(&Token::RAngle) {
                break;
            }
            if let Err(e) = self.expect(&Token::Tick) {
                parse_err = Some(e);
                break;
            }
            let (name, _) = match self.expect_upper() {
                Ok(p) => p,
                Err(e) => {
                    parse_err = Some(e);
                    break;
                }
            };

            let payload = if self.can_start_arg() && !self.check(&Token::Tick) {
                match self.parse_access() {
                    Ok(p) => Some(p),
                    Err(e) => {
                        parse_err = Some(e);
                        break;
                    }
                }
            } else {
                None
            };
            tags.push(VariantTag { name, payload });

            if self.eat(&Token::Semicolon) {
                match self.parse_expr_top() {
                    Ok(t) => tail = Some(t),
                    Err(e) => parse_err = Some(e),
                }
                break;
            }
            if !self.eat(&Token::Pipe) {
                break;
            }
            self.skip_newlines();
        }
        self.no_angle_op_depth -= 1;
        if let Some(e) = parse_err {
            return Err(e);
        }

        self.expect(&Token::RAngle)?;
        let span = self.span_from(start);
        Ok(spanned(ExprKind::VariantType(tags, tail), span))
    }

    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // backslash

        let mut params = Vec::new();
        while self.can_start_pattern_atom() && !self.check(&Token::FatArrow) {
            params.push(self.parse_pattern()?);
        }

        if params.is_empty() {
            return Err(ParseError {
                message: "lambda requires at least one parameter".into(),
                span: self.peek_span(),
            });
        }

        self.expect(&Token::FatArrow)?;
        let body = self.parse_expr_top()?;
        let span = self.span_from(start);
        Ok(spanned(ExprKind::Lam(params, body), span))
    }

    fn parse_case(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // case

        let scrutinee = self.parse_expr_top()?;
        self.expect(&Token::Of)?;
        self.skip_newlines();

        // Determine the indentation level of the first branch
        let branch_col = self.current_col();
        self.min_col.push(branch_col);

        let mut branches = Vec::new();
        while self.can_start_pattern_atom() && self.in_layout_block() {
            let pattern = self.parse_pattern()?;
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr_top()?;
            branches.push(CaseBranch { pattern, body });
            self.skip_newlines();
        }

        self.min_col.pop();

        if branches.is_empty() {
            return Err(ParseError {
                message: "case expression requires at least one branch".into(),
                span: self.peek_span(),
            });
        }

        let span = self.span_from(start);
        Ok(spanned(ExprKind::Case(scrutinee, branches), span))
    }

    fn parse_if(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // if

        let cond = self.parse_expr_top()?;
        self.expect(&Token::Then)?;
        let then_expr = self.parse_expr_top()?;
        self.expect(&Token::Else)?;
        let else_expr = self.parse_expr_top()?;

        let span = self.span_from(start);
        Ok(spanned(ExprKind::If(cond, then_expr, else_expr), span))
    }

    fn parse_let(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // let
        self.skip_newlines();

        // Determine the indentation level of the first binding
        let bind_col = self.current_col();
        self.min_col.push(bind_col);

        let mut bindings = Vec::new();

        loop {
            self.skip_newlines();
            if self.check(&Token::In) || self.at_end() {
                break;
            }
            // Check if we're still in the layout block
            if !self.in_layout_block() {
                break;
            }

            let binding = self.parse_let_binding()?;
            bindings.push(binding);
        }

        self.min_col.pop();

        if bindings.is_empty() {
            return Err(ParseError {
                message: "let requires at least one binding".into(),
                span: self.peek_span(),
            });
        }

        self.expect(&Token::In)?;
        let body = self.parse_expr_top()?;

        let span = self.span_from(start);
        Ok(spanned(ExprKind::Let(bindings, body), span))
    }

    fn parse_let_binding(&mut self) -> Result<LetBinding, ParseError> {
        let bind_start = self.peek_span().0;
        let (name, _) = self.expect_ident()?;

        let ty = if self.check(&Token::Colon) {
            self.advance();
            let t = self.parse_arrow()?;
            Some(t)
        } else {
            None
        };

        if ty.is_some() && !self.check(&Token::Equals) {
            // Two-line form: `name : Type\n name params = value`
            self.skip_newlines();
            let (name2, _) = self.expect_ident()?;
            if name != name2 {
                return Err(ParseError {
                    message: format!(
                        "type declaration for `{name}` followed by binding for `{name2}`"
                    ),
                    span: self.peek_span(),
                });
            }
        }

        let mut params = Vec::new();
        while self.can_start_pattern_atom() && !self.check(&Token::Equals) {
            params.push(self.parse_pattern()?);
        }

        self.expect(&Token::Equals)?;
        let value = self.parse_expr_top()?;
        let span = self.span_from(bind_start);

        Ok(LetBinding {
            ty,
            name,
            params,
            value,
            span,
        })
    }

    fn parse_mu(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // Mu
        let (name, _) = self.expect_lower()?;
        self.expect(&Token::Dot)?;
        let body = self.parse_expr_top()?;
        let span = self.span_from(start);
        Ok(spanned(ExprKind::Mu(name, body), span))
    }

    fn parse_import(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek_span().0;
        self.advance(); // import

        match self.peek() {
            Some(Token::String(_)) => {
                let (tok, _) = self.advance();
                if let Token::String(path) = tok {
                    let span = self.span_from(start);
                    Ok(spanned(ExprKind::Import(path), span))
                } else {
                    unreachable!()
                }
            }
            _ => Err(ParseError {
                message: "expected string after import".into(),
                span: self.peek_span(),
            }),
        }
    }
}

fn spanned(kind: ExprKind, span: Span) -> Expr {
    Box::new(Spanned::new(kind, span))
}

pub fn parse(input: &str) -> Result<Expr, Vec<ParseError>> {
    let (expr, _comments) = parse_with_comments(input)?;
    Ok(expr)
}

pub fn parse_with_comments(input: &str) -> Result<(Expr, Vec<(Span, String)>), Vec<ParseError>> {
    let tokens = lexer::lex(input).map_err(|e| {
        vec![ParseError {
            message: e.message,
            span: e.span,
        }]
    })?;

    let mut parser = Parser::new(tokens, input);
    let expr = parser.parse_expr().map_err(|e| vec![e])?;

    parser.skip_newlines();
    if !parser.at_end() {
        return Err(vec![ParseError {
            message: format!("unexpected token `{}`", parser.peek().unwrap()),
            span: parser.peek_span(),
        }]);
    }

    Ok((expr, parser.comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Expr {
        match parse(input) {
            Ok(expr) => expr,
            Err(errs) => {
                let msgs: Vec<_> = errs.iter().map(|e| format!("{e}")).collect();
                panic!("Parse failed:\n{}", msgs.join("\n"));
            }
        }
    }

    fn assert_parses(input: &str) {
        parse_ok(input);
    }

    // === Literals ===

    #[test]
    fn test_integer_literal() {
        let e = parse_ok("42");
        assert!(matches!(e.node, ExprKind::Lit(Lit::Integer(42))));
    }

    #[test]
    fn test_string_literal() {
        let e = parse_ok("\"hello\"");
        assert!(matches!(e.node, ExprKind::Lit(Lit::String(ref s)) if s == "hello"));
    }

    #[test]
    fn test_bool_literals() {
        assert!(matches!(parse_ok("True").node, ExprKind::Lit(Lit::Bool(true))));
        assert!(matches!(parse_ok("False").node, ExprKind::Lit(Lit::Bool(false))));
    }

    #[test]
    fn test_unit() {
        assert!(matches!(parse_ok("()").node, ExprKind::Lit(Lit::Unit)));
    }

    #[test]
    fn test_char_literal() {
        let e = parse_ok("'x'");
        assert!(matches!(e.node, ExprKind::Lit(Lit::Char('x'))));
    }

    #[test]
    fn test_negative_integer() {
        let e = parse_ok("-42");
        assert!(matches!(e.node, ExprKind::Lit(Lit::Integer(-42))));
    }

    #[test]
    fn test_escape_in_string() {
        let e = parse_ok("\"hello\\nworld\"");
        if let ExprKind::Lit(Lit::String(ref s)) = e.node {
            assert_eq!(s, "hello\nworld");
        } else {
            panic!("expected string");
        }
    }

    // === Variables & Types ===

    #[test]
    fn test_variable() {
        assert!(matches!(parse_ok("foo").node, ExprKind::Var(ref s) if s == "foo"));
    }

    #[test]
    fn test_type_literal() {
        assert!(matches!(parse_ok("Type").node, ExprKind::TypeLit(TypeLit::Type)));
        assert!(matches!(parse_ok("String").node, ExprKind::TypeLit(TypeLit::String)));
    }

    #[test]
    fn test_undefined() {
        assert!(matches!(parse_ok("undefined").node, ExprKind::Undefined));
    }

    #[test]
    fn test_hole() {
        let e = parse_ok("?foo");
        assert!(matches!(e.node, ExprKind::Hole(ref s) if s == "foo"));
    }

    // === Lambda ===

    #[test]
    fn test_lambda_simple() {
        let e = parse_ok("\\x => x");
        assert!(matches!(e.node, ExprKind::Lam(..)));
    }

    #[test]
    fn test_lambda_multi_param() {
        let e = parse_ok("\\x y z => x");
        if let ExprKind::Lam(params, _) = &e.node {
            assert_eq!(params.len(), 3);
        } else {
            panic!("expected lambda");
        }
    }

    #[test]
    fn test_lambda_record_pattern() {
        assert_parses("\\{x, y} => x");
    }

    #[test]
    fn test_lambda_variant_pattern() {
        assert_parses("\\'Just x => x");
    }

    // === Application ===

    #[test]
    fn test_application() {
        let e = parse_ok("f x");
        assert!(matches!(e.node, ExprKind::App(..)));
    }

    #[test]
    fn test_multi_application() {
        let e = parse_ok("f x y z");
        if let ExprKind::App(_, args) = &e.node {
            assert_eq!(args.len(), 3);
        } else {
            panic!("expected app");
        }
    }

    #[test]
    fn test_app_with_paren_group() {
        assert_parses("f (x y) z");
    }

    // === Records ===

    #[test]
    fn test_record_literal() {
        assert_parses("{x = 1, y = 2}");
    }

    #[test]
    fn test_empty_record() {
        let e = parse_ok("{}");
        if let ExprKind::Record(fields) = &e.node {
            assert!(fields.is_empty());
        } else {
            panic!("expected empty record");
        }
    }

    #[test]
    fn test_nested_record() {
        assert_parses("{name = \"John\", address = {street = \"Main\", city = \"NYC\"}}");
    }

    #[test]
    fn test_record_access() {
        let e = parse_ok("foo.bar");
        assert!(matches!(e.node, ExprKind::RecordAccess(..)));
    }

    #[test]
    fn test_chained_access() {
        let e = parse_ok("foo.bar.baz");
        if let ExprKind::RecordAccess(inner, field) = &e.node {
            assert_eq!(field, "baz");
            assert!(matches!(inner.node, ExprKind::RecordAccess(..)));
        } else {
            panic!("expected chained access");
        }
    }

    // === Record Types ===

    #[test]
    fn test_record_type() {
        let e = parse_ok("{x : String, y : Integer}");
        assert!(matches!(e.node, ExprKind::RecordType(..)));
    }

    #[test]
    fn test_rec_type() {
        assert_parses("Rec {x : String, y : Integer}");
    }

    #[test]
    fn test_record_type_with_tail() {
        assert_parses("{x : String ; r}");
    }

    // === Variants ===

    #[test]
    fn test_variant_constructor() {
        let e = parse_ok("'Just x");
        if let ExprKind::Variant(name, Some(_)) = &e.node {
            assert_eq!(name, "Just");
        } else {
            panic!("expected variant");
        }
    }

    #[test]
    fn test_variant_nullary() {
        let e = parse_ok("'Nothing");
        if let ExprKind::Variant(name, None) = &e.node {
            assert_eq!(name, "Nothing");
        } else {
            panic!("expected variant with no payload");
        }
    }

    #[test]
    fn test_variant_type() {
        let e = parse_ok("< 'Nothing | 'Just String >");
        assert!(matches!(e.node, ExprKind::VariantType(..)));
    }

    #[test]
    fn test_variant_type_with_tail() {
        assert_parses("< 'Left String | 'Right Integer ; r >");
    }

    // === List ===

    #[test]
    fn test_list_literal() {
        let e = parse_ok("[1, 2, 3]");
        if let ExprKind::List(elems) = &e.node {
            assert_eq!(elems.len(), 3);
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_empty_list() {
        let e = parse_ok("[]");
        if let ExprKind::List(elems) = &e.node {
            assert!(elems.is_empty());
        } else {
            panic!("expected list");
        }
    }

    // === Binary + unary operators ===

    #[test]
    fn test_op_equality() {
        let e = parse_ok("1 == 2");
        assert!(matches!(e.node, ExprKind::BinOp(BinOp::Eq, _, _)));
    }

    #[test]
    fn test_op_neq() {
        let e = parse_ok("x != 0");
        assert!(matches!(e.node, ExprKind::BinOp(BinOp::Neq, _, _)));
    }

    #[test]
    fn test_op_ordering() {
        assert!(matches!(parse_ok("a < b").node, ExprKind::BinOp(BinOp::Lt, _, _)));
        assert!(matches!(parse_ok("a > b").node, ExprKind::BinOp(BinOp::Gt, _, _)));
        assert!(matches!(parse_ok("a <= b").node, ExprKind::BinOp(BinOp::Lte, _, _)));
        assert!(matches!(parse_ok("a >= b").node, ExprKind::BinOp(BinOp::Gte, _, _)));
    }

    #[test]
    fn test_op_logical() {
        let e = parse_ok("True && False");
        assert!(matches!(e.node, ExprKind::BinOp(BinOp::And, _, _)));
        let e = parse_ok("a || b");
        assert!(matches!(e.node, ExprKind::BinOp(BinOp::Or, _, _)));
    }

    #[test]
    fn test_op_not() {
        let e = parse_ok("!x");
        assert!(matches!(e.node, ExprKind::UnOp(UnOp::Not, _)));
    }

    #[test]
    fn test_op_precedence_and_over_or() {
        // a || b && c parses as a || (b && c)
        let e = parse_ok("a || b && c");
        if let ExprKind::BinOp(BinOp::Or, _lhs, rhs) = &e.node {
            assert!(matches!(rhs.node, ExprKind::BinOp(BinOp::And, _, _)));
        } else {
            panic!("expected top-level || with nested &&");
        }
    }

    #[test]
    fn test_op_precedence_cmp_over_and() {
        // x < y && z parses as (x < y) && z
        let e = parse_ok("x < y && z");
        if let ExprKind::BinOp(BinOp::And, lhs, _rhs) = &e.node {
            assert!(matches!(lhs.node, ExprKind::BinOp(BinOp::Lt, _, _)));
        } else {
            panic!("expected top-level && with nested <");
        }
    }

    #[test]
    fn test_op_variant_type_still_parses() {
        // Regression: `< 'Tag ; r >` must still parse as a variant type;
        // the trailing `>` cannot be eaten as a comparison.
        assert_parses("< 'Left String | 'Right Integer ; r >");
        assert_parses("< 'A >");
    }

    // === Arrow / Pi ===

    #[test]
    fn test_arrow_type() {
        let e = parse_ok("String -> Integer");
        assert!(matches!(e.node, ExprKind::Arrow(..)));
    }

    #[test]
    fn test_arrow_right_assoc() {
        let e = parse_ok("A -> B -> C");
        if let ExprKind::Arrow(_, rhs) = &e.node {
            assert!(matches!(rhs.node, ExprKind::Arrow(..)));
        } else {
            panic!("expected arrow");
        }
    }

    #[test]
    fn test_type_annotation() {
        let e = parse_ok("(x : String)");
        assert!(matches!(e.node, ExprKind::Ann(..)));
    }

    #[test]
    fn test_pi_type() {
        let e = parse_ok("(x : Type) -> String");
        assert!(matches!(e.node, ExprKind::Pi(..)));
    }

    #[test]
    fn test_complex_type() {
        assert_parses("(a : Type) -> (b : Type) -> a -> b");
    }

    // === Control flow ===

    #[test]
    fn test_if_then_else() {
        assert_parses("if True then 1 else 0");
    }

    #[test]
    fn test_case_expression() {
        assert_parses("case x of\n  'Just v => v\n  'Nothing => 0");
    }

    #[test]
    fn test_case_single_line() {
        assert_parses("case x of _ => 42");
    }

    #[test]
    fn test_case_with_record_pattern() {
        assert_parses("case x of\n  {a, b} => a");
    }

    // === Let ===

    #[test]
    fn test_let_simple() {
        assert_parses("let x = 5 in x");
    }

    #[test]
    fn test_let_with_type() {
        assert_parses("let\n  x : Integer\n  x = 5\nin x");
    }

    #[test]
    fn test_let_multiple_bindings() {
        assert_parses("let\n  x = 1\n  y = 2\nin x");
    }

    #[test]
    fn test_let_function_binding() {
        assert_parses("let f x = x in f 42");
    }

    // === Import ===

    #[test]
    fn test_import() {
        assert_parses("import \"./Foo\"");
    }

    // === Multi-line examples ===

    #[test]
    fn test_multiline_let_with_types() {
        assert_parses(
            "let\n  Either : Type -> Type -> Type\n  Either l r = < 'Left l | 'Right r >\nin Either",
        );
    }

    #[test]
    fn test_multiline_case_multiple_branches() {
        assert_parses("case x of\n  'Left err => err\n  'Right val => val");
    }

    #[test]
    fn test_multiline_record_type() {
        assert_parses("Rec {\n  name : String,\n  age : Integer\n}");
    }

    #[test]
    fn test_nested_let() {
        assert_parses("let x = let y = 1 in y in x");
    }

    #[test]
    fn test_full_example() {
        assert_parses(
            "\
let
  Either : Type -> Type -> Type
  Either l r = < 'Left l | 'Right r >

  Maybe : Type -> Type
  Maybe t = < 'Nothing | 'Just t >

  fromMaybe : (a : Type) -> a -> Maybe a -> a
  fromMaybe _ default val = case val of
    'Just x => x
    'Nothing => default

  result : Either String Integer
  result = 'Right 42
in fromMaybe Integer 0 ('Just 5)",
        );
    }

    #[test]
    fn test_case_indentation_ends_block() {
        // Case branches at col 4, next let binding at col 2
        // The case should stop when indentation decreases
        assert_parses(
            "\
let
  f x = case x of
    'Just v => v
    'Nothing => 0
  g = 42
in f g",
        );
    }
}
