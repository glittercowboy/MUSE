//! Semantic resolution pass for the Muse language.
//!
//! Walks the parsed AST and validates DSP function calls against the registry.
//! Produces structured `Diagnostic`s with error codes E003–E013 for semantic
//! errors and a side-table mapping expression spans to their resolved `DspType`.
//!
//! Routing validation:
//! - E007: split without merge — every split must be paired with merge
//! - E008: merge without preceding split
//! - E009: feedback body must be a signal processing chain
//!
//! Preset validation:
//! - E012: unknown parameter in preset block
//! - E013: type mismatch in preset assignment
//!
//! GUI validation:
//! - E014: invalid gui block values (bad theme, bad accent color, duplicate block)
//!
//! Bus name validation:
//! - E016: duplicate bus name

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::dsp::primitives::DspRegistry;
use crate::span::Span;
use crate::types::{type_from_unit_suffix, DspType};

// ── Public types ─────────────────────────────────────────────

/// The result of resolving a plugin — the original AST plus a type map.
///
/// The `type_map` keys are `(start, end)` byte offsets matching the
/// `Diagnostic.span` format. This side-table approach avoids modifying
/// the existing AST nodes.
#[derive(Debug)]
pub struct ResolvedPlugin<'a> {
    pub plugin: &'a PluginDef,
    pub type_map: HashMap<(usize, usize), DspType>,
}

// ── Public entry point ───────────────────────────────────────

/// Resolve and validate a parsed plugin definition against the DSP registry.
///
/// Returns `Ok(ResolvedPlugin)` when all DSP calls are valid, or
/// `Err(Vec<Diagnostic>)` with E003–E009 diagnostics on failure.
pub fn resolve_plugin<'a>(
    plugin: &'a PluginDef,
    registry: &DspRegistry,
) -> Result<ResolvedPlugin<'a>, Vec<Diagnostic>> {
    let mut resolver = Resolver::new(registry);
    resolver.resolve_plugin(plugin);

    if resolver.diagnostics.is_empty() {
        Ok(ResolvedPlugin {
            plugin,
            type_map: resolver.type_map,
        })
    } else {
        Err(resolver.diagnostics)
    }
}

// ── Resolver ─────────────────────────────────────────────────

struct Resolver<'a> {
    registry: &'a DspRegistry,
    /// param name → ParamType from the plugin's param declarations
    params: HashMap<String, ParamType>,
    /// User-defined function declarations: name → FnDef
    fn_defs: HashMap<String, FnDef>,
    /// Whether the plugin has MIDI note handlers (enables note.X resolution)
    has_midi_note: bool,
    /// Whether the plugin has any MIDI declaration at all (instrument mode)
    has_midi_decl: bool,
    /// Whether a voice declaration was seen already
    seen_voice_decl: bool,
    /// Whether a unison declaration was seen already
    seen_unison_decl: bool,
    /// Whether a gui block was seen already
    seen_gui_decl: bool,
    /// let-binding scope: name → resolved DspType
    scope: HashMap<String, DspType>,
    /// Span → DspType map (the output)
    type_map: HashMap<(usize, usize), DspType>,
    /// Accumulated diagnostics
    diagnostics: Vec<Diagnostic>,
    /// Nesting depth of split blocks in the current chain context.
    /// Incremented when `Expr::Split` is resolved in a chain, decremented
    /// when `Expr::Merge` is encountered. Used to validate E007/E008.
    split_depth: usize,
    /// Sample declarations: name → path
    samples: HashMap<String, String>,
    /// Duplicate sample detection
    seen_samples: HashSet<String>,
    /// Wavetable declarations: name → path
    wavetables: HashMap<String, String>,
    /// Duplicate wavetable detection
    seen_wavetables: HashSet<String>,
    /// Bus names (effective name → direction) for identifier resolution in process blocks.
    /// Named buses resolve as DspType::Signal (e.g. `sidechain` in `sidechain -> gain(0.5)`).
    bus_names: HashMap<String, IoDirection>,
    /// Duplicate bus name detection scoped by direction: (direction, effective_name)
    seen_bus_names: HashSet<(IoDirection, String)>,
}

impl<'a> Resolver<'a> {
    fn new(registry: &'a DspRegistry) -> Self {
        Self {
            registry,
            params: HashMap::new(),
            fn_defs: HashMap::new(),
            has_midi_note: false,
            has_midi_decl: false,
            seen_voice_decl: false,
            seen_unison_decl: false,
            seen_gui_decl: false,
            scope: HashMap::new(),
            type_map: HashMap::new(),
            diagnostics: Vec::new(),
            split_depth: 0,
            samples: HashMap::new(),
            seen_samples: HashSet::new(),
            wavetables: HashMap::new(),
            seen_wavetables: HashSet::new(),
            bus_names: HashMap::new(),
            seen_bus_names: HashSet::new(),
        }
    }

    fn record(&mut self, span: Span, ty: DspType) {
        self.type_map.insert((span.start, span.end), ty);
    }

    // ── Plugin-level ─────────────────────────────────────────

    fn resolve_plugin(&mut self, plugin: &PluginDef) {
        // Phase 1: extract param declarations, detect MIDI note handlers, and register samples
        for (item, _span) in &plugin.items {
            match item {
                PluginItem::ParamDecl(param_def) => {
                    self.params
                        .insert(param_def.name.clone(), param_def.param_type.clone());
                }
                PluginItem::MidiDecl(midi_decl) => {
                    self.has_midi_decl = true;
                    for (midi_item, _) in &midi_decl.items {
                        if matches!(midi_item, MidiItem::NoteHandler(_)) {
                            self.has_midi_note = true;
                        }
                    }
                }
                PluginItem::SampleDecl(decl) => {
                    if self.seen_samples.contains(&decl.name) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E015",
                                decl.span,
                                format!("duplicate sample name '{}'", decl.name),
                            )
                            .with_suggestion("Each sample must have a unique name."),
                        );
                    } else {
                        self.seen_samples.insert(decl.name.clone());
                        self.samples.insert(decl.name.clone(), decl.path.clone());
                    }
                }
                PluginItem::WavetableDecl(decl) => {
                    if self.seen_wavetables.contains(&decl.name) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E015",
                                decl.span,
                                format!("duplicate wavetable name '{}'", decl.name),
                            )
                            .with_suggestion("Each wavetable must have a unique name."),
                        );
                    } else {
                        self.seen_wavetables.insert(decl.name.clone());
                        self.wavetables.insert(decl.name.clone(), decl.path.clone());
                    }
                }
                PluginItem::FnDef(fn_def) => {
                    self.fn_defs.insert(fn_def.name.clone(), fn_def.clone());
                }
                PluginItem::IoDecl(io_decl) => {
                    let reserved = ["input", "output", "param", "note"];
                    let effective_name = io_decl.name.clone().unwrap_or_else(|| "main".to_string());

                    // Reject reserved words as explicit bus names
                    if io_decl.name.is_some() && reserved.contains(&effective_name.as_str()) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E016",
                                io_decl.span,
                                format!(
                                    "'{}' is a reserved word and cannot be used as a bus name",
                                    effective_name
                                ),
                            )
                            .with_suggestion("Choose a different name for this bus (e.g. 'sidechain', 'fx_send')."),
                        );
                    } else {
                        let key = (io_decl.direction.clone(), effective_name.clone());
                        if !self.seen_bus_names.insert(key) {
                            // E016: duplicate bus name within the same direction
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E016",
                                    io_decl.span,
                                    format!("duplicate bus name '{}'", effective_name),
                                )
                                .with_suggestion("Each bus must have a unique name within its direction (input/output)."),
                            );
                        } else {
                            // Register for identifier resolution (only non-main named buses)
                            if io_decl.name.is_some() && effective_name != "main" {
                                self.bus_names.insert(effective_name, io_decl.direction.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Phase 2: validate plugin-level declarations that depend on full plugin context
        for (item, _span) in &plugin.items {
            match item {
                PluginItem::VoiceDecl(voice) => {
                    self.validate_voice_decl(voice);
                }
                PluginItem::UnisonDecl(unison) => {
                    self.validate_unison_decl(unison, plugin);
                }
                PluginItem::GuiDecl(gui) => {
                    self.validate_gui_decl(gui);
                }
                _ => {}
            }
        }

        // Phase 3: resolve MIDI handlers first (they define note bindings)
        for (item, _span) in &plugin.items {
            if let PluginItem::MidiDecl(midi_decl) = item {
                for (midi_item, _) in &midi_decl.items {
                    match midi_item {
                        MidiItem::NoteHandler(stmts) => {
                            self.resolve_statements(stmts);
                        }
                        MidiItem::CcHandler { body, .. } => {
                            self.resolve_statements(body);
                        }
                    }
                }
            }
        }

        // Phase 4: resolve process blocks
        for (item, _span) in &plugin.items {
            if let PluginItem::ProcessBlock(block) = item {
                self.resolve_statements(&block.body);
            }
        }

        // Phase 5: validate preset blocks against declared params
        for (item, _span) in &plugin.items {
            if let PluginItem::PresetDecl(preset) = item {
                self.validate_preset(preset);
            }
        }
    }

    fn validate_voice_decl(&mut self, voice: &VoiceConfig) {
        if self.seen_voice_decl {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    voice.span,
                    "duplicate voices declaration — only one `voices` declaration is allowed",
                )
                .with_suggestion("Remove the extra `voices` declaration from the plugin body."),
            );
            return;
        }
        self.seen_voice_decl = true;

        if !self.has_midi_decl {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    voice.span,
                    "voices declaration requires a midi block — polyphony is only valid for instruments",
                )
                .with_suggestion("Add a midi block or remove the `voices` declaration."),
            );
        }

        if !(1..=128).contains(&voice.count) {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    voice.span,
                    format!("voice count must be between 1 and 128, got {}", voice.count),
                )
                .with_suggestion("Choose a voice count in the range 1..=128."),
            );
        }
    }

    fn validate_unison_decl(&mut self, unison: &UnisonConfig, _plugin: &PluginDef) {
        if self.seen_unison_decl {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    unison.span,
                    "duplicate unison declaration — only one `unison` block is allowed",
                )
                .with_suggestion("Remove the extra `unison` block from the plugin body."),
            );
            return;
        }
        self.seen_unison_decl = true;

        // Unison requires voices declaration
        if !self.seen_voice_decl {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    unison.span,
                    "unison requires a `voices` declaration — unison is only valid for polyphonic instruments",
                )
                .with_suggestion("Add a `voices N` declaration before the `unison` block."),
            );
        }

        if unison.count < 2 {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    unison.span,
                    format!("unison count must be at least 2, got {}", unison.count),
                )
                .with_suggestion("Set unison count to 2 or more."),
            );
        }

        if unison.detune_cents <= 0.0 {
            self.diagnostics.push(
                Diagnostic::error(
                    "E010",
                    unison.span,
                    format!("unison detune must be greater than 0, got {}", unison.detune_cents),
                )
                .with_suggestion("Set detune to a positive value in cents (e.g. 15)."),
            );
        }

        // Note: we intentionally don't warn if voices < unison count here
        // because the diagnostic system treats all diagnostics as errors.
        // The user is responsible for sizing the voice pool appropriately.
    }

    // ── GUI validation ───────────────────────────────────────

    fn validate_gui_decl(&mut self, gui: &GuiBlock) {
        if self.seen_gui_decl {
            self.diagnostics.push(
                Diagnostic::error(
                    "E014",
                    gui.span,
                    "duplicate gui block — only one `gui` block is allowed per plugin",
                )
                .with_suggestion("Remove the extra `gui` block from the plugin body."),
            );
            return;
        }
        self.seen_gui_decl = true;

        self.validate_gui_items(&gui.items);
    }

    /// Recursively validate gui items (layout and panel children are nested).
    fn validate_gui_items(&mut self, items: &[Spanned<GuiItem>]) {
        for (item, span) in items {
            match item {
                GuiItem::Theme(value) => {
                    if value != "dark" && value != "light" {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                *span,
                                format!(
                                    "invalid gui theme '{}' — must be 'dark' or 'light'",
                                    value
                                ),
                            )
                            .with_suggestion("Use `theme dark` or `theme light`."),
                        );
                    }
                }
                GuiItem::Accent(value) => {
                    if !is_valid_hex_color(value) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                *span,
                                format!(
                                    "invalid accent color '{}' — must be a hex color (#RGB or #RRGGBB)",
                                    value
                                ),
                            )
                            .with_suggestion(
                                "Use a hex color string like \"#E8A87C\" or \"#FFF\".",
                            ),
                        );
                    }
                }
                GuiItem::Size(_, _) => {
                    // Size is always valid if it parsed (numbers from parser)
                }
                GuiItem::Css(value) => {
                    if value.trim().is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                *span,
                                "empty css string — `css` must contain CSS rules",
                            )
                            .with_suggestion(
                                "Provide CSS content: `css \".my-class { color: red; }\"`",
                            ),
                        );
                    }
                }
                GuiItem::Layout(layout) => {
                    // Validate direction was recognized (parser accepts any ident,
                    // but defaulted to Vertical for unknown — we still want to error)
                    // We re-check the direction string isn't needed because the parser
                    // already mapped it. But we should validate children recursively.
                    self.validate_gui_items(&layout.children);
                }
                GuiItem::Panel(panel) => {
                    self.validate_gui_items(&panel.children);
                }
                GuiItem::Widget(widget) => {
                    self.validate_widget(widget, *span);
                }
            }
        }
    }

    /// Validate a widget declaration: param bindings, label constraints.
    fn validate_widget(&mut self, widget: &WidgetDecl, span: Span) {
        match widget.widget_type {
            // Param-bound widgets must reference an existing param
            WidgetType::Knob
            | WidgetType::Slider
            | WidgetType::Meter
            | WidgetType::Switch
            | WidgetType::Value => {
                if let Some(ref name) = widget.param_name {
                    if !self.params.contains_key(name) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                span,
                                format!(
                                    "widget references unknown parameter '{}' — no `param {}` declared in this plugin",
                                    name, name
                                ),
                            )
                            .with_suggestion(
                                "Declare the parameter first: `param {} : float = 0.0 in 0.0..1.0`",
                            ),
                        );
                    }
                }
            }
            // XY pad binds two params — both must exist
            WidgetType::XyPad => {
                if let Some(ref name_x) = widget.param_name {
                    if !self.params.contains_key(name_x) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                span,
                                format!(
                                    "xy_pad references unknown X-axis parameter '{}' — no `param {}` declared in this plugin",
                                    name_x, name_x
                                ),
                            )
                            .with_suggestion(
                                "Declare the parameter first: `param {} : float = 0.0 in 0.0..1.0`",
                            ),
                        );
                    }
                }
                if let Some(ref name_y) = widget.param_name_y {
                    if !self.params.contains_key(name_y) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E014",
                                span,
                                format!(
                                    "xy_pad references unknown Y-axis parameter '{}' — no `param {}` declared in this plugin",
                                    name_y, name_y
                                ),
                            )
                            .with_suggestion(
                                "Declare the parameter first: `param {} : float = 0.0 in 0.0..1.0`",
                            ),
                        );
                    }
                }
            }
            // Visualization widgets must NOT have a param binding
            WidgetType::Spectrum
            | WidgetType::Waveform
            | WidgetType::Envelope
            | WidgetType::EqCurve
            | WidgetType::Reduction => {
                if widget.param_name.is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E014",
                            span,
                            format!(
                                "{:?} is a visualization widget and does not take a parameter binding",
                                widget.widget_type
                            ),
                        )
                        .with_suggestion(
                            "Remove the parameter name: just use `spectrum` instead of `spectrum gain`",
                        ),
                    );
                }
            }
            // Label must NOT have a param_name (it has label_text instead)
            WidgetType::Label => {
                // The parser enforces this structurally — label uses StringLiteral,
                // not an ident param name. So param_name is always None here.
            }
        }
    }

    // ── Preset validation ────────────────────────────────────

    fn validate_preset(&mut self, preset: &PresetBlock) {
        for (assignment, span) in &preset.assignments {
            match self.params.get(&assignment.param_name) {
                None => {
                    // E012: unknown param in preset
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E012",
                            *span,
                            format!(
                                "preset '{}': unknown parameter '{}'",
                                preset.name, assignment.param_name
                            ),
                        )
                        .with_suggestion(format!(
                            "Declared parameters: {}",
                            self.params
                                .keys()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
                Some(param_type) => {
                    // E013: type mismatch — validate value type against param type
                    let mismatch = match (&assignment.value, param_type) {
                        (PresetValue::Number(_), ParamType::Float)
                        | (PresetValue::Number(_), ParamType::Int) => false,
                        (PresetValue::Bool(_), ParamType::Bool) => false,
                        (PresetValue::Ident(_), ParamType::Enum(_)) => false,
                        _ => true,
                    };

                    if mismatch {
                        let expected = match param_type {
                            ParamType::Float | ParamType::Int => "a number",
                            ParamType::Bool => "a boolean (true/false)",
                            ParamType::Enum(_) => "an enum variant identifier",
                        };
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E013",
                                *span,
                                format!(
                                    "preset '{}': type mismatch for parameter '{}' — expected {}",
                                    preset.name, assignment.param_name, expected
                                ),
                            )
                            .with_suggestion(format!(
                                "Parameter '{}' is declared as {:?}.",
                                assignment.param_name, param_type
                            )),
                        );
                    }
                }
            }
        }
    }

    // ── Statement resolution ─────────────────────────────────

    fn resolve_statements(&mut self, stmts: &[Spanned<Statement>]) {
        for (stmt, _span) in stmts {
            self.resolve_statement(stmt);
        }
    }

    fn resolve_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let { name, value } => {
                let saved_depth = self.split_depth;
                self.split_depth = 0;
                if let Some(ty) = self.resolve_expr(value) {
                    self.scope.insert(name.clone(), ty);
                }
                if self.split_depth > 0 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E007",
                            value.1,
                            "split without merge — add `-> merge` after the split block",
                        )
                        .with_suggestion(
                            "Every split must be followed by merge in the same chain.",
                        ),
                    );
                }
                self.split_depth = saved_depth;
            }
            Statement::Assign { value, .. } => {
                let saved_depth = self.split_depth;
                self.split_depth = 0;
                self.resolve_expr(value);
                if self.split_depth > 0 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E007",
                            value.1,
                            "split without merge — add `-> merge` after the split block",
                        )
                        .with_suggestion(
                            "Every split must be followed by merge in the same chain.",
                        ),
                    );
                }
                self.split_depth = saved_depth;
            }
            Statement::Return(expr) => {
                let saved_depth = self.split_depth;
                self.split_depth = 0;
                self.resolve_expr(expr);
                if self.split_depth > 0 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E007",
                            expr.1,
                            "split without merge — add `-> merge` after the split block",
                        )
                        .with_suggestion(
                            "Every split must be followed by merge in the same chain.",
                        ),
                    );
                }
                self.split_depth = saved_depth;
            }
            Statement::Expr(expr) => {
                let saved_depth = self.split_depth;
                self.split_depth = 0;
                self.resolve_expr(expr);
                if self.split_depth > 0 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E007",
                            expr.1,
                            "split without merge — add `-> merge` after the split block",
                        )
                        .with_suggestion(
                            "Every split must be followed by merge in the same chain.",
                        ),
                    );
                }
                self.split_depth = saved_depth;
            }
        }
    }

    // ── Expression resolution ────────────────────────────────

    /// Resolve an expression, returning its DspType (or None on error).
    fn resolve_expr(&mut self, spanned_expr: &Spanned<Expr>) -> Option<DspType> {
        let (expr, span) = spanned_expr;
        let ty = self.resolve_expr_inner(expr, *span)?;
        self.record(*span, ty);
        Some(ty)
    }

    fn resolve_expr_inner(&mut self, expr: &Expr, span: Span) -> Option<DspType> {
        match expr {
            Expr::Number(_, Some(suffix)) => Some(type_from_unit_suffix(*suffix)),
            Expr::Number(_, None) => Some(DspType::Number),
            Expr::Bool(_) => Some(DspType::Bool),
            Expr::StringLit(_) => None, // not used in process blocks
            Expr::Ident(name) => self.resolve_ident(name, span),
            Expr::FieldAccess(base_expr, field) => {
                self.resolve_field_access(base_expr, field, span)
            }
            Expr::FnCall { callee, args } => self.resolve_fn_call(callee, args, span),
            Expr::Binary { left, op, right } => self.resolve_binary(left, *op, right, span),
            Expr::Unary { operand, .. } => self.resolve_expr(operand),
            Expr::If {
                condition,
                then_body,
                then_expr,
                else_body,
            } => {
                self.resolve_expr(condition);
                self.resolve_statements(then_body);
                let then_ty = self.resolve_expr(then_expr);
                if let Some((else_stmts, else_expr)) = else_body {
                    self.resolve_statements(else_stmts);
                    self.resolve_expr(else_expr);
                }
                then_ty // use then-branch type for now
            }
            Expr::Grouped(inner) => self.resolve_expr(inner),
            // ── Routing constructs ─────────────────────────────────
            Expr::Split { branches } => self.resolve_split(branches, span),
            Expr::Merge => self.resolve_merge(span),
            Expr::Feedback { body } => self.resolve_feedback(body, span),
            Expr::Error => None,
        }
    }

    // ── Routing construct resolution ────────────────────────

    /// Resolve a split block: validate each branch independently.
    /// Split produces Signal (conceptually a multi-channel fan-out).
    fn resolve_split(
        &mut self,
        branches: &[Vec<Spanned<Statement>>],
        _span: Span,
    ) -> Option<DspType> {
        for branch in branches {
            self.resolve_statements(branch);
        }
        self.split_depth += 1;
        Some(DspType::Signal)
    }

    /// Resolve a merge keyword. Merge is a combiner: Processor (signal→signal).
    /// Validates that we're inside a split context (split_depth > 0),
    /// otherwise emits E008.
    fn resolve_merge(&mut self, span: Span) -> Option<DspType> {
        if self.split_depth == 0 {
            self.diagnostics.push(
                Diagnostic::error(
                    "E008",
                    span,
                    "merge without preceding split — merge must follow a split block in a chain",
                )
                .with_suggestion("Add a split { ... } block before merge in the chain."),
            );
            return None;
        }
        self.split_depth -= 1;
        Some(DspType::Processor)
    }

    /// Resolve a feedback block: the body must be a valid signal processing
    /// chain (should resolve without errors). Feedback wraps a chain as a
    /// Processor (signal→signal with implicit one-sample delay).
    fn resolve_feedback(
        &mut self,
        body: &[Spanned<Statement>],
        span: Span,
    ) -> Option<DspType> {
        // Resolve all body statements
        self.resolve_statements(body);

        // Check that the body's last expression produces Signal or Processor
        let last_ty = body.last().and_then(|(stmt, _)| match stmt {
            Statement::Expr(expr) => self.type_map.get(&(expr.1.start, expr.1.end)).copied(),
            Statement::Let { value, .. } => {
                self.type_map.get(&(value.1.start, value.1.end)).copied()
            }
            Statement::Return(expr) => {
                self.type_map.get(&(expr.1.start, expr.1.end)).copied()
            }
            Statement::Assign { value, .. } => {
                self.type_map.get(&(value.1.start, value.1.end)).copied()
            }
        });

        match last_ty {
            Some(DspType::Signal) | Some(DspType::Processor) => {}
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        "E009",
                        span,
                        "feedback body must be a signal processing chain",
                    )
                    .with_suggestion(
                        "The feedback body should end with a Signal or Processor expression.",
                    ),
                );
            }
        }

        Some(DspType::Processor)
    }

    // ── Identifier resolution ────────────────────────────────

    fn resolve_ident(&self, name: &str, _span: Span) -> Option<DspType> {
        match name {
            "input" => Some(DspType::Signal),
            "output" => Some(DspType::Signal),
            "tempo" => Some(DspType::Number),
            "beat_position" => Some(DspType::Number),
            _ => {
                // Check named bus declarations (e.g. `sidechain` from `input sidechain stereo`)
                if self.bus_names.contains_key(name) {
                    return Some(DspType::Signal);
                }
                // Check let-binding scope
                self.scope.get(name).copied()
            }
        }
    }

    // ── Field access ─────────────────────────────────────────

    fn resolve_field_access(
        &mut self,
        base_expr: &Spanned<Expr>,
        field: &str,
        _span: Span,
    ) -> Option<DspType> {
        let (base, _base_span) = base_expr;

        // param.X — look up in param declarations
        if let Expr::Ident(ref base_name) = base {
            if base_name == "param" {
                return self.resolve_param_field(field, _span);
            }
            if base_name == "note" {
                return self.resolve_note_field(field, _span);
            }
        }

        // For other field accesses, resolve the base and propagate
        self.resolve_expr(base_expr)
    }

    fn resolve_param_field(&mut self, field: &str, span: Span) -> Option<DspType> {
        match self.params.get(field) {
            Some(param_type) => Some(param_type_to_dsp_type(param_type)),
            None => {
                self.diagnostics.push(
                    Diagnostic::error(
                        "E005",
                        span,
                        format!("unknown parameter '{}'", field),
                    )
                    .with_suggestion(format!(
                        "Declare the parameter first: `param {}: float = 0.5 in 0.0..1.0`",
                        field
                    )),
                );
                None
            }
        }
    }

    fn resolve_note_field(&mut self, field: &str, span: Span) -> Option<DspType> {
        if !self.has_midi_note {
            self.diagnostics.push(
                Diagnostic::error(
                    "E005",
                    span,
                    format!("'note.{}' used without a MIDI note handler", field),
                )
                .with_suggestion("Add a `midi { note { ... } }` block to use note fields"),
            );
            return None;
        }
        match field {
            "pitch" => Some(DspType::Frequency),
            "velocity" => Some(DspType::Number),
            "gate" => Some(DspType::Bool),
            "pressure" => Some(DspType::Number),
            "bend" => Some(DspType::Number),
            "slide" => Some(DspType::Number),
            "number" => Some(DspType::Number),
            _ => Some(DspType::Number),
        }
    }

    // ── Function call resolution ─────────────────────────────

    fn resolve_fn_call(
        &mut self,
        callee: &Spanned<Expr>,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Option<DspType> {
        // Extract callee name
        let callee_name = match &callee.0 {
            Expr::Ident(name) => name.clone(),
            _ => {
                // Non-ident callee — resolve it and move on
                self.resolve_expr(callee);
                for arg in args {
                    self.resolve_expr(arg);
                }
                return None;
            }
        };

        // Special handling for wavetable_osc() — takes a wavetable name, pitch, and position
        if callee_name == "wavetable_osc" {
            if args.len() != 3 {
                self.diagnostics.push(Diagnostic::error(
                    "E004",
                    span,
                    format!(
                        "function 'wavetable_osc' expects 3 arguments (wavetable, pitch, position), got {}",
                        args.len()
                    ),
                ));
                return None;
            }
            if let Expr::Ident(ref wt_name) = args[0].0 {
                if self.wavetables.contains_key(wt_name) {
                    self.record(args[0].1, DspType::Signal);
                    self.resolve_expr(&args[1]);
                    self.resolve_expr(&args[2]);
                    return Some(DspType::Signal);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E003",
                            span,
                            format!("unknown wavetable '{}' in wavetable_osc() call", wt_name),
                        )
                        .with_suggestion(format!(
                            "Declare the wavetable first: `wavetable {} \"path/to/file.wav\"`",
                            wt_name
                        )),
                    );
                    return None;
                }
            }
        }

        // Special handling for play() — takes a sample name, not standard DSP types
        if callee_name == "play" {
            if args.len() != 1 {
                self.diagnostics.push(Diagnostic::error(
                    "E004",
                    span,
                    format!(
                        "function 'play' expects 1 argument (sample name), got {}",
                        args.len()
                    ),
                ));
                return None;
            }
            if let Expr::Ident(ref sample_name) = args[0].0 {
                if self.samples.contains_key(sample_name) {
                    // Resolve the arg (for type_map completeness)
                    self.record(args[0].1, DspType::Signal);
                    return Some(DspType::Signal);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E003",
                            span,
                            format!("unknown sample '{}' in play() call", sample_name),
                        )
                        .with_suggestion(format!("Declare the sample first: `sample {} \"path/to/file.wav\"`", sample_name)),
                    );
                    return None;
                }
            }
        }

        // Special handling for loop() — like play() but with wraparound playback
        if callee_name == "loop" && args.len() != 1 && args.len() != 3 {
            self.diagnostics.push(Diagnostic::error(
                "E004",
                span,
                format!(
                    "function 'loop' expects 1 or 3 arguments (sample [, start, end]), got {}",
                    args.len()
                ),
            ));
            return None;
        }
        if callee_name == "loop" && (args.len() == 1 || args.len() == 3) {
            if let Expr::Ident(ref sample_name) = args[0].0 {
                if self.samples.contains_key(sample_name) {
                    self.record(args[0].1, DspType::Signal);
                    // For 3-arg variant, resolve start and end arguments
                    if args.len() == 3 {
                        self.resolve_expr(&args[1]);
                        self.resolve_expr(&args[2]);
                    }
                    return Some(DspType::Signal);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E003",
                            span,
                            format!("unknown sample '{}' in loop() call", sample_name),
                        )
                        .with_suggestion(format!("Declare the sample first: `sample {} \"path/to/file.wav\"`", sample_name)),
                    );
                    return None;
                }
            }
        }

        // Check user-defined functions first
        if let Some(fn_def) = self.fn_defs.get(&callee_name).cloned() {
            // Validate argument count
            if args.len() != fn_def.params.len() {
                self.diagnostics.push(Diagnostic::error(
                    "E004",
                    span,
                    format!(
                        "function '{}' expects {} argument{}, got {}",
                        callee_name,
                        fn_def.params.len(),
                        if fn_def.params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
                for arg in args {
                    self.resolve_expr(arg);
                }
                return None;
            }

            // Resolve arguments
            for arg in args {
                self.resolve_expr(arg);
            }

            // Resolve the fn body in a new scope with params bound
            let saved_scope = self.scope.clone();
            for (i, param) in fn_def.params.iter().enumerate() {
                // Use the resolved type of each argument, default to Signal
                let arg_ty = self.type_map
                    .get(&(args[i].1.start, args[i].1.end))
                    .copied()
                    .unwrap_or(DspType::Signal);
                self.scope.insert(param.name.clone(), arg_ty);
            }
            self.resolve_statements(&fn_def.body);
            self.scope = saved_scope;

            // Return type: use hint if given, otherwise infer from last expression
            let return_type = match fn_def.return_hint {
                Some(FnReturnHint::Processor) => DspType::Processor,
                Some(FnReturnHint::Signal) => DspType::Signal,
                None => {
                    // Infer from last body statement
                    fn_def.body.last().and_then(|(stmt, _)| match stmt {
                        Statement::Expr(expr) => {
                            self.type_map.get(&(expr.1.start, expr.1.end)).copied()
                        }
                        Statement::Return(expr) => {
                            self.type_map.get(&(expr.1.start, expr.1.end)).copied()
                        }
                        _ => None,
                    }).unwrap_or(DspType::Processor)
                }
            };

            return Some(return_type);
        }

        // Look up in DSP registry (takes priority over local scope)
        let func = match self.registry.lookup(&callee_name) {
            Some(f) => f.clone(), // clone to release the borrow on self
            None => {
                // Unknown function — E003
                let mut diag = Diagnostic::error(
                    "E003",
                    span,
                    format!("Unknown function '{callee_name}'"),
                );
                if let Some(suggestion) = suggest_function(&callee_name, self.registry) {
                    diag = diag
                        .with_suggestion(format!("Did you mean '{suggestion}'?"));
                }
                self.diagnostics.push(diag);
                // Still resolve args so we don't cascade errors
                for arg in args {
                    self.resolve_expr(arg);
                }
                return None;
            }
        };

        // Check argument count
        let required_count = func.params.iter().filter(|p| !p.optional).count();
        let total_count = func.params.len();
        let actual_count = args.len();

        if actual_count < required_count || actual_count > total_count {
            let msg = if required_count == total_count {
                format!(
                    "Function '{}' expects {} argument{}, got {}",
                    callee_name,
                    required_count,
                    if required_count == 1 { "" } else { "s" },
                    actual_count
                )
            } else {
                format!(
                    "Function '{}' expects {}-{} arguments, got {}",
                    callee_name, required_count, total_count, actual_count
                )
            };
            self.diagnostics
                .push(Diagnostic::error("E004", span, msg));
            // Still resolve args
            for arg in args {
                self.resolve_expr(arg);
            }
            return Some(func.return_type);
        }

        // Type-check each argument
        for (i, arg) in args.iter().enumerate() {
            let arg_ty = self.resolve_expr(arg);
            if let Some(arg_ty) = arg_ty {
                let expected = &func.params[i];
                if !arg_ty.is_compatible_with(expected.dsp_type) {
                    self.diagnostics.push(Diagnostic::error(
                        "E005",
                        arg.1,
                        format!(
                            "Expected {} for parameter '{}', got {}",
                            expected.dsp_type, expected.name, arg_ty
                        ),
                    ));
                }
            }
        }

        Some(func.return_type)
    }

    // ── Binary expression resolution ─────────────────────────

    fn resolve_binary(
        &mut self,
        left: &Spanned<Expr>,
        op: BinOp,
        right: &Spanned<Expr>,
        span: Span,
    ) -> Option<DspType> {
        let left_ty = self.resolve_expr(left);
        let right_ty = self.resolve_expr(right);

        match op {
            BinOp::Chain => {
                self.resolve_chain(left_ty, right_ty, left, right, span)
            }
            // Arithmetic ops: result depends on operand types
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                match (left_ty, right_ty) {
                    // Signal * Signal -> Signal (ring modulation, AM synthesis)
                    // Signal +/- Signal -> Signal (mixing, subtraction)
                    (Some(DspType::Signal), Some(DspType::Signal))
                        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) =>
                    {
                        Some(DspType::Signal)
                    }
                    // Signal * Number -> Signal (amplitude scaling)
                    // Number * Signal -> Signal (commutative)
                    (Some(DspType::Signal), Some(rhs))
                        if op == BinOp::Mul && rhs.is_numeric_domain() =>
                    {
                        Some(DspType::Signal)
                    }
                    (Some(lhs), Some(DspType::Signal))
                        if op == BinOp::Mul && lhs.is_numeric_domain() =>
                    {
                        Some(DspType::Signal)
                    }
                    // Signal +/- Number -> Signal (DC offset, level adjustment)
                    (Some(DspType::Signal), Some(rhs))
                        if matches!(op, BinOp::Add | BinOp::Sub) && rhs.is_numeric_domain() =>
                    {
                        Some(DspType::Signal)
                    }
                    (Some(lhs), Some(DspType::Signal))
                        if matches!(op, BinOp::Add | BinOp::Sub) && lhs.is_numeric_domain() =>
                    {
                        Some(DspType::Signal)
                    }
                    // Default: pure numeric arithmetic
                    _ => Some(DspType::Number),
                }
            }
            // Comparison ops: result is Bool
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq
            | BinOp::GtEq => Some(DspType::Bool),
            // Logical ops: result is Bool
            BinOp::And | BinOp::Or => Some(DspType::Bool),
        }
    }

    fn resolve_chain(
        &mut self,
        left_ty: Option<DspType>,
        right_ty: Option<DspType>,
        _left: &Spanned<Expr>,
        _right: &Spanned<Expr>,
        span: Span,
    ) -> Option<DspType> {
        match (left_ty, right_ty) {
            // Signal -> Processor → Signal (standard audio chain)
            (Some(DspType::Signal), Some(DspType::Processor)) => Some(DspType::Signal),
            // Signal -> Signal → valid chain to output destination
            (Some(DspType::Signal), Some(DspType::Signal)) => {
                // This is the `... -> output` case — output is Signal type,
                // and chaining a signal to the output destination is valid
                Some(DspType::Signal)
            }
            // Processor -> Processor → Processor (chaining processors)
            (Some(DspType::Processor), Some(DspType::Processor)) => {
                Some(DspType::Processor)
            }
            // Signal -> Envelope → valid (envelope modulates signal)
            (Some(DspType::Signal), Some(DspType::Envelope)) => Some(DspType::Signal),
            // Number/Gain/etc passed where Processor expected on right side
            (Some(left_t), Some(right_t))
                if left_t == DspType::Signal
                    && right_t != DspType::Processor
                    && right_t != DspType::Signal
                    && right_t != DspType::Envelope =>
            {
                self.diagnostics.push(Diagnostic::error(
                    "E006",
                    span,
                    format!(
                        "Cannot chain {} into {} — right side of -> must be a Processor",
                        left_t, right_t
                    ),
                ));
                None
            }
            // Other type on left that isn't Signal
            (Some(left_t), Some(right_t))
                if left_t != DspType::Signal
                    && left_t != DspType::Processor =>
            {
                self.diagnostics.push(Diagnostic::error(
                    "E006",
                    span,
                    format!(
                        "Cannot chain {} into {} — left side of -> must be Signal or Processor",
                        left_t, right_t
                    ),
                ));
                None
            }
            // If either side failed to resolve, don't cascade
            _ => None,
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────

/// Map a ParamType (from AST) to a DspType (for type checking).
fn param_type_to_dsp_type(param_type: &ParamType) -> DspType {
    match param_type {
        ParamType::Float | ParamType::Int => DspType::Number,
        ParamType::Bool => DspType::Bool,
        ParamType::Enum(_) => DspType::Number, // enums are numeric indices
    }
}

/// Simple Levenshtein edit distance for "did you mean?" suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    // Use a single row + prev value to save memory
    let mut row: Vec<usize> = (0..=b_len).collect();

    for i in 1..=a_len {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            let new_val = (prev + cost)
                .min(row[j] + 1)
                .min(row[j - 1] + 1);
            prev = row[j];
            row[j] = new_val;
        }
    }

    row[b_len]
}

/// Find the closest function name in the registry, if within edit distance 3.
fn suggest_function(name: &str, registry: &DspRegistry) -> Option<String> {
    registry
        .functions
        .keys()
        .map(|known| (known.clone(), edit_distance(name, known)))
        .filter(|(_, dist)| *dist <= 3 && *dist > 0)
        .min_by_key(|(_, dist)| *dist)
        .map(|(name, _)| name)
}

/// Validate a hex color string: must be `#RGB` or `#RRGGBB` with valid hex digits.
fn is_valid_hex_color(s: &str) -> bool {
    let s = s.as_bytes();
    if s.first() != Some(&b'#') {
        return false;
    }
    let hex = &s[1..];
    if hex.len() != 3 && hex.len() != 6 {
        return false;
    }
    hex.iter().all(|b| b.is_ascii_hexdigit())
}
