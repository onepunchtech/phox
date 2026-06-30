use logos::Logos;

use crate::ast::Span;

fn lex_string(lex: &mut logos::Lexer<'_, RawToken>) -> Option<String> {
    let remainder = lex.remainder();
    let mut s = String::new();
    let mut chars = remainder.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                lex.bump(i + 1);
                return Some(s);
            }
            '\\' => match chars.next() {
                Some((_, 'n')) => s.push('\n'),
                Some((_, 't')) => s.push('\t'),
                Some((_, 'r')) => s.push('\r'),
                Some((_, '\\')) => s.push('\\'),
                Some((_, '"')) => s.push('"'),
                Some((_, '\'')) => s.push('\''),
                Some((_, '0')) => s.push('\0'),
                _ => return None,
            },
            _ => s.push(c),
        }
    }
    None // unterminated
}

/// Result of lexing a single quote: either a char literal or a tick (variant prefix).
#[derive(Debug, Clone, PartialEq)]
enum QuoteResult {
    Char(char),
    Tick,
}

fn lex_quote(lex: &mut logos::Lexer<'_, RawToken>) -> Option<QuoteResult> {
    let remainder = lex.remainder();
    let mut chars = remainder.chars();

    let first = chars.next()?;

    // 'X where X is uppercase => tick for variant constructor
    if first.is_ascii_uppercase() {
        // Don't bump — the uppercase ident will be lexed as a separate token
        return Some(QuoteResult::Tick);
    }

    // Try char literal: 'c' or '\n'
    let c = match first {
        '\\' => match chars.next()? {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            '\'' => '\'',
            '0' => '\0',
            _ => return None,
        },
        '\'' => return None, // empty char literal
        c => c,
    };
    if chars.next()? != '\'' {
        return None;
    }
    let consumed = remainder.len() - chars.as_str().len();
    lex.bump(consumed);
    Some(QuoteResult::Char(c))
}

/// Raw token produced by logos. We post-process identifiers into keywords
/// and handle a few compound tokens in a second pass.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
enum RawToken {
    // Comments
    #[regex(r"--[^\n]*", allow_greedy = true)]
    Comment,

    // Compound tokens (must come before single-char variants)
    #[token("=>")]
    FatArrow,
    #[token("->")]
    ThinArrow,
    #[token("|>")]
    PipeRight,
    #[token("?{")]
    QBrace,
    #[token("()")]
    Unit,

    // Comparison + boolean operators. Listed before the single-char
    // siblings (=, <, >, !, |) so the multi-char rules win.
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    LAngleEq,
    #[token(">=")]
    RAngleEq,
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,
    #[token("!")]
    Bang,

    // Multi-char symbols (before single-char to take priority)
    #[token("...")]
    DotDotDot,

    // Single-char symbols
    #[token("\\")]
    Backslash,
    #[token(":")]
    Colon,
    #[token("=")]
    Equals,
    #[token(".")]
    Dot,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token("|")]
    Pipe,
    #[token("?")]
    Question,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    LAngle,
    #[token(">")]
    RAngle,
    #[token("\n")]
    Newline,

    // Literals
    #[regex(r"[0-9]+\.[0-9]+([eE][+\-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    Double(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Integer(i64),

    #[token("\"", lex_string)]
    String(String),

    // Single quote: either a char literal ('x') or a tick for variants ('Tag)
    #[token("'", lex_quote)]
    Quote(QuoteResult),

    // Negative numbers: - immediately followed by digits
    #[regex(r"-[0-9]+\.[0-9]+([eE][+\-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    NegDouble(f64),

    #[regex(r"-[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    NegInteger(i64),

    // Identifiers (keywords resolved in post-processing)
    #[regex(r"[a-z_][a-zA-Z0-9_']*")]
    LowerIdent,

    #[regex(r"[A-Z][a-zA-Z0-9_']*")]
    UpperIdent,
}

// --- Public Token type (unchanged from before) ---

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Integer(i64),
    Double(f64),
    String(String),
    Char(char),
    Lower(String),
    Upper(String),

    // Keywords
    Let,
    In,
    Case,
    Of,
    If,
    Then,
    Else,
    Import,
    Undefined,

    // Type keywords
    KwType,
    KwString,
    KwInteger,
    KwDouble,
    KwChar,
    KwBool,
    KwUnit,
    KwRow,
    KwRec,
    KwVar,
    KwList,
    KwArray,
    KwLazy,
    KwMu,

    // Literal keywords
    True,
    False,

    // Fold/Unfold keywords
    Fold,
    Unfold,

    // Symbols
    Backslash,
    FatArrow,
    ThinArrow,
    Colon,
    Equals,
    Dot,
    Comma,
    Semicolon,
    Pipe,
    Underscore,
    Tick,
    Question,

    // Brackets
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LAngle,
    RAngle,

    // Compound
    QBrace,
    Unit,
    DotDotDot,
    PipeRight,

    // Binary + unary operators
    EqEq,
    BangEq,
    LAngleEq,
    RAngleEq,
    AmpAmp,
    PipePipe,
    Bang,

    Newline,

    Comment(String),
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Integer(n) => write!(f, "{n}"),
            Token::Double(n) => write!(f, "{n}"),
            Token::String(s) => write!(f, "\"{s}\""),
            Token::Char(c) => write!(f, "'{c}'"),
            Token::Lower(s) | Token::Upper(s) => write!(f, "{s}"),
            Token::Let => write!(f, "let"),
            Token::In => write!(f, "in"),
            Token::Case => write!(f, "case"),
            Token::Of => write!(f, "of"),
            Token::If => write!(f, "if"),
            Token::Then => write!(f, "then"),
            Token::Else => write!(f, "else"),
            Token::Import => write!(f, "import"),
            Token::Undefined => write!(f, "undefined"),
            Token::KwType => write!(f, "Type"),
            Token::KwString => write!(f, "String"),
            Token::KwInteger => write!(f, "Integer"),
            Token::KwDouble => write!(f, "Double"),
            Token::KwChar => write!(f, "Char"),
            Token::KwBool => write!(f, "Bool"),
            Token::KwUnit => write!(f, "Unit"),
            Token::KwRow => write!(f, "Row"),
            Token::KwRec => write!(f, "Rec"),
            Token::KwVar => write!(f, "Var"),
            Token::KwList => write!(f, "List"),
            Token::KwArray => write!(f, "Array"),
            Token::KwLazy => write!(f, "Lazy"),
            Token::KwMu => write!(f, "Mu"),
            Token::True => write!(f, "True"),
            Token::False => write!(f, "False"),
            Token::Fold => write!(f, "fold"),
            Token::Unfold => write!(f, "unfold"),
            Token::Backslash => write!(f, "\\"),
            Token::FatArrow => write!(f, "=>"),
            Token::ThinArrow => write!(f, "->"),
            Token::Colon => write!(f, ":"),
            Token::Equals => write!(f, "="),
            Token::Dot => write!(f, "."),
            Token::Comma => write!(f, ","),
            Token::Semicolon => write!(f, ";"),
            Token::Pipe => write!(f, "|"),
            Token::Underscore => write!(f, "_"),
            Token::Tick => write!(f, "'"),
            Token::Question => write!(f, "?"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LAngle => write!(f, "<"),
            Token::RAngle => write!(f, ">"),
            Token::QBrace => write!(f, "?{{"),
            Token::Unit => write!(f, "()"),
            Token::DotDotDot => write!(f, "..."),
            Token::PipeRight => write!(f, "|>"),
            Token::EqEq => write!(f, "=="),
            Token::BangEq => write!(f, "!="),
            Token::LAngleEq => write!(f, "<="),
            Token::RAngleEq => write!(f, ">="),
            Token::AmpAmp => write!(f, "&&"),
            Token::PipePipe => write!(f, "||"),
            Token::Bang => write!(f, "!"),
            Token::Newline => write!(f, "\\n"),
            Token::Comment(s) => write!(f, "{s}"),
        }
    }
}

pub type Spanned = (Token, Span);

#[derive(Debug)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Resolve an identifier slice into a keyword or Lower/Upper token.
fn resolve_lower(s: &str) -> Token {
    match s {
        "let" => Token::Let,
        "in" => Token::In,
        "case" => Token::Case,
        "of" => Token::Of,
        "if" => Token::If,
        "then" => Token::Then,
        "else" => Token::Else,
        "import" => Token::Import,
        "undefined" => Token::Undefined,
        "fold" => Token::Fold,
        "unfold" => Token::Unfold,
        "_" => Token::Underscore,
        _ => Token::Lower(s.to_string()),
    }
}

fn resolve_upper(s: &str) -> Token {
    match s {
        "Type" => Token::KwType,
        "String" => Token::KwString,
        "Integer" => Token::KwInteger,
        "Double" => Token::KwDouble,
        "Char" => Token::KwChar,
        "Bool" => Token::KwBool,
        "Unit" => Token::KwUnit,
        "Row" => Token::KwRow,
        "Rec" => Token::KwRec,
        "Var" => Token::KwVar,
        "List" => Token::KwList,
        "Array" => Token::KwArray,
        "Lazy" => Token::KwLazy,
        "Mu" => Token::KwMu,
        "True" => Token::True,
        "False" => Token::False,
        _ => Token::Upper(s.to_string()),
    }
}

pub fn lex(src: &str) -> Result<Vec<Spanned>, LexError> {
    let mut tokens = Vec::new();
    let mut lexer = RawToken::lexer(src);

    while let Some(result) = lexer.next() {
        let span: Span = (lexer.span().start, lexer.span().end);
        let slice = lexer.slice();

        let tok = match result {
            Ok(raw) => match raw {
                RawToken::Comment => Token::Comment(slice.to_string()),
                RawToken::Integer(n) => Token::Integer(n),
                RawToken::Double(n) => Token::Double(n),
                RawToken::NegInteger(n) => Token::Integer(n),
                RawToken::NegDouble(n) => Token::Double(n),
                RawToken::String(s) => Token::String(s),
                RawToken::Quote(QuoteResult::Char(c)) => Token::Char(c),
                RawToken::Quote(QuoteResult::Tick) => Token::Tick,
                RawToken::LowerIdent => resolve_lower(slice),
                RawToken::UpperIdent => resolve_upper(slice),
                RawToken::FatArrow => Token::FatArrow,
                RawToken::ThinArrow => Token::ThinArrow,
                RawToken::QBrace => Token::QBrace,
                RawToken::Unit => Token::Unit,
                RawToken::DotDotDot => Token::DotDotDot,
                RawToken::PipeRight => Token::PipeRight,
                RawToken::EqEq => Token::EqEq,
                RawToken::BangEq => Token::BangEq,
                RawToken::LAngleEq => Token::LAngleEq,
                RawToken::RAngleEq => Token::RAngleEq,
                RawToken::AmpAmp => Token::AmpAmp,
                RawToken::PipePipe => Token::PipePipe,
                RawToken::Bang => Token::Bang,
                RawToken::Backslash => Token::Backslash,
                RawToken::Colon => Token::Colon,
                RawToken::Equals => Token::Equals,
                RawToken::Dot => Token::Dot,
                RawToken::Comma => Token::Comma,
                RawToken::Semicolon => Token::Semicolon,
                RawToken::Pipe => Token::Pipe,
                RawToken::Question => Token::Question,
                RawToken::LParen => Token::LParen,
                RawToken::RParen => Token::RParen,
                RawToken::LBrace => Token::LBrace,
                RawToken::RBrace => Token::RBrace,
                RawToken::LBracket => Token::LBracket,
                RawToken::RBracket => Token::RBracket,
                RawToken::LAngle => Token::LAngle,
                RawToken::RAngle => Token::RAngle,
                RawToken::Newline => Token::Newline,
            },
            Err(()) => {
                return Err(LexError {
                    message: format!("unexpected character: '{}'", slice.chars().next().unwrap_or('?')),
                    span,
                });
            }
        };

        tokens.push((tok, span));
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_ok(input: &str) -> Vec<Token> {
        lex(input)
            .unwrap()
            .into_iter()
            .map(|(tok, _)| tok)
            .filter(|t| !matches!(t, Token::Newline | Token::Comment(_)))
            .collect()
    }

    #[test]
    fn test_integer() {
        assert_eq!(lex_ok("42"), vec![Token::Integer(42)]);
    }

    #[test]
    fn test_float() {
        assert_eq!(lex_ok("3.14"), vec![Token::Double(3.14)]);
    }

    #[test]
    fn test_string() {
        assert_eq!(lex_ok("\"hello\""), vec![Token::String("hello".into())]);
    }

    #[test]
    fn test_char() {
        assert_eq!(lex_ok("'x'"), vec![Token::Char('x')]);
    }

    #[test]
    fn test_keywords() {
        assert_eq!(
            lex_ok("let in case of if then else"),
            vec![Token::Let, Token::In, Token::Case, Token::Of, Token::If, Token::Then, Token::Else]
        );
    }

    #[test]
    fn test_type_keywords() {
        assert_eq!(
            lex_ok("Type String Integer Rec List Array Lazy"),
            vec![
                Token::KwType, Token::KwString, Token::KwInteger,
                Token::KwRec, Token::KwList, Token::KwArray, Token::KwLazy,
            ]
        );
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(
            lex_ok("foo Bar baz'"),
            vec![
                Token::Lower("foo".into()),
                Token::Upper("Bar".into()),
                Token::Lower("baz'".into()),
            ]
        );
    }

    #[test]
    fn test_symbols() {
        assert_eq!(
            lex_ok("\\ => -> : = . , ; |"),
            vec![
                Token::Backslash, Token::FatArrow, Token::ThinArrow,
                Token::Colon, Token::Equals, Token::Dot,
                Token::Comma, Token::Semicolon, Token::Pipe,
            ]
        );
    }

    #[test]
    fn test_brackets() {
        assert_eq!(
            lex_ok("( ) { } [ ] < >"),
            vec![
                Token::LParen, Token::RParen, Token::LBrace, Token::RBrace,
                Token::LBracket, Token::RBracket, Token::LAngle, Token::RAngle,
            ]
        );
    }

    #[test]
    fn test_unit() {
        assert_eq!(lex_ok("()"), vec![Token::Unit]);
    }

    #[test]
    fn test_qbrace() {
        assert_eq!(
            lex_ok("?{x}"),
            vec![Token::QBrace, Token::Lower("x".into()), Token::RBrace]
        );
    }

    #[test]
    fn test_comment() {
        assert_eq!(
            lex_ok("foo -- this is a comment"),
            vec![Token::Lower("foo".into())]
        );
    }

    #[test]
    fn test_booleans() {
        assert_eq!(lex_ok("True False"), vec![Token::True, Token::False]);
    }

    #[test]
    fn test_escape_sequences() {
        assert_eq!(
            lex_ok("\"hello\\nworld\""),
            vec![Token::String("hello\nworld".into())]
        );
    }

    #[test]
    fn test_variant_tick() {
        assert_eq!(
            lex_ok("'Just"),
            vec![Token::Tick, Token::Upper("Just".into())]
        );
    }

    #[test]
    fn test_negative_number() {
        assert_eq!(lex_ok("-42"), vec![Token::Integer(-42)]);
    }

    #[test]
    fn test_arrow_not_negative() {
        assert_eq!(lex_ok("->"), vec![Token::ThinArrow]);
    }

    #[test]
    fn test_newlines_preserved() {
        let tokens: Vec<Token> = lex("foo\nbar").unwrap().into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            tokens,
            vec![Token::Lower("foo".into()), Token::Newline, Token::Lower("bar".into())]
        );
    }
}
