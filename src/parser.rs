//! Chumsky parser for the Muse audio plugin DSL.
//!
//! Transforms a token stream (from logos) into a typed AST.
//! Uses chumsky 1.0-alpha.8 with `extra::Err<Rich<'_, Token, Span>>`.

use chumsky::input::{Stream, ValueInput};
use chumsky::prelude::*;

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Severity};
use crate::span::Span;
use crate::token::{lex, Token};

/// Type alias for the parser extra (error type).
type ParserExtra<'src> = extra::Err<Rich<'src, Token, Span>>;

// ── Public API ───────────────────────────────────────────────

/// Parse a Muse source string into an AST.
///
/// Returns `(Option<PluginDef>, Vec<Rich<Token, Span>>)`:
/// - A partial or complete AST (if recovery succeeded)
/// - A list of parse errors (may be non-empty even when AST is Some)
pub fn parse(source: &str) -> (Option<PluginDef>, Vec<Rich<'_, Token, Span>>) {
    let len = source.len();
    let lex_results = lex(source);

    let token_iter = lex_results.into_iter().filter_map(|r| match r {
        Ok((tok, span)) => Some((tok, Span::new(span.start, span.end))),
        Err(_) => None,
    });

    let stream =
        Stream::from_iter(token_iter).map((len..len).into(), |(t, s): (_, _)| (t, s));

    let (ast, errs) = plugin_parser().parse(stream).into_output_errors();
    (ast, errs)
}

/// Parse a Muse source string, returning an optional AST and structured diagnostics.
///
/// This is the primary public API for tooling — converts chumsky's internal error
/// representation into `Diagnostic` structs with error codes, messages, and suggestions.
pub fn parse_to_diagnostics(source: &str) -> (Option<PluginDef>, Vec<Diagnostic>) {
    let (ast, errors) = parse(source);
    let diagnostics = errors
        .into_iter()
        .map(|err| rich_error_to_diagnostic(&err))
        .collect();
    (ast, diagnostics)
}

/// Convert a chumsky `Rich` error into a structured `Diagnostic`.
fn rich_error_to_diagnostic(err: &Rich<'_, Token, Span>) -> Diagnostic {
    let span = *err.span();

    // Extract what the parser expected vs what it found
    let expected: Vec<String> = err
        .expected()
        .map(|e| match e {
            chumsky::error::RichPattern::Token(t) => {
                let token: &Token = t;
                format!("'{token}'")
            }
            chumsky::error::RichPattern::Label(l) => l.to_string(),
            chumsky::error::RichPattern::EndOfInput => "end of input".to_string(),
            chumsky::error::RichPattern::Identifier(id) => format!("identifier '{id}'"),
            chumsky::error::RichPattern::Any => "any token".to_string(),
            chumsky::error::RichPattern::SomethingElse => "something else".to_string(),
        })
        .collect();

    let found = err.found().map(|t| format!("'{t}'"));

    // Determine error code and build message based on the error pattern
    let (code, message, suggestion) = classify_error(&expected, found.as_deref(), err);

    let mut diag = Diagnostic {
        code,
        span: (span.start, span.end),
        severity: Severity::Error,
        message,
        suggestion: None,
    };

    if let Some(s) = suggestion {
        diag = diag.with_suggestion(s);
    }

    diag
}

/// Classify a parse error into an error code, message, and optional suggestion.
fn classify_error(
    expected: &[String],
    found: Option<&str>,
    _err: &Rich<'_, Token, Span>,
) -> (String, String, Option<String>) {
    // Check for unclosed block (expected '}')
    let expects_rbrace = expected.iter().any(|e| e.contains('}'));
    let expects_rparen = expected.iter().any(|e| e.contains(')'));
    let expects_rbracket = expected.iter().any(|e| e.contains(']'));

    if expects_rbrace && found.is_none() {
        return (
            "E002".to_string(),
            "unclosed block: expected '}'".to_string(),
            Some("add closing brace '}'".to_string()),
        );
    }

    if expects_rparen && found.is_none() {
        return (
            "E002".to_string(),
            "unclosed group: expected ')'".to_string(),
            Some("add closing parenthesis ')'".to_string()),
        );
    }

    if expects_rbracket && found.is_none() {
        return (
            "E002".to_string(),
            "unclosed bracket: expected ']'".to_string(),
            Some("add closing bracket ']'".to_string()),
        );
    }

    // Build the expected list for the message
    let expected_str = if expected.is_empty() {
        "something else".to_string()
    } else if expected.len() == 1 {
        expected[0].clone()
    } else if expected.len() <= 4 {
        let (head, tail) = expected.split_at(expected.len() - 1);
        format!(
            "{}, or {}",
            head.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            tail[0]
        )
    } else {
        // Too many expected tokens — just show the first few
        format!(
            "{}, or one of {} others",
            expected[..3]
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            expected.len() - 3
        )
    };

    let message = match found {
        Some(f) => format!("unexpected token {f}, expected {expected_str}"),
        None => format!("unexpected end of input, expected {expected_str}"),
    };

    // Generate a suggestion for simple cases
    let suggestion = if expected.len() == 1 {
        Some(format!("add {}", expected[0]))
    } else {
        None
    };

    ("E001".to_string(), message, suggestion)
}

// ── Sub-parsers ──────────────────────────────────────────────

/// Parse a number literal with optional unit suffix.
fn number_with_unit<'src, I>() -> impl Parser<'src, I, Spanned<Expr>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let unit_suffix = select! {
        Token::UnitHz => UnitSuffix::Hz,
        Token::UnitKHz => UnitSuffix::KHz,
        Token::UnitMs => UnitSuffix::Ms,
        Token::UnitS => UnitSuffix::S,
        Token::UnitDB => UnitSuffix::DB,
        Token::UnitSt => UnitSuffix::St,
        Token::Percent => UnitSuffix::Percent,
    };

    select! { Token::Number(n) => n }
        .then(unit_suffix.or_not())
        .map_with(|(n, unit), e| {
            let val: f64 = n.parse().unwrap_or(0.0);
            (Expr::Number(val, unit), e.span())
        })
}

/// Parse a string literal.
fn string_lit<'src, I>() -> impl Parser<'src, I, Spanned<Expr>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! { Token::StringLiteral(s) => s }
        .map_with(|s, e| (Expr::StringLit(s), e.span()))
}

/// Parse a boolean literal.
fn bool_lit<'src, I>() -> impl Parser<'src, I, Spanned<Expr>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! {
        Token::True => true,
        Token::False => false,
    }
    .map_with(|b, e| (Expr::Bool(b), e.span()))
}

/// Parse an identifier, including keywords that can appear in expression position.
fn ident_expr<'src, I>() -> impl Parser<'src, I, Spanned<Expr>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! {
        Token::Ident(s) => s,
        // These keywords appear as identifiers in process blocks / expressions
        Token::Input => "input".to_string(),
        Token::Output => "output".to_string(),
        Token::Param => "param".to_string(),
        Token::Note => "note".to_string(),
        Token::Cc => "cc".to_string(),
        Token::Sample => "sample".to_string(),
        // Built-in function names that are also keywords
        Token::Midi => "midi".to_string(),
    }
    .map_with(|name, e| (Expr::Ident(name), e.span()))
}

/// Parse a bare identifier name (for declarations, not expression position).
fn ident_name<'src, I>() -> impl Parser<'src, I, String, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! { Token::Ident(s) => s }
}

/// Parse an identifier-like token including keywords that can be used as
/// feature/subcategory names in format blocks.
fn feature_ident<'src, I>() -> impl Parser<'src, I, String, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! {
        Token::Ident(s) => s,
        Token::Stereo => "stereo".to_string(),
        Token::Mono => "mono".to_string(),
        Token::Effect => "effect".to_string(),
        Token::Instrument => "instrument".to_string(),
        Token::Analyzer => "analyzer".to_string(),
        Token::Utility => "utility".to_string(),
        Token::Input => "input".to_string(),
        Token::Output => "output".to_string(),
        Token::Unit => "unit".to_string(),
        Token::Display => "display".to_string(),
        Token::Sample => "sample".to_string(),
        Token::Test => "test".to_string(),
        Token::Split => "split".to_string(),
        Token::Merge => "merge".to_string(),
        Token::Bus => "bus".to_string(),
        Token::Linear => "linear".to_string(),
        Token::Float => "float".to_string(),
        Token::Bool => "bool".to_string(),
        Token::Int => "int".to_string(),
    }
}

/// Expression parser — the heart of the language.
///
/// Precedence levels (lowest to highest):
/// 1. Signal chain: `->`
/// 2. Logical: `&&` `||`
/// 3. Comparison: `==` `!=` `<` `>` `<=` `>=`
/// 4. Additive: `+` `-`
/// 5. Multiplicative: `*` `/` `%`
/// 6. Unary: `-` `!`
/// 7. Postfix: `.field` `(args)`
fn expr_parser<'src, I>() -> impl Parser<'src, I, Spanned<Expr>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    recursive(|expr| {
        // ── Statement parser (used inside blocks) ────────
        let stmt = {
            let let_stmt = just(Token::Let)
                .ignore_then(ident_name())
                .then_ignore(just(Token::Eq))
                .then(expr.clone())
                .map_with(|(name, value), e| (Statement::Let { name, value }, e.span()));

            let return_stmt = just(Token::Return)
                .ignore_then(expr.clone())
                .map_with(|value, e| (Statement::Return(value), e.span()));

            let expr_stmt = expr
                .clone()
                .map_with(|e, extra| (Statement::Expr(e), extra.span()));

            let_stmt.or(return_stmt).or(expr_stmt)
        };

        // ── If expression ────────────────────────────────
        // if cond { stmts; final_expr } else { stmts; final_expr }
        //
        // The grammar says the body is `statement* expression`.
        // But statements can be expressions too, so we parse the body as
        // a list of statements where the last one is treated as the final expression.
        let if_block_body = stmt
            .clone()
            .repeated()
            .collect::<Vec<Spanned<Statement>>>();

        let if_expr = recursive(|if_| {
            just(Token::If)
                .ignore_then(expr.clone())
                .then(if_block_body.clone().delimited_by(
                    just(Token::LBrace),
                    just(Token::RBrace),
                ))
                .then(
                    just(Token::Else)
                        .ignore_then(
                            // else { ... }
                            if_block_body
                                .clone()
                                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                                .map(ElseBranch::Block)
                                // else if ...
                                .or(if_.map(ElseBranch::IfExpr)),
                        )
                        .or_not(),
                )
                .map_with(|((cond, then_stmts), else_part), e| {
                    let (then_body, then_final) = split_block_stmts(then_stmts, e.span());
                    let else_body = else_part.map(|branch| match branch {
                        ElseBranch::Block(stmts) => {
                            let (body, final_expr) = split_block_stmts(stmts, e.span());
                            (body, Box::new(final_expr))
                        }
                        ElseBranch::IfExpr(if_spanned) => {
                            (Vec::new(), Box::new(if_spanned))
                        }
                    });
                    (
                        Expr::If {
                            condition: Box::new(cond),
                            then_body,
                            then_expr: Box::new(then_final),
                            else_body,
                        },
                        e.span(),
                    )
                })
        });

        // ── Signal routing atoms ─────────────────────────
        // merge — zero-argument keyword producing Expr::Merge
        let merge_expr = just(Token::Merge)
            .map_with(|_, e| (Expr::Merge, e.span()));

        // split { branch1_chain  branch2_chain  ... }
        // Body is parsed as stmt.repeated(); each top-level Statement::Expr
        // becomes one branch (a Vec of one statement). Let bindings stay
        // attached to the branch they precede.
        let split_expr = just(Token::Split)
            .ignore_then(
                stmt.clone()
                    .repeated()
                    .collect::<Vec<Spanned<Statement>>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map_with(|stmts, e| {
                // Partition statements into branches: each expression-statement
                // starts a new branch. Let statements accumulate into the
                // current branch.
                let mut branches: Vec<Vec<Spanned<Statement>>> = Vec::new();
                for s in stmts {
                    match &s.0 {
                        Statement::Expr(_) => {
                            // Each expression-statement is its own branch
                            branches.push(vec![s]);
                        }
                        Statement::Let { .. } | Statement::Assign { .. } => {
                            // Attach let/assign to the next branch (or start one)
                            if branches.is_empty() || {
                                let last = branches.last().unwrap();
                                matches!(last.last().map(|s| &s.0), Some(Statement::Expr(_)))
                            } {
                                branches.push(vec![s]);
                            } else {
                                branches.last_mut().unwrap().push(s);
                            }
                        }
                        Statement::Return(_) => {
                            // Treat return as expression-like — own branch
                            branches.push(vec![s]);
                        }
                    }
                }
                (Expr::Split { branches }, e.span())
            });

        // feedback { body }
        // Same parse shape as a process block body.
        let feedback_expr = just(Token::Feedback)
            .ignore_then(
                stmt.clone()
                    .repeated()
                    .collect::<Vec<Spanned<Statement>>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map_with(|body, e| (Expr::Feedback { body }, e.span()));

        // ── Primary expressions (atoms) ──────────────────
        let atom = number_with_unit()
            .or(string_lit())
            .or(bool_lit())
            .or(merge_expr)
            .or(split_expr)
            .or(feedback_expr)
            .or(ident_expr())
            .or(if_expr)
            .or(expr
                .clone()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .map_with(|inner, e| (Expr::Grouped(Box::new(inner)), e.span())))
            .recover_with(via_parser(nested_delimiters(
                Token::LParen,
                Token::RParen,
                [
                    (Token::LBracket, Token::RBracket),
                    (Token::LBrace, Token::RBrace),
                ],
                |span| (Expr::Error, span),
            )))
            .boxed();

        // ── Postfix: field access `.field` and function call `(args)` ──
        let args = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>();

        let postfix = atom.foldl_with(
            choice((
                args.delimited_by(just(Token::LParen), just(Token::RParen))
                    .map(PostfixOp::Call),
                just(Token::Dot)
                    .ignore_then(select! {
                        Token::Ident(s) => s,
                        Token::Input => "input".to_string(),
                        Token::Output => "output".to_string(),
                        Token::Param => "param".to_string(),
                        Token::Note => "note".to_string(),
                    })
                    .map(PostfixOp::Field),
            ))
            .repeated(),
            |lhs, op, e| match op {
                PostfixOp::Call(a) => (
                    Expr::FnCall {
                        callee: Box::new(lhs),
                        args: a,
                    },
                    e.span(),
                ),
                PostfixOp::Field(name) => (Expr::FieldAccess(Box::new(lhs), name), e.span()),
            },
        );

        // ── Unary: `-x`, `!x` ───────────────────────────
        let unary = choice((
            just(Token::Minus).to(UnaryOp::Neg),
            just(Token::Bang).to(UnaryOp::Not),
        ))
        .repeated()
        .foldr_with(postfix, |op, val, e| {
            (
                Expr::Unary {
                    op,
                    operand: Box::new(val),
                },
                e.span(),
            )
        });

        // ── Multiplicative: `*`, `/`, `%` ────────────────
        let mul_op = choice((
            just(Token::Star).to(BinOp::Mul),
            just(Token::Slash).to(BinOp::Div),
            just(Token::Percent).to(BinOp::Mod),
        ));
        let multiplicative =
            unary
                .clone()
                .foldl_with(mul_op.then(unary).repeated(), |a, (op, b), e| {
                    (
                        Expr::Binary {
                            left: Box::new(a),
                            op,
                            right: Box::new(b),
                        },
                        e.span(),
                    )
                });

        // ── Additive: `+`, `-` ───────────────────────────
        let add_op = choice((
            just(Token::Plus).to(BinOp::Add),
            just(Token::Minus).to(BinOp::Sub),
        ));
        let additive = multiplicative.clone().foldl_with(
            add_op.then(multiplicative).repeated(),
            |a, (op, b), e| {
                (
                    Expr::Binary {
                        left: Box::new(a),
                        op,
                        right: Box::new(b),
                    },
                    e.span(),
                )
            },
        );

        // ── Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=` ─
        let cmp_op = choice((
            just(Token::EqEq).to(BinOp::Eq),
            just(Token::BangEq).to(BinOp::NotEq),
            just(Token::LtEq).to(BinOp::LtEq),
            just(Token::GtEq).to(BinOp::GtEq),
            just(Token::Lt).to(BinOp::Lt),
            just(Token::Gt).to(BinOp::Gt),
        ));
        let comparison =
            additive
                .clone()
                .foldl_with(cmp_op.then(additive).repeated(), |a, (op, b), e| {
                    (
                        Expr::Binary {
                            left: Box::new(a),
                            op,
                            right: Box::new(b),
                        },
                        e.span(),
                    )
                });

        // ── Logical: `&&`, `||` ──────────────────────────
        let log_op = choice((
            just(Token::AmpAmp).to(BinOp::And),
            just(Token::PipePipe).to(BinOp::Or),
        ));
        let logical =
            comparison
                .clone()
                .foldl_with(log_op.then(comparison).repeated(), |a, (op, b), e| {
                    (
                        Expr::Binary {
                            left: Box::new(a),
                            op,
                            right: Box::new(b),
                        },
                        e.span(),
                    )
                });

        // ── Signal chain: `->` (lowest precedence) ───────
        logical.clone().foldl_with(
            just(Token::Arrow).then(logical).repeated(),
            |a, (_, b), e| {
                (
                    Expr::Binary {
                        left: Box::new(a),
                        op: BinOp::Chain,
                        right: Box::new(b),
                    },
                    e.span(),
                )
            },
        )
    })
}

/// Helper: split a list of statements into (body_stmts, final_expr).
/// The last statement becomes the final expression. If empty, produces an Error node.
fn split_block_stmts(mut stmts: Vec<Spanned<Statement>>, fallback_span: Span) -> (Vec<Spanned<Statement>>, Spanned<Expr>) {
    if let Some(last) = stmts.pop() {
        let final_expr = match last.0 {
            Statement::Expr(e) => e,
            Statement::Return(e) => e,
            other => {
                // A let/assign as the last statement — wrap it back as a statement
                // and use an implicit unit-like value
                let span = last.1;
                stmts.push((other, span));
                (Expr::Ident("()".to_string()), span)
            }
        };
        (stmts, final_expr)
    } else {
        (Vec::new(), (Expr::Error, fallback_span))
    }
}

/// Helper enum for else branches during parsing (not part of public AST).
#[derive(Debug, Clone)]
enum ElseBranch {
    Block(Vec<Spanned<Statement>>),
    IfExpr(Spanned<Expr>),
}

/// Helper enum for postfix operations (not part of public AST).
#[derive(Debug, Clone)]
enum PostfixOp {
    Call(Vec<Spanned<Expr>>),
    Field(String),
}

// ── Metadata parsers ─────────────────────────────────────────

fn metadata_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let string_metadata = select! {
        Token::Vendor => MetadataKey::Vendor,
        Token::Version => MetadataKey::Version,
        Token::Url => MetadataKey::Url,
        Token::Email => MetadataKey::Email,
    }
    .then(select! { Token::StringLiteral(s) => s })
    .map_with(|(key, value), e| {
        (
            PluginItem::Metadata(MetadataField {
                key,
                value: MetadataValue::StringVal(value),
                span: e.span(),
            }),
            e.span(),
        )
    });

    let category_metadata = just(Token::Category)
        .ignore_then(select! {
            Token::Effect => "effect".to_string(),
            Token::Instrument => "instrument".to_string(),
            Token::Analyzer => "analyzer".to_string(),
            Token::Utility => "utility".to_string(),
            Token::Ident(s) => s,
        })
        .map_with(|value, e| {
            (
                PluginItem::Metadata(MetadataField {
                    key: MetadataKey::Category,
                    value: MetadataValue::Identifier(value),
                    span: e.span(),
                }),
                e.span(),
            )
        });

    string_metadata.or(category_metadata)
}

// ── Format block parsers ─────────────────────────────────────

fn clap_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let clap_id = just(Token::Id)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|s, e| (ClapItem::Id(s), e.span()));

    let clap_desc = just(Token::Description)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|s, e| (ClapItem::Description(s), e.span()));

    let clap_features = just(Token::Features)
        .ignore_then(
            feature_ident()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBracket), just(Token::RBracket)),
        )
        .map_with(|features, e| (ClapItem::Features(features), e.span()));

    let clap_item = clap_id.or(clap_desc).or(clap_features);

    just(Token::Clap)
        .ignore_then(
            clap_item
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|items, e| {
            (
                PluginItem::FormatBlock(FormatBlock::Clap(ClapBlock {
                    items,
                    span: e.span(),
                })),
                e.span(),
            )
        })
}

fn vst3_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let vst3_id = just(Token::Id)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|s, e| (Vst3Item::Id(s), e.span()));

    let vst3_subcats = just(Token::Subcategories)
        .ignore_then(
            feature_ident()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBracket), just(Token::RBracket)),
        )
        .map_with(|cats, e| (Vst3Item::Subcategories(cats), e.span()));

    let vst3_item = vst3_id.or(vst3_subcats);

    just(Token::Vst3)
        .ignore_then(
            vst3_item
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|items, e| {
            (
                PluginItem::FormatBlock(FormatBlock::Vst3(Vst3Block {
                    items,
                    span: e.span(),
                })),
                e.span(),
            )
        })
}

// ── I/O declaration parser ───────────────────────────────────

fn io_decl_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let direction = select! {
        Token::Input => IoDirection::Input,
        Token::Output => IoDirection::Output,
    };

    let channel_spec = choice((
        just(Token::Mono).to(ChannelSpec::Mono),
        just(Token::Stereo).to(ChannelSpec::Stereo),
        select! { Token::Number(n) => n }.map(|n| ChannelSpec::Count(n.parse::<u32>().unwrap_or(2))),
    ));

    direction
        .then(channel_spec)
        .map_with(|(direction, channels), e| {
            (
                PluginItem::IoDecl(IoDecl {
                    direction,
                    channels,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── MIDI declaration parser ──────────────────────────────────

fn midi_decl_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let expr = expr_parser();

    let stmt = {
        let let_stmt = just(Token::Let)
            .ignore_then(ident_name())
            .then_ignore(just(Token::Eq))
            .then(expr.clone())
            .map_with(|(name, value), e| (Statement::Let { name, value }, e.span()));

        let return_stmt = just(Token::Return)
            .ignore_then(expr.clone())
            .map_with(|value, e| (Statement::Return(value), e.span()));

        let expr_stmt = expr
            .clone()
            .map_with(|e, extra| (Statement::Expr(e), extra.span()));

        let_stmt.or(return_stmt).or(expr_stmt)
    };

    let note_handler = just(Token::Note)
        .ignore_then(
            stmt.clone()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|body, e| (MidiItem::NoteHandler(body), e.span()));

    let cc_handler = just(Token::Cc)
        .ignore_then(select! { Token::Number(n) => n })
        .then(
            stmt.repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(cc_num, body), e| {
            (
                MidiItem::CcHandler {
                    cc_number: cc_num.parse::<u32>().unwrap_or(0),
                    body,
                },
                e.span(),
            )
        });

    let midi_item = note_handler.or(cc_handler);

    just(Token::Midi)
        .ignore_then(
            midi_item
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|items, e| {
            (
                PluginItem::MidiDecl(MidiDecl {
                    items,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Parameter declaration parser ─────────────────────────────

fn param_decl_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let expr = expr_parser();

    let enum_variants = just(Token::Enum).ignore_then(
        ident_name()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket)),
    );

    let param_type = choice((
        just(Token::Float).to(ParamType::Float),
        just(Token::Int).to(ParamType::Int),
        just(Token::Bool).to(ParamType::Bool),
        enum_variants.map(ParamType::Enum),
    ));

    let default_val = just(Token::Eq).ignore_then(expr.clone());

    let range = just(Token::In)
        .ignore_then(expr.clone())
        .then_ignore(just(Token::DotDot))
        .then(expr.clone())
        .map_with(|(min, max), e| ParamRange {
            min,
            max,
            span: e.span(),
        });

    let smoothing_kind = select! {
        Token::Linear => SmoothingKind::Linear,
        Token::Logarithmic => SmoothingKind::Logarithmic,
        Token::Exponential => SmoothingKind::Exponential,
    };
    let smoothing_opt = just(Token::Smoothing)
        .ignore_then(smoothing_kind)
        .then(expr)
        .map_with(|(kind, value), e| (ParamOption::Smoothing { kind, value }, e.span()));

    let display_opt = just(Token::Display)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|s, e| (ParamOption::Display(s), e.span()));

    let unit_opt = just(Token::Unit)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|s, e| (ParamOption::Unit(s), e.span()));

    let param_option = smoothing_opt.or(display_opt).or(unit_opt);

    let param_body = param_option
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace));

    just(Token::Param)
        .ignore_then(ident_name())
        .then_ignore(just(Token::Colon))
        .then(param_type)
        .then(default_val.or_not())
        .then(range.or_not())
        .then(param_body.or_not())
        .map_with(|((((name, param_type), default), range), options), e| {
            (
                PluginItem::ParamDecl(Box::new(ParamDef {
                    name,
                    param_type,
                    default,
                    range,
                    options: options.unwrap_or_default(),
                    span: e.span(),
                })),
                e.span(),
            )
        })
}

// ── Process block parser ─────────────────────────────────────

fn process_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let expr = expr_parser();
    let stmt = {
        let let_stmt = just(Token::Let)
            .ignore_then(ident_name())
            .then_ignore(just(Token::Eq))
            .then(expr.clone())
            .map_with(|(name, value), e| (Statement::Let { name, value }, e.span()));

        let return_stmt = just(Token::Return)
            .ignore_then(expr.clone())
            .map_with(|value, e| (Statement::Return(value), e.span()));

        let expr_stmt = expr
            .map_with(|e, extra| (Statement::Expr(e), extra.span()));

        let_stmt.or(return_stmt).or(expr_stmt)
    };

    just(Token::Process)
        .ignore_then(
            stmt.repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|body, e| {
            (
                PluginItem::ProcessBlock(ProcessBlock {
                    body,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Test block parser ────────────────────────────────────────

fn test_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    // Parse a number literal (possibly negative) with optional unit suffix.
    // Returns the raw f64 value (applying dB conversion is the runtime's job).
    let test_number = just(Token::Minus)
        .or_not()
        .then(select! { Token::Number(n) => n })
        .then(
            select! {
                Token::UnitHz => "Hz",
                Token::UnitDB => "dB",
            }
            .or_not(),
        )
        .map(|((neg, n), _unit)| {
            let val: f64 = n.parse().unwrap_or(0.0);
            if neg.is_some() { -val } else { val }
        });

    // input <signal> <count> samples
    // Signal: silence | sine <freq>Hz | impulse
    let test_signal = choice((
        select! { Token::Ident(s) => s }
            .filter(|s: &String| s == "silence")
            .to(TestSignal::Silence),
        select! { Token::Ident(s) => s }
            .filter(|s: &String| s == "sine")
            .then(select! { Token::Number(n) => n })
            .then_ignore(just(Token::UnitHz))
            .map(|(_, freq_str)| TestSignal::Sine {
                frequency: freq_str.parse().unwrap_or(440.0),
            }),
        select! { Token::Ident(s) => s }
            .filter(|s: &String| s == "impulse")
            .to(TestSignal::Impulse),
    ));

    let input_stmt = just(Token::Input)
        .ignore_then(test_signal)
        .then(select! { Token::Number(n) => n })
        .then_ignore(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "samples"),
        )
        .map_with(|(signal, count_str), e| {
            (
                TestStatement::Input(TestInput {
                    signal,
                    sample_count: count_str.parse().unwrap_or(512),
                }),
                e.span(),
            )
        });

    // set preset "Name"
    let set_preset_stmt = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "set")
        .ignore_then(just(Token::Preset))
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|name, e| {
            (
                TestStatement::SetPreset { name },
                e.span(),
            )
        });

    // set param.<name> = <value>
    let set_stmt = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "set")
        .ignore_then(just(Token::Param))
        .ignore_then(just(Token::Dot))
        .ignore_then(ident_name())
        .then_ignore(just(Token::Eq))
        .then(test_number.clone())
        .map_with(|(param_path, value), e| {
            (
                TestStatement::Set(TestSet { param_path, value }),
                e.span(),
            )
        });

    // assert <property> <op> <value>
    // property: output.rms | output.peak | input.rms | input.peak
    //           output.rms_in N..M | output.peak_in N..M  (temporal range)
    let test_property = choice((
        just(Token::Output)
            .ignore_then(just(Token::Dot))
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                // Optional range for temporal assertions: N..M
                select! { Token::Number(n) => n }
                    .then_ignore(just(Token::DotDot))
                    .then(select! { Token::Number(n) => n })
                    .or_not(),
            )
            .map(|(field, range)| match (field.as_str(), range) {
                ("rms_in", Some((start, end))) => TestProperty::OutputRmsIn(
                    start.parse().unwrap_or(0),
                    end.parse().unwrap_or(0),
                ),
                ("peak_in", Some((start, end))) => TestProperty::OutputPeakIn(
                    start.parse().unwrap_or(0),
                    end.parse().unwrap_or(0),
                ),
                ("rms", _) => TestProperty::OutputRms,
                ("peak", _) => TestProperty::OutputPeak,
                _ => TestProperty::OutputRms, // fallback
            }),
        just(Token::Input)
            .ignore_then(just(Token::Dot))
            .ignore_then(select! { Token::Ident(s) => s })
            .map(|field| match field.as_str() {
                "rms" => TestProperty::InputRms,
                "peak" => TestProperty::InputPeak,
                _ => TestProperty::InputRms, // fallback
            }),
    ));

    let test_op = choice((
        just(Token::TildeEq).to(TestOp::ApproxEqual),
        just(Token::EqEq).to(TestOp::Equal),
        just(Token::Lt).to(TestOp::LessThan),
        just(Token::Gt).to(TestOp::GreaterThan),
    ));

    let assert_stmt = just(Token::Assert)
        .ignore_then(test_property)
        .then(test_op.clone())
        .then(test_number.clone())
        .map_with(|((property, op), value), e| {
            (
                TestStatement::Assert(TestAssert {
                    property,
                    op,
                    value,
                    tolerance: None,
                }),
                e.span(),
            )
        });

    // assert no_nan | assert no_denormal | assert no_inf
    let safety_assert_stmt = just(Token::Assert)
        .ignore_then(
            select! { Token::Ident(s) => s }
                .filter(|s: &String| s == "no_nan" || s == "no_denormal" || s == "no_inf"),
        )
        .map_with(|check_name, e| {
            let check = match check_name.as_str() {
                "no_nan" => SafetyCheck::NoNan,
                "no_denormal" => SafetyCheck::NoDenormal,
                "no_inf" => SafetyCheck::NoInf,
                _ => unreachable!(),
            };
            (TestStatement::SafetyAssert(check), e.span())
        });

    // assert frequency <number>Hz <op> <value>  — FFT magnitude at a frequency bin
    let frequency_assert_stmt = just(Token::Assert)
        .ignore_then(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "frequency"),
        )
        .ignore_then(select! { Token::Number(n) => n })
        .then_ignore(just(Token::UnitHz))
        .then(test_op)
        .then(test_number)
        .map_with(|((freq_str, op), value), e| {
            let freq: f64 = freq_str.parse().unwrap_or(440.0);
            (
                TestStatement::Assert(TestAssert {
                    property: TestProperty::Frequency(freq),
                    op,
                    value,
                    tolerance: None,
                }),
                e.span(),
            )
        });

    // note on <note> <velocity> at <timing>
    let note_on_stmt = just(Token::Note)
        .ignore_then(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "on"),
        )
        .ignore_then(select! { Token::Number(n) => n })
        .then(select! { Token::Number(n) => n })
        .then_ignore(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "at"),
        )
        .then(select! { Token::Number(n) => n })
        .map_with(|((note_str, vel_str), timing_str), e| {
            (
                TestStatement::NoteOn {
                    note: note_str.parse::<u8>().unwrap_or(69),
                    velocity: vel_str.parse::<f64>().unwrap_or(0.8),
                    timing: timing_str.parse::<u64>().unwrap_or(0),
                },
                e.span(),
            )
        });

    // note off <note> at <timing>
    let note_off_stmt = just(Token::Note)
        .ignore_then(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "off"),
        )
        .ignore_then(select! { Token::Number(n) => n })
        .then_ignore(
            select! { Token::Ident(s) => s }.filter(|s: &String| s == "at"),
        )
        .then(select! { Token::Number(n) => n })
        .map_with(|(note_str, timing_str), e| {
            (
                TestStatement::NoteOff {
                    note: note_str.parse::<u8>().unwrap_or(69),
                    timing: timing_str.parse::<u64>().unwrap_or(0),
                },
                e.span(),
            )
        });

    let test_stmt = choice((input_stmt, set_preset_stmt, set_stmt, safety_assert_stmt, frequency_assert_stmt, assert_stmt, note_on_stmt, note_off_stmt));

    just(Token::Test)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .then(
            test_stmt
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(name, statements), e| {
            (
                PluginItem::TestBlock(TestBlock {
                    name,
                    statements,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Voice declaration parser ─────────────────────────────────

fn voice_config_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    just(Token::Voice)
        .ignore_then(select! { Token::Number(n) => n }.filter(|n: &String| !n.contains('.')))
        .map_with(|count, e| {
            (
                PluginItem::VoiceDecl(VoiceConfig {
                    count: count.parse::<u32>().unwrap_or(0),
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Unison block parser ──────────────────────────────────────

fn unison_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    // Parse: unison { count N detune X }
    // "count" and "detune" are parsed as Ident tokens since they aren't keywords.
    let count_field = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "count")
        .ignore_then(select! { Token::Number(n) => n }.filter(|n: &String| !n.contains('.')))
        .map(|n| n.parse::<u32>().unwrap_or(0));

    let detune_field = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "detune")
        .ignore_then(select! { Token::Number(n) => n })
        .map(|n| n.parse::<f64>().unwrap_or(0.0));

    just(Token::Unison)
        .ignore_then(just(Token::LBrace))
        .ignore_then(count_field)
        .then(detune_field)
        .then_ignore(just(Token::RBrace))
        .map_with(|(count, detune_cents), e| {
            (
                PluginItem::UnisonDecl(UnisonConfig {
                    count,
                    detune_cents,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── GUI block parser ─────────────────────────────────────────

fn gui_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    // theme <dark|light> — "theme" is parsed as an Ident (not a keyword)
    let gui_theme = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "theme")
        .ignore_then(select! {
            Token::Ident(s) => s,
        })
        .map_with(|value, e| (GuiItem::Theme(value), e.span()));

    // accent "#RRGGBB" — "accent" is parsed as an Ident
    let gui_accent = select! { Token::Ident(s) => s }
        .filter(|s: &String| s == "accent")
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .map_with(|value, e| (GuiItem::Accent(value), e.span()));

    let gui_item = gui_theme.or(gui_accent);

    just(Token::Gui)
        .ignore_then(
            gui_item
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|items, e| {
            (
                PluginItem::GuiDecl(GuiBlock {
                    items,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Preset block parser ──────────────────────────────────────

fn preset_block_parser<'src, I>(
) -> impl Parser<'src, I, Spanned<PluginItem>, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    // Parse: preset "Name" { param_name = value ... }
    // Param names are Ident tokens; values can be numbers (with optional negation),
    // booleans, or identifiers (for enum params).
    let preset_value = choice((
        // Negative number
        just(Token::Minus)
            .ignore_then(select! { Token::Number(n) => n })
            .map(|n| PresetValue::Number(-n.parse::<f64>().unwrap_or(0.0))),
        // Positive number
        select! { Token::Number(n) => n }
            .map(|n| PresetValue::Number(n.parse::<f64>().unwrap_or(0.0))),
        // Boolean
        select! {
            Token::True => PresetValue::Bool(true),
            Token::False => PresetValue::Bool(false),
        },
        // Identifier (for enum params)
        select! { Token::Ident(s) => PresetValue::Ident(s) },
    ));

    let preset_assignment = ident_name()
        .then_ignore(just(Token::Eq))
        .then(preset_value)
        .map_with(|(param_name, value), e| {
            (
                PresetAssignment { param_name, value },
                e.span(),
            )
        });

    just(Token::Preset)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .then(
            preset_assignment
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(name, assignments), e| {
            (
                PluginItem::PresetDecl(PresetBlock {
                    name,
                    assignments,
                    span: e.span(),
                }),
                e.span(),
            )
        })
}

// ── Top-level plugin parser ──────────────────────────────────

fn plugin_parser<'src, I>() -> impl Parser<'src, I, PluginDef, ParserExtra<'src>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let plugin_item = choice((
        metadata_parser(),
        clap_block_parser(),
        vst3_block_parser(),
        io_decl_parser(),
        param_decl_parser(),
        midi_decl_parser(),
        voice_config_parser(),
        unison_block_parser(),
        preset_block_parser(),
        gui_block_parser(),
        process_block_parser(),
        test_block_parser(),
    ))
    .recover_with(via_parser(nested_delimiters(
        Token::LBrace,
        Token::RBrace,
        [
            (Token::LParen, Token::RParen),
            (Token::LBracket, Token::RBracket),
        ],
        |span| {
            (
                PluginItem::ProcessBlock(ProcessBlock {
                    body: vec![],
                    span,
                }),
                span,
            )
        },
    )));

    just(Token::Plugin)
        .ignore_then(select! { Token::StringLiteral(s) => s })
        .then(
            plugin_item
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(name, items), e| PluginDef {
            name,
            items,
            span: e.span(),
        })
}
