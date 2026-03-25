//! Semantic resolution pass for the Muse language.
//!
//! Walks the parsed AST and validates DSP function calls against the registry.
//! Produces structured `Diagnostic`s with error codes E003–E006 for semantic
//! errors and a side-table mapping expression spans to their resolved `DspType`.

use std::collections::HashMap;

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
/// `Err(Vec<Diagnostic>)` with E003–E006 diagnostics on failure.
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
    /// Whether the plugin has MIDI note handlers (enables note.X resolution)
    has_midi_note: bool,
    /// let-binding scope: name → resolved DspType
    scope: HashMap<String, DspType>,
    /// Span → DspType map (the output)
    type_map: HashMap<(usize, usize), DspType>,
    /// Accumulated diagnostics
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Resolver<'a> {
    fn new(registry: &'a DspRegistry) -> Self {
        Self {
            registry,
            params: HashMap::new(),
            has_midi_note: false,
            scope: HashMap::new(),
            type_map: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn record(&mut self, span: Span, ty: DspType) {
        self.type_map.insert((span.start, span.end), ty);
    }

    // ── Plugin-level ─────────────────────────────────────────

    fn resolve_plugin(&mut self, plugin: &PluginDef) {
        // Phase 1: extract param declarations and detect MIDI note handlers
        for (item, _span) in &plugin.items {
            match item {
                PluginItem::ParamDecl(param_def) => {
                    self.params
                        .insert(param_def.name.clone(), param_def.param_type.clone());
                }
                PluginItem::MidiDecl(midi_decl) => {
                    for (midi_item, _) in &midi_decl.items {
                        if matches!(midi_item, MidiItem::NoteHandler(_)) {
                            self.has_midi_note = true;
                        }
                    }
                }
                _ => {}
            }
        }

        // Phase 2: resolve MIDI handlers first (they define note bindings)
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

        // Phase 3: resolve process blocks
        for (item, _span) in &plugin.items {
            if let PluginItem::ProcessBlock(block) = item {
                self.resolve_statements(&block.body);
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
                if let Some(ty) = self.resolve_expr(value) {
                    self.scope.insert(name.clone(), ty);
                }
            }
            Statement::Assign { value, .. } => {
                self.resolve_expr(value);
            }
            Statement::Return(expr) => {
                self.resolve_expr(expr);
            }
            Statement::Expr(expr) => {
                self.resolve_expr(expr);
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
            // Routing constructs — full resolution implemented in S03/T03
            Expr::Split { .. } | Expr::Merge | Expr::Feedback { .. } => None,
            Expr::Error => None,
        }
    }

    // ── Identifier resolution ────────────────────────────────

    fn resolve_ident(&self, name: &str, _span: Span) -> Option<DspType> {
        match name {
            "input" => Some(DspType::Signal),
            "output" => Some(DspType::Signal),
            _ => {
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

    fn resolve_param_field(&self, field: &str, _span: Span) -> Option<DspType> {
        match self.params.get(field) {
            Some(param_type) => Some(param_type_to_dsp_type(param_type)),
            None => {
                // Unknown param — we could emit an error, but for now just return Number
                // (param errors are parser-level; resolve assumes params are declared)
                self.diagnostics
                    .len(); // no-op to avoid unused warning
                Some(DspType::Number)
            }
        }
    }

    fn resolve_note_field(&self, field: &str, _span: Span) -> Option<DspType> {
        if !self.has_midi_note {
            // note.X used in a plugin without MIDI note handler
            return Some(DspType::Number);
        }
        match field {
            "pitch" => Some(DspType::Frequency),
            "velocity" => Some(DspType::Number),
            "gate" => Some(DspType::Bool),
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

        // Look up in DSP registry first (takes priority over local scope)
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
            // Arithmetic ops: result is Number (or propagate domain types)
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                Some(DspType::Number)
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
