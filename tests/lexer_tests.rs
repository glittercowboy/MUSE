use muse_lang::token::{lex, Token};

/// Helper: lex a source string and assert no error tokens are produced.
/// Returns the vec of (Token, Range) pairs.
fn lex_ok(source: &str) -> Vec<Token> {
    let results = lex(source);
    let mut tokens = Vec::new();
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok((tok, _span)) => tokens.push(tok.clone()),
            Err(span) => {
                let snippet = &source[span.clone()];
                panic!(
                    "Unexpected error token at index {i}, span {span:?}, text: {snippet:?}"
                );
            }
        }
    }
    tokens
}

// ── Example file tests ───────────────────────────────────────────

#[test]
fn test_lex_gain_example() {
    let source = include_str!("../examples/gain.muse");
    let tokens = lex_ok(source);
    assert!(!tokens.is_empty(), "gain.muse should produce tokens");

    // Verify the file starts with: plugin "Warm Gain" {
    assert_eq!(tokens[0], Token::Plugin);
    assert_eq!(tokens[1], Token::StringLiteral("Warm Gain".into()));
    assert_eq!(tokens[2], Token::LBrace);

    // Verify it contains key structural tokens
    assert!(tokens.contains(&Token::Vendor));
    assert!(tokens.contains(&Token::Version));
    assert!(tokens.contains(&Token::Clap));
    assert!(tokens.contains(&Token::Vst3));
    assert!(tokens.contains(&Token::Param));
    assert!(tokens.contains(&Token::Process));
    assert!(tokens.contains(&Token::Arrow));
}

#[test]
fn test_lex_filter_example() {
    let source = include_str!("../examples/filter.muse");
    let tokens = lex_ok(source);
    assert!(!tokens.is_empty(), "filter.muse should produce tokens");

    // Verify it starts correctly
    assert_eq!(tokens[0], Token::Plugin);
    assert_eq!(tokens[1], Token::StringLiteral("Velvet Filter".into()));
    assert_eq!(tokens[2], Token::LBrace);

    // Contains enum param features
    assert!(tokens.contains(&Token::Enum));
    assert!(tokens.contains(&Token::If));
    assert!(tokens.contains(&Token::Else));
    assert!(tokens.contains(&Token::Let));

    // Contains comparison operators used in process block
    assert!(tokens.contains(&Token::Gt));
}

#[test]
fn test_lex_synth_example() {
    let source = include_str!("../examples/synth.muse");
    let tokens = lex_ok(source);
    assert!(!tokens.is_empty(), "synth.muse should produce tokens");

    // Verify it starts correctly
    assert_eq!(tokens[0], Token::Plugin);
    assert_eq!(tokens[1], Token::StringLiteral("Glass Synth".into()));
    assert_eq!(tokens[2], Token::LBrace);

    // Contains MIDI features
    assert!(tokens.contains(&Token::Midi));
    assert!(tokens.contains(&Token::Note));

    // Contains instrument category
    assert!(tokens.contains(&Token::Instrument));
}

// ── Specific token sequence tests ────────────────────────────────

#[test]
fn test_param_declaration_tokens() {
    let source = r#"param gain: float = 0.0 in -30.0..30.0"#;
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Param,
            Token::Ident("gain".into()),
            Token::Colon,
            Token::Float,
            Token::Eq,
            Token::Number("0.0".into()),
            Token::In,
            Token::Minus,
            Token::Number("30.0".into()),
            Token::DotDot,
            Token::Number("30.0".into()),
        ]
    );
}

#[test]
fn test_signal_chain_tokens() {
    let source = "input -> gain(param.gain) -> output";
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Input,
            Token::Arrow,
            Token::Ident("gain".into()),
            Token::LParen,
            Token::Param,
            Token::Dot,
            Token::Ident("gain".into()),
            Token::RParen,
            Token::Arrow,
            Token::Output,
        ]
    );
}

#[test]
fn test_enum_param_tokens() {
    let source = "param mode: enum [lowpass, highpass, bandpass, notch] = lowpass";
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Param,
            Token::Ident("mode".into()),
            Token::Colon,
            Token::Enum,
            Token::LBracket,
            Token::Ident("lowpass".into()),
            Token::Comma,
            Token::Ident("highpass".into()),
            Token::Comma,
            Token::Ident("bandpass".into()),
            Token::Comma,
            Token::Ident("notch".into()),
            Token::RBracket,
            Token::Eq,
            Token::Ident("lowpass".into()),
        ]
    );
}

#[test]
fn test_smoothing_block_tokens() {
    let source = r#"smoothing logarithmic 50ms
    unit "dB""#;
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Smoothing,
            Token::Logarithmic,
            Token::Number("50".into()),
            Token::UnitMs,
            Token::Unit,
            Token::StringLiteral("dB".into()),
        ]
    );
}

#[test]
fn test_if_expression_tokens() {
    let source = "if param.drive > 0.0 { filtered } else { input }";
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::If,
            Token::Param,
            Token::Dot,
            Token::Ident("drive".into()),
            Token::Gt,
            Token::Number("0.0".into()),
            Token::LBrace,
            Token::Ident("filtered".into()),
            Token::RBrace,
            Token::Else,
            Token::LBrace,
            Token::Input,
            Token::RBrace,
        ]
    );
}

#[test]
fn test_clap_block_tokens() {
    let source = r#"clap {
    id          "dev.museaudio.warm-gain"
    description "A warm, musical gain stage"
    features    [audio_effect, stereo, utility]
  }"#;
    let tokens = lex_ok(source);
    assert_eq!(tokens[0], Token::Clap);
    assert_eq!(tokens[1], Token::LBrace);
    assert_eq!(tokens[2], Token::Id);
    assert_eq!(tokens[3], Token::StringLiteral("dev.museaudio.warm-gain".into()));
    assert_eq!(tokens[4], Token::Description);
    assert!(tokens.contains(&Token::Features));
    assert!(tokens.contains(&Token::LBracket));
    assert!(tokens.contains(&Token::RBracket));
}

#[test]
fn test_midi_block_tokens() {
    let source = "midi { note { let freq = note.pitch } }";
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Midi,
            Token::LBrace,
            Token::Note,
            Token::LBrace,
            Token::Let,
            Token::Ident("freq".into()),
            Token::Eq,
            Token::Note,
            Token::Dot,
            Token::Ident("pitch".into()),
            Token::RBrace,
            Token::RBrace,
        ]
    );
}

#[test]
fn test_number_with_unit_no_space() {
    // Unit suffixes directly after numbers (no space) — this is how they
    // appear in real Muse code: 50ms, 440Hz, etc.
    let source = "50ms 440Hz 0.5s -12dB 2st 3kHz";
    let tokens = lex_ok(source);
    assert_eq!(
        tokens,
        vec![
            Token::Number("50".into()),
            Token::UnitMs,
            Token::Number("440".into()),
            Token::UnitHz,
            Token::Number("0.5".into()),
            Token::UnitS,
            Token::Minus,
            Token::Number("12".into()),
            Token::UnitDB,
            Token::Number("2".into()),
            Token::UnitSt,
            Token::Number("3".into()),
            Token::UnitKHz,
        ]
    );
}

// ── Edge case tests ──────────────────────────────────────────────

#[test]
fn test_error_tokens_for_invalid_input() {
    let results = lex("plugin @ # $ param");
    let errors: Vec<_> = results.iter().filter(|r| r.is_err()).collect();
    assert!(
        !errors.is_empty(),
        "Should produce error tokens for @, #, $"
    );

    // The valid tokens should still be present
    let tokens: Vec<_> = results
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(t, _)| t)
        .collect();
    assert!(tokens.contains(&Token::Plugin));
    assert!(tokens.contains(&Token::Param));
}

#[test]
fn test_empty_input() {
    let tokens = lex_ok("");
    assert!(tokens.is_empty());
}

#[test]
fn test_only_comments() {
    let tokens = lex_ok("// just a comment\n/* block comment */");
    assert!(tokens.is_empty());
}

#[test]
fn test_all_reserved_keywords() {
    let tokens = lex_ok("voices poly sample import test feedback split merge bus");
    assert_eq!(
        tokens,
        vec![
            Token::Voice,
            Token::Poly,
            Token::Sample,
            Token::Import,
            Token::Test,
            Token::Feedback,
            Token::Split,
            Token::Merge,
            Token::Bus,
        ]
    );
}

#[test]
fn test_type_keywords() {
    let tokens = lex_ok("float int bool enum");
    assert_eq!(
        tokens,
        vec![Token::Float, Token::Int, Token::Bool, Token::Enum]
    );
}

#[test]
fn test_category_values() {
    let tokens = lex_ok("effect instrument analyzer utility");
    assert_eq!(
        tokens,
        vec![Token::Effect, Token::Instrument, Token::Analyzer, Token::Utility]
    );
}

#[test]
fn test_spans_are_correct() {
    let source = "plugin param";
    let results = lex(source);
    let (tok1, span1) = results[0].as_ref().unwrap();
    let (tok2, span2) = results[1].as_ref().unwrap();

    assert_eq!(*tok1, Token::Plugin);
    assert_eq!(span1.clone(), 0..6);
    assert_eq!(&source[span1.clone()], "plugin");

    assert_eq!(*tok2, Token::Param);
    assert_eq!(span2.clone(), 7..12);
    assert_eq!(&source[span2.clone()], "param");
}

#[test]
fn test_unison_keyword() {
    let tokens: Vec<_> = lex("unison")
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(t, _)| t)
        .collect();
    assert_eq!(tokens, vec![Token::Unison]);
}

#[test]
fn test_preset_keyword() {
    let tokens: Vec<_> = lex("preset")
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(t, _)| t)
        .collect();
    assert_eq!(tokens, vec![Token::Preset]);
}
