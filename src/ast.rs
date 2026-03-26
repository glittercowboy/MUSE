//! AST types for the Muse audio plugin DSL.
//!
//! Every node that represents a source construct carries a `Span` for error reporting.
//! All nodes derive `Debug, Clone, PartialEq`.

use crate::span::Span;

/// A spanned AST node: `(node, span)`.
pub type Spanned<T> = (T, Span);

// ── Top-level ────────────────────────────────────────────────

/// Root AST node: `plugin "Name" { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginDef {
    pub name: String,
    pub items: Vec<Spanned<PluginItem>>,
    pub span: Span,
}

/// An item inside a plugin body.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginItem {
    Metadata(MetadataField),
    FormatBlock(FormatBlock),
    IoDecl(IoDecl),
    ParamDecl(Box<ParamDef>),
    MidiDecl(MidiDecl),
    VoiceDecl(VoiceConfig),
    UnisonDecl(UnisonConfig),
    ProcessBlock(ProcessBlock),
    TestBlock(TestBlock),
    PresetDecl(PresetBlock),
    GuiDecl(GuiBlock),
    SampleDecl(SampleDecl),
    WavetableDecl(WavetableDecl),
}

// ── Metadata ─────────────────────────────────────────────────

/// `vendor "Muse Audio"`, `version "0.1.0"`, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataField {
    pub key: MetadataKey,
    pub value: MetadataValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetadataKey {
    Vendor,
    Version,
    Url,
    Email,
    Category,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetadataValue {
    StringVal(String),
    Identifier(String),
}

// ── Format blocks ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum FormatBlock {
    Clap(ClapBlock),
    Vst3(Vst3Block),
}

/// `clap { id "..." description "..." features [...] }`
#[derive(Debug, Clone, PartialEq)]
pub struct ClapBlock {
    pub items: Vec<Spanned<ClapItem>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClapItem {
    Id(String),
    Description(String),
    Features(Vec<String>),
}

/// `vst3 { id "..." subcategories [...] }`
#[derive(Debug, Clone, PartialEq)]
pub struct Vst3Block {
    pub items: Vec<Spanned<Vst3Item>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Vst3Item {
    Id(String),
    Subcategories(Vec<String>),
}

// ── I/O ──────────────────────────────────────────────────────

/// `input stereo`, `output mono`, `input 4`, `input sidechain stereo`
#[derive(Debug, Clone, PartialEq)]
pub struct IoDecl {
    pub direction: IoDirection,
    pub name: Option<String>,
    pub channels: ChannelSpec,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IoDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelSpec {
    Mono,
    Stereo,
    Count(u32),
}

// ── MIDI ─────────────────────────────────────────────────────

/// `midi { note { ... } cc 1 { ... } }`
#[derive(Debug, Clone, PartialEq)]
pub struct MidiDecl {
    pub items: Vec<Spanned<MidiItem>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MidiItem {
    NoteHandler(Vec<Spanned<Statement>>),
    CcHandler { cc_number: u32, body: Vec<Spanned<Statement>> },
}

/// `voices 8`
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceConfig {
    pub count: u32,
    pub span: Span,
}

/// `unison { count 3 detune 15 }`
#[derive(Debug, Clone, PartialEq)]
pub struct UnisonConfig {
    pub count: u32,
    pub detune_cents: f64,
    pub span: Span,
}

// ── Parameters ───────────────────────────────────────────────

/// `param gain: float = 0.0 in -30.0..30.0 { smoothing logarithmic 50ms }`
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub param_type: ParamType,
    pub default: Option<Spanned<Expr>>,
    pub range: Option<ParamRange>,
    pub options: Vec<Spanned<ParamOption>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
    Enum(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamRange {
    pub min: Spanned<Expr>,
    pub max: Spanned<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamOption {
    Smoothing {
        kind: SmoothingKind,
        value: Spanned<Expr>,
    },
    Display(String),
    Unit(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SmoothingKind {
    Linear,
    Logarithmic,
    Exponential,
}

// ── Process block ────────────────────────────────────────────

/// `process { ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessBlock {
    pub body: Vec<Spanned<Statement>>,
    pub span: Span,
}

// ── Statements ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        value: Spanned<Expr>,
    },
    Assign {
        target: String,
        value: Spanned<Expr>,
    },
    Return(Spanned<Expr>),
    Expr(Spanned<Expr>),
}

// ── Expressions ──────────────────────────────────────────────

/// The body of an `else` branch: `(statements, final_expression)`.
pub type ElseBody = (Vec<Spanned<Statement>>, Box<Spanned<Expr>>);

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A number literal, optionally with a unit suffix: `440.0`, `50ms`
    Number(f64, Option<UnitSuffix>),
    /// A string literal: `"hello"`
    StringLit(String),
    /// A boolean literal: `true`, `false`
    Bool(bool),
    /// An identifier: `gain`, `input`, `output`
    Ident(String),
    /// Field access: `param.gain`, `note.pitch`
    FieldAccess(Box<Spanned<Expr>>, String),
    /// Function call: `lowpass(cutoff, 0.5)`
    FnCall {
        callee: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },
    /// Binary operation: `a + b`, `a -> b`
    Binary {
        left: Box<Spanned<Expr>>,
        op: BinOp,
        right: Box<Spanned<Expr>>,
    },
    /// Unary operation: `-x`, `!flag`
    Unary {
        op: UnaryOp,
        operand: Box<Spanned<Expr>>,
    },
    /// If expression: `if cond { a } else { b }`
    If {
        condition: Box<Spanned<Expr>>,
        then_body: Vec<Spanned<Statement>>,
        then_expr: Box<Spanned<Expr>>,
        else_body: Option<ElseBody>,
    },
    /// Parenthesized expression: `(a + b)`
    Grouped(Box<Spanned<Expr>>),
    /// Parallel split: `split { branch1; branch2 }`
    /// Each branch is a list of statements (same shape as process block bodies),
    /// enabling chains inside branches.
    Split {
        branches: Vec<Vec<Spanned<Statement>>>,
    },
    /// Merge parallel branches back to a single signal.
    /// Zero-argument keyword expression that sums split branches;
    /// must follow a split in a chain.
    Merge,
    /// Feedback loop: `feedback { body }`
    /// The body receives/produces Signal with an implicit one-sample delay
    /// for real-time safety.
    Feedback {
        body: Vec<Spanned<Statement>>,
    },
    /// Error recovery placeholder
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    // Logical
    And,
    Or,
    // Signal chain
    Chain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitSuffix {
    Hz,
    KHz,
    Ms,
    S,
    DB,
    St,
    Percent,
}

// ── Test blocks ──────────────────────────────────────────────

/// A `test "name" { ... }` block inside a plugin definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TestBlock {
    pub name: String,
    pub statements: Vec<Spanned<TestStatement>>,
    pub span: Span,
}

/// A statement inside a test block.
#[derive(Debug, Clone, PartialEq)]
pub enum TestStatement {
    Input(TestInput),
    Set(TestSet),
    SetPreset { name: String },
    Assert(TestAssert),
    SafetyAssert(SafetyCheck),
    NoteOn {
        note: u8,
        velocity: f64,
        timing: u64,
    },
    NoteOff {
        note: u8,
        timing: u64,
    },
}

/// Safety check variants for `assert no_nan`, `assert no_denormal`, `assert no_inf`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafetyCheck {
    NoNan,
    NoDenormal,
    NoInf,
}

/// `input [bus_name] <signal> <count> samples`
#[derive(Debug, Clone, PartialEq)]
pub struct TestInput {
    pub bus_name: Option<String>,
    pub signal: TestSignal,
    pub sample_count: u64,
}

/// Signal type for test input generation.
#[derive(Debug, Clone, PartialEq)]
pub enum TestSignal {
    Silence,
    Sine { frequency: f64 },
    Impulse,
}

/// `set param.<name> = <value>`
#[derive(Debug, Clone, PartialEq)]
pub struct TestSet {
    pub param_path: String,
    pub value: f64,
}

/// `assert <property> <op> <value>`
#[derive(Debug, Clone, PartialEq)]
pub struct TestAssert {
    pub property: TestProperty,
    pub op: TestOp,
    pub value: f64,
    pub tolerance: Option<f64>,
}

/// Assertable signal properties in test blocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TestProperty {
    OutputRms,
    OutputPeak,
    InputRms,
    InputPeak,
    OutputRmsIn(u64, u64),
    OutputPeakIn(u64, u64),
    /// FFT-based magnitude at a specific frequency: `assert frequency 440Hz > -20dB`
    Frequency(f64),
}

/// Comparison operators for test assertions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TestOp {
    LessThan,
    GreaterThan,
    Equal,
    ApproxEqual,
}

// ── Preset blocks ────────────────────────────────────────────

/// A `preset "Name" { param = value ... }` block inside a plugin definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetBlock {
    pub name: String,
    pub assignments: Vec<Spanned<PresetAssignment>>,
    pub span: Span,
}

/// A single parameter assignment inside a preset block.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetAssignment {
    pub param_name: String,
    pub value: PresetValue,
}

/// A value in a preset assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetValue {
    /// A numeric value: `gain = -6.0`
    Number(f64),
    /// A boolean value: `bypass = true`
    Bool(bool),
    /// An identifier value (for enum params): `mode = lowpass`
    Ident(String),
}

// ── GUI blocks ───────────────────────────────────────────────

/// `sample kick "samples/kick.wav"` — a sample declaration for playback.
/// `sample kick "samples/kick.wav" external` — external mode: loaded at runtime, not embedded.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleDecl {
    pub name: String,
    pub path: String,
    /// If true (default), the sample is embedded via include_bytes!().
    /// If false (`external` keyword), loaded from disk at runtime.
    pub embed: bool,
    pub span: Span,
}

/// `wavetable wt "samples/saw_stack.wav"` — a wavetable declaration for oscillator playback.
/// `wavetable wt "samples/saw_stack.wav" external` — external mode: loaded at runtime, not embedded.
#[derive(Debug, Clone, PartialEq)]
pub struct WavetableDecl {
    pub name: String,
    pub path: String,
    pub frame_size: u32,
    /// If true (default), the wavetable is embedded via include_bytes!().
    /// If false (`external` keyword), loaded from disk at runtime.
    pub embed: bool,
    pub span: Span,
}

/// `gui { theme dark accent "#E8A87C" layout vertical { ... } }`
#[derive(Debug, Clone, PartialEq)]
pub struct GuiBlock {
    pub items: Vec<Spanned<GuiItem>>,
    pub span: Span,
}

/// An item inside a `gui { }` block.
#[derive(Debug, Clone, PartialEq)]
pub enum GuiItem {
    /// `theme dark` or `theme light`
    Theme(String),
    /// `accent "#E8A87C"`
    Accent(String),
    /// `size 700 450` — editor dimensions (width, height)
    Size(u32, u32),
    /// `layout vertical { ... }` — flex container with children
    Layout(LayoutDecl),
    /// `panel "Title" { ... }` — titled section grouping
    Panel(PanelDecl),
    /// `knob gain`, `slider mix`, `label "Output Level"`, etc.
    Widget(WidgetDecl),
    /// `css "..."` — raw CSS string injected into the HTML
    Css(String),
}

/// Direction for layout containers.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutDirection {
    Horizontal,
    Vertical,
    Grid,
}

/// `layout <direction> { ...children... }`
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDecl {
    pub direction: LayoutDirection,
    pub children: Vec<Spanned<GuiItem>>,
    pub span: Span,
}

/// `panel "Title" { ...children... }`
#[derive(Debug, Clone, PartialEq)]
pub struct PanelDecl {
    pub title: String,
    pub children: Vec<Spanned<GuiItem>>,
    pub span: Span,
}

/// Widget type discriminator.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetType {
    Knob,
    Slider,
    Meter,
    Switch,
    Label,
    Value,
    /// XY pad — binds two parameters (X axis and Y axis)
    XyPad,
    /// Live frequency spectrum analyzer (visualization, no param binding)
    Spectrum,
    /// Waveform display (visualization, no param binding)
    Waveform,
    /// Envelope visualizer (visualization, no param binding)
    Envelope,
    /// EQ curve display (visualization, no param binding)
    EqCurve,
    /// Gain reduction meter (visualization, no param binding)
    Reduction,
}

/// Optional property on a widget: `{ style "vintage" class "hero-knob" label "Custom" }`
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetProp {
    /// `style "vintage"`
    Style(String),
    /// `class "hero-knob"`
    Class(String),
    /// `label "Custom Label"` — overrides the default label
    Label(String),
}

/// A widget declaration: `knob gain { style "vintage" }` or `label "Output Level"`
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetDecl {
    pub widget_type: WidgetType,
    /// Parameter name for param-bound widgets (Knob, Slider, Meter, Switch, Value).
    /// For XyPad, this is the X-axis parameter.
    /// `None` for Label and visualization widgets.
    pub param_name: Option<String>,
    /// Y-axis parameter name — only used by XyPad.
    pub param_name_y: Option<String>,
    /// For Label widgets, the static text to display.
    pub label_text: Option<String>,
    /// Optional properties block: `{ style "vintage" class "hero-knob" }`
    pub props: Vec<WidgetProp>,
    pub span: Span,
}
