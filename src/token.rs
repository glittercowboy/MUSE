use logos::Logos;

/// All tokens produced by the Muse lexer.
///
/// Logos drives tokenization via the `#[token]` and `#[regex]` attributes.
/// Keywords have priority over identifiers because `#[token]` matches are
/// checked before `#[regex]` patterns.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
pub enum Token {
    // ── Keywords ──────────────────────────────────────────────
    #[token("plugin")]
    Plugin,
    #[token("param")]
    Param,
    #[token("process")]
    Process,
    #[token("input")]
    Input,
    #[token("output")]
    Output,
    #[token("clap")]
    Clap,
    #[token("vst3")]
    Vst3,
    #[token("midi")]
    Midi,
    #[token("note")]
    Note,
    #[token("cc")]
    Cc,
    #[token("vendor")]
    Vendor,
    #[token("version")]
    Version,
    #[token("url")]
    Url,
    #[token("email")]
    Email,
    #[token("category")]
    Category,
    #[token("mono")]
    Mono,
    #[token("stereo")]
    Stereo,
    #[token("in")]
    In,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("let")]
    Let,
    #[token("return")]
    Return,
    #[token("true")]
    True,
    #[token("false")]
    False,

    // ── Format-block keywords ────────────────────────────────
    #[token("id")]
    Id,
    #[token("description")]
    Description,
    #[token("features")]
    Features,
    #[token("subcategories")]
    Subcategories,

    // ── Param-body keywords ──────────────────────────────────
    #[token("smoothing")]
    Smoothing,
    #[token("linear")]
    Linear,
    #[token("logarithmic")]
    Logarithmic,
    #[token("exponential")]
    Exponential,
    #[token("display")]
    Display,
    #[token("unit")]
    Unit,

    // ── Category values ──────────────────────────────────────
    #[token("effect")]
    Effect,
    #[token("instrument")]
    Instrument,
    #[token("analyzer")]
    Analyzer,
    #[token("utility")]
    Utility,

    // ── Reserved keywords ────────────────────────────────────
    #[token("voice")]
    Voice,
    #[token("poly")]
    Poly,
    #[token("sample")]
    Sample,
    #[token("import")]
    Import,
    #[token("test")]
    Test,
    #[token("feedback")]
    Feedback,
    #[token("split")]
    Split,
    #[token("merge")]
    Merge,
    #[token("bus")]
    Bus,

    // ── Type keywords ────────────────────────────────────────
    #[token("float")]
    Float,
    #[token("int")]
    Int,
    #[token("bool")]
    Bool,
    #[token("enum")]
    Enum,

    // ── Unit suffixes ────────────────────────────────────────
    // These are lexed as separate tokens. The parser binds them
    // to the preceding number literal.
    #[token("Hz")]
    UnitHz,
    #[token("kHz")]
    UnitKHz,
    #[token("ms")]
    UnitMs,
    #[token("s")]
    UnitS,
    #[token("dB")]
    UnitDB,
    #[token("st")]
    UnitSt,
    // Note: "%" is already the modulo operator. The parser
    // disambiguates based on context (after a number literal = unit).

    // ── Operators ────────────────────────────────────────────
    #[token("->")]
    Arrow,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,
    #[token("..")]
    DotDot,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("!")]
    Bang,
    #[token(".")]
    Dot,

    // ── Delimiters ───────────────────────────────────────────
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,

    // ── Literals ─────────────────────────────────────────────
    /// A number literal: integer or float (e.g. `42`, `3.14`, `440.0`).
    /// Negative numbers are handled by the parser as unary minus + number.
    #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().to_string())]
    Number(String),

    /// A double-quoted string literal with escape support.
    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        // Strip surrounding quotes
        s[1..s.len()-1].to_string()
    })]
    StringLiteral(String),

    // ── Identifiers ──────────────────────────────────────────
    /// An identifier: starts with a letter or underscore, followed by
    /// alphanumeric characters or underscores.
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 1)]
    Ident(String),
}

/// A single lexer result: either a (Token, byte-range) pair or an error span.
pub type LexResult = Result<(Token, std::ops::Range<usize>), std::ops::Range<usize>>;

/// Lex the source text into a sequence of (Token, byte-range) pairs.
///
/// Comments (line `//` and block `/* */`) are stripped before tokenization.
/// Error tokens from unrecognized input are included as `Err(())` in the
/// result so callers can report diagnostics.
pub fn lex(source: &str) -> Vec<LexResult> {
    // Strip comments before lexing. We do this in a pre-pass to keep the
    // logos grammar simple (logos doesn't support nested block comments well).
    let cleaned = strip_comments(source);

    let lexer = Token::lexer(&cleaned);
    lexer
        .spanned()
        .map(|(tok, span)| match tok {
            Ok(t) => Ok((t, span)),
            Err(()) => Err(span),
        })
        .collect()
}

/// Strip line comments (`// ...`) and block comments (`/* ... */`, nested).
///
/// Replaces comment content with spaces to preserve byte offsets for spans.
/// String literals are tracked so that `//` and `/*` inside strings are not
/// treated as comment starts.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut result = vec![b' '; len];
    let mut i = 0;
    let mut depth = 0u32; // block comment nesting depth
    let mut in_string = false;

    while i < len {
        if in_string {
            // Inside a string literal — copy through until unescaped closing quote
            result[i] = bytes[i];
            if bytes[i] == b'\\' && i + 1 < len {
                // Escape sequence — copy both bytes
                result[i + 1] = bytes[i + 1];
                i += 2;
            } else if bytes[i] == b'"' {
                in_string = false;
                i += 1;
            } else {
                i += 1;
            }
        } else if depth > 0 {
            // Inside a block comment
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                depth += 1;
                i += 2;
            } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                depth -= 1;
                i += 2;
            } else {
                // Preserve newlines inside comments for line counting
                if bytes[i] == b'\n' {
                    result[i] = b'\n';
                }
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment — skip to end of line
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            // Keep the newline
            if i < len {
                result[i] = b'\n';
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Start block comment
            depth += 1;
            i += 2;
        } else if bytes[i] == b'"' {
            // Start string literal
            in_string = true;
            result[i] = bytes[i];
            i += 1;
        } else {
            // Normal character — copy through
            result[i] = bytes[i];
            i += 1;
        }
    }

    // SAFETY: we only replaced ASCII bytes with ASCII bytes
    String::from_utf8(result).expect("comment stripping produced invalid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let tokens: Vec<_> = lex("plugin param process input output")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![Token::Plugin, Token::Param, Token::Process, Token::Input, Token::Output]
        );
    }

    #[test]
    fn test_operators() {
        let tokens: Vec<_> = lex("-> == != <= >= && || .. + - * / % = < > ! .")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::Arrow,
                Token::EqEq,
                Token::BangEq,
                Token::LtEq,
                Token::GtEq,
                Token::AmpAmp,
                Token::PipePipe,
                Token::DotDot,
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Eq,
                Token::Lt,
                Token::Gt,
                Token::Bang,
                Token::Dot,
            ]
        );
    }

    #[test]
    fn test_delimiters() {
        let tokens: Vec<_> = lex("{ } ( ) [ ] , : ;")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::LBrace,
                Token::RBrace,
                Token::LParen,
                Token::RParen,
                Token::LBracket,
                Token::RBracket,
                Token::Comma,
                Token::Colon,
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn test_number_literals() {
        let tokens: Vec<_> = lex("42 3.14 0.5 20000.0")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::Number("42".into()),
                Token::Number("3.14".into()),
                Token::Number("0.5".into()),
                Token::Number("20000.0".into()),
            ]
        );
    }

    #[test]
    fn test_string_literals() {
        let tokens: Vec<_> = lex(r#""hello" "world \"escaped\"""#)
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::StringLiteral("hello".into()),
                Token::StringLiteral(r#"world \"escaped\""#.into()),
            ]
        );
    }

    #[test]
    fn test_identifiers() {
        let tokens: Vec<_> = lex("foo bar_baz _x abc123")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::Ident("foo".into()),
                Token::Ident("bar_baz".into()),
                Token::Ident("_x".into()),
                Token::Ident("abc123".into()),
            ]
        );
    }

    #[test]
    fn test_line_comment_stripped() {
        let tokens: Vec<_> = lex("plugin // this is a comment\nparam")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(tokens, vec![Token::Plugin, Token::Param]);
    }

    #[test]
    fn test_block_comment_stripped() {
        let tokens: Vec<_> = lex("plugin /* block */ param")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(tokens, vec![Token::Plugin, Token::Param]);
    }

    #[test]
    fn test_nested_block_comments() {
        let tokens: Vec<_> = lex("plugin /* outer /* inner */ still comment */ param")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(tokens, vec![Token::Plugin, Token::Param]);
    }

    #[test]
    fn test_unit_suffixes() {
        // When a number is followed by a unit suffix with no space,
        // logos will try to match the longest token. Since "50ms" isn't
        // a single token, it will produce Number("50") then UnitMs.
        let tokens: Vec<_> = lex("50 ms 440 Hz")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            tokens,
            vec![
                Token::Number("50".into()),
                Token::UnitMs,
                Token::Number("440".into()),
                Token::UnitHz,
            ]
        );
    }

    #[test]
    fn test_error_on_invalid_input() {
        let results = lex("plugin @ param");
        let error_count = results.iter().filter(|r| r.is_err()).count();
        assert!(error_count > 0, "Expected at least one error token for '@'");
    }

    #[test]
    fn test_booleans() {
        let tokens: Vec<_> = lex("true false")
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(tokens, vec![Token::True, Token::False]);
    }
}
