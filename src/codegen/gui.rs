//! Generates HTML/CSS/JS assets for the Tier 1 auto-layout web editor.
//!
//! The generated editor uses a CSS grid of canvas-drawn rotary knobs,
//! one per float/int parameter. The JS↔Rust bridge uses WebKit message
//! handlers (JS→Rust) and `window.updateParam()` (Rust→JS).

use crate::ast::{GuiBlock, GuiItem, ParamDef, ParamType, PluginDef, PluginItem};

/// Metadata about a parameter, extracted from the AST for GUI generation.
#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub display_name: String,
    pub param_type: ParamInfoType,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub unit: String,
}

/// Simplified param type for GUI purposes (bool/enum skipped in Tier 1).
#[derive(Debug, Clone, PartialEq)]
pub enum ParamInfoType {
    Float,
    Int,
}

/// Extract GUI-relevant param info from the plugin AST.
/// Skips bool and enum params (Tier 1 only renders knobs for float/int).
pub fn collect_param_info(plugin: &PluginDef) -> Vec<ParamInfo> {
    plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::ParamDecl(p) = item {
                param_info_from_def(p)
            } else {
                None
            }
        })
        .collect()
}

/// Extract the GuiBlock from the plugin AST, if present.
pub fn find_gui_block(plugin: &PluginDef) -> Option<&GuiBlock> {
    plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item {
            Some(gui)
        } else {
            None
        }
    })
}

/// Extract theme string from a GuiBlock. Defaults to "dark".
pub fn gui_theme(gui: &GuiBlock) -> &str {
    for (item, _) in &gui.items {
        if let GuiItem::Theme(t) = item {
            return t.as_str();
        }
    }
    "dark"
}

/// Extract accent color from a GuiBlock. Defaults to "#E8A87C".
pub fn gui_accent(gui: &GuiBlock) -> &str {
    for (item, _) in &gui.items {
        if let GuiItem::Accent(a) = item {
            return a.as_str();
        }
    }
    "#E8A87C"
}

/// Generate the complete HTML document with inline CSS and JS for the editor.
pub fn generate_editor_html(plugin: &PluginDef) -> String {
    let gui = find_gui_block(plugin);
    let theme = gui.map_or("dark", gui_theme);
    let accent = gui.map_or("#E8A87C", gui_accent);
    let params = collect_param_info(plugin);

    let css = generate_editor_css(theme, accent);
    let js = generate_editor_js(&params);

    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"UTF-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
    html.push_str(&format!(
        "<title>{} — Muse Editor</title>\n",
        plugin.name
    ));
    html.push_str("<style>\n");
    html.push_str(&css);
    html.push_str("\n</style>\n");
    html.push_str("</head>\n");
    html.push_str(&format!(
        "<body data-theme=\"{}\">\n",
        theme
    ));

    // Plugin title
    html.push_str(&format!(
        "<header class=\"editor-header\"><h1>{}</h1></header>\n",
        plugin.name
    ));

    // Grid of knobs
    html.push_str("<div class=\"knob-grid\">\n");
    for p in &params {
        html.push_str(&format!(
            "  <div class=\"knob-cell\" data-param=\"{}\">\n",
            p.name
        ));
        html.push_str(&format!(
            "    <canvas id=\"knob-{}\" width=\"80\" height=\"80\"></canvas>\n",
            p.name
        ));
        html.push_str(&format!(
            "    <div class=\"knob-value\" id=\"value-{}\">{}</div>\n",
            p.name,
            format_display_value(p.default, &p.unit, &p.param_type)
        ));
        html.push_str(&format!(
            "    <div class=\"knob-label\">{}</div>\n",
            p.display_name
        ));
        html.push_str("  </div>\n");
    }
    html.push_str("</div>\n");

    // JS
    html.push_str("<script>\n");
    html.push_str(&js);
    html.push_str("\n</script>\n");
    html.push_str("</body>\n</html>\n");

    html
}

/// Generate the CSS stylesheet for the editor.
pub fn generate_editor_css(theme: &str, accent: &str) -> String {
    let (bg, fg, knob_bg, knob_track) = if theme == "light" {
        ("#f5f5f5", "#1a1a1a", "#e0e0e0", "#c0c0c0")
    } else {
        ("#1a1a1a", "#e0e0e0", "#2a2a2a", "#3a3a3a")
    };

    format!(
        r#":root {{
  --bg: {bg};
  --fg: {fg};
  --accent: {accent};
  --knob-bg: {knob_bg};
  --knob-track: {knob_track};
  --knob-size: 80px;
}}

* {{
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}}

body {{
  background: var(--bg);
  color: var(--fg);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  font-size: 13px;
  -webkit-font-smoothing: antialiased;
  user-select: none;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  height: 100vh;
}}

.editor-header {{
  padding: 12px 16px 8px;
  text-align: center;
}}

.editor-header h1 {{
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.02em;
  opacity: 0.8;
}}

.knob-grid {{
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
  gap: 16px;
  padding: 16px;
  justify-items: center;
  align-content: start;
  flex: 1;
}}

.knob-cell {{
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}}

.knob-cell canvas {{
  cursor: grab;
}}

.knob-cell canvas:active {{
  cursor: grabbing;
}}

.knob-value {{
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  opacity: 0.9;
  min-height: 16px;
}}

.knob-label {{
  font-size: 11px;
  opacity: 0.5;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}"#,
        bg = bg,
        fg = fg,
        accent = accent,
        knob_bg = knob_bg,
        knob_track = knob_track,
    )
}

/// Generate the JavaScript for knob widgets and the parameter bridge.
pub fn generate_editor_js(params: &[ParamInfo]) -> String {
    let mut js = String::new();

    // Knob widget class
    js.push_str(KNOB_CLASS_JS);
    js.push('\n');

    // Parameter bridge
    js.push_str(PARAM_BRIDGE_JS);
    js.push('\n');

    // Instantiate knobs
    js.push_str("// --- Knob instances ---\n");
    js.push_str("const knobs = {};\n");
    js.push_str("document.addEventListener('DOMContentLoaded', () => {\n");
    for p in params {
        let normalized_default = if (p.max - p.min).abs() > f64::EPSILON {
            (p.default - p.min) / (p.max - p.min)
        } else {
            0.0
        };
        js.push_str(&format!(
            "  knobs[\"{name}\"] = new MuseKnob(\"{name}\", document.getElementById(\"knob-{name}\"), {{\n    min: {min},\n    max: {max},\n    default: {default},\n    value: {normalized:.6},\n    unit: \"{unit}\",\n    isInt: {is_int}\n  }});\n",
            name = p.name,
            min = p.min,
            max = p.max,
            default = p.default,
            normalized = normalized_default,
            unit = p.unit,
            is_int = matches!(p.param_type, ParamInfoType::Int),
        ));
    }
    js.push_str("});\n");

    js
}

// ── Private helpers ──────────────────────────────────────────

fn param_info_from_def(p: &ParamDef) -> Option<ParamInfo> {
    let param_type = match &p.param_type {
        ParamType::Float => ParamInfoType::Float,
        ParamType::Int => ParamInfoType::Int,
        ParamType::Bool | ParamType::Enum(_) => return None,
    };

    let default = p
        .default
        .as_ref()
        .and_then(|(expr, _)| {
            if let crate::ast::Expr::Number(n, _) = expr {
                Some(*n)
            } else {
                None
            }
        })
        .unwrap_or(0.0);

    let (min, max) = p
        .range
        .as_ref()
        .map(|r| {
            let min = match &r.min.0 {
                crate::ast::Expr::Number(n, _) => *n,
                crate::ast::Expr::Unary {
                    op: crate::ast::UnaryOp::Neg,
                    operand,
                } => {
                    if let crate::ast::Expr::Number(n, _) = &operand.0 {
                        -n
                    } else {
                        0.0
                    }
                }
                _ => 0.0,
            };
            let max = match &r.max.0 {
                crate::ast::Expr::Number(n, _) => *n,
                _ => 1.0,
            };
            (min, max)
        })
        .unwrap_or((0.0, 1.0));

    let unit = p
        .options
        .iter()
        .find_map(|(opt, _)| {
            if let crate::ast::ParamOption::Unit(u) = opt {
                Some(u.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    Some(ParamInfo {
        display_name: capitalize_first(&p.name),
        name: p.name.clone(),
        param_type,
        default,
        min,
        max,
        unit,
    })
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn format_display_value(value: f64, unit: &str, param_type: &ParamInfoType) -> String {
    match param_type {
        ParamInfoType::Int => {
            if unit.is_empty() {
                format!("{}", value as i64)
            } else {
                format!("{} {}", value as i64, unit)
            }
        }
        ParamInfoType::Float => {
            if unit.is_empty() {
                format!("{:.2}", value)
            } else {
                format!("{:.2} {}", value, unit)
            }
        }
    }
}

// ── Embedded JS constants ────────────────────────────────────

const KNOB_CLASS_JS: &str = r#"// --- MuseKnob canvas widget ---
class MuseKnob {
  constructor(id, canvas, opts) {
    this.id = id;
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.min = opts.min;
    this.max = opts.max;
    this.defaultValue = opts.default;
    this.value = opts.value; // normalized 0..1
    this.unit = opts.unit || '';
    this.isInt = opts.isInt || false;
    this.dragging = false;
    this.dragStartY = 0;
    this.dragStartValue = 0;

    // Arc geometry
    this.startAngle = 0.75 * Math.PI;
    this.endAngle = 2.25 * Math.PI;
    this.radius = 30;
    this.lineWidth = 4;

    // Read CSS variables
    const style = getComputedStyle(document.documentElement);
    this.accentColor = style.getPropertyValue('--accent').trim();
    this.trackColor = style.getPropertyValue('--knob-track').trim();
    this.bgColor = style.getPropertyValue('--knob-bg').trim();

    this.draw();
    this.bindEvents();
  }

  get realValue() {
    const v = this.min + this.value * (this.max - this.min);
    return this.isInt ? Math.round(v) : v;
  }

  draw() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    const cx = w / 2;
    const cy = h / 2;

    ctx.clearRect(0, 0, w, h);

    // Background circle
    ctx.beginPath();
    ctx.arc(cx, cy, this.radius, 0, 2 * Math.PI);
    ctx.fillStyle = this.bgColor;
    ctx.fill();

    // Track arc (full range)
    ctx.beginPath();
    ctx.arc(cx, cy, this.radius, this.startAngle, this.endAngle);
    ctx.strokeStyle = this.trackColor;
    ctx.lineWidth = this.lineWidth;
    ctx.lineCap = 'round';
    ctx.stroke();

    // Value arc
    const valueAngle = this.startAngle + this.value * (this.endAngle - this.startAngle);
    if (this.value > 0.001) {
      ctx.beginPath();
      ctx.arc(cx, cy, this.radius, this.startAngle, valueAngle);
      ctx.strokeStyle = this.accentColor;
      ctx.lineWidth = this.lineWidth;
      ctx.lineCap = 'round';
      ctx.stroke();
    }

    // Pointer dot
    const dotX = cx + Math.cos(valueAngle) * (this.radius - 10);
    const dotY = cy + Math.sin(valueAngle) * (this.radius - 10);
    ctx.beginPath();
    ctx.arc(dotX, dotY, 3, 0, 2 * Math.PI);
    ctx.fillStyle = this.accentColor;
    ctx.fill();
  }

  updateDisplay() {
    const el = document.getElementById('value-' + this.id);
    if (el) {
      const v = this.realValue;
      const display = this.isInt ? v.toString() : v.toFixed(2);
      el.textContent = this.unit ? display + ' ' + this.unit : display;
    }
  }

  bindEvents() {
    this.canvas.addEventListener('mousedown', (e) => {
      this.dragging = true;
      this.dragStartY = e.clientY;
      this.dragStartValue = this.value;
      e.preventDefault();
    });

    document.addEventListener('mousemove', (e) => {
      if (!this.dragging) return;
      const dy = this.dragStartY - e.clientY;
      const sensitivity = e.shiftKey ? 0.001 : 0.005;
      this.value = Math.max(0, Math.min(1, this.dragStartValue + dy * sensitivity));
      this.draw();
      this.updateDisplay();
      sendParam(this.id, this.value);
    });

    document.addEventListener('mouseup', () => {
      this.dragging = false;
    });

    // Double-click resets to default
    this.canvas.addEventListener('dblclick', () => {
      if ((this.max - this.min) > 0) {
        this.value = (this.defaultValue - this.min) / (this.max - this.min);
      } else {
        this.value = 0;
      }
      this.draw();
      this.updateDisplay();
      sendParam(this.id, this.value);
    });
  }

  setNormalized(v) {
    this.value = Math.max(0, Math.min(1, v));
    this.draw();
    this.updateDisplay();
  }
}"#;

const PARAM_BRIDGE_JS: &str = r#"// --- Parameter bridge ---
function sendParam(id, normalizedValue) {
  try {
    window.webkit.messageHandlers.paramBridge.postMessage(
      JSON.stringify({ id: id, value: normalizedValue })
    );
  } catch (e) {
    // Fallback: no native bridge (preview mode)
    console.log('paramBridge:', id, normalizedValue);
  }
}

// Called from Rust via evaluateJavaScript
window.updateParam = function(id, normalizedValue) {
  if (knobs[id]) {
    knobs[id].setNormalized(normalizedValue);
  }
};"#;

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::span::Span;

    /// Build a minimal PluginDef with the given params and optional gui block.
    fn make_plugin(name: &str, params: Vec<ParamDef>, gui: Option<GuiBlock>) -> PluginDef {
        let mut items: Vec<Spanned<PluginItem>> = params
            .into_iter()
            .map(|p| (PluginItem::ParamDecl(Box::new(p)), Span::new(0, 0)))
            .collect();
        if let Some(g) = gui {
            items.push((PluginItem::GuiDecl(g), Span::new(0, 0)));
        }
        PluginDef {
            name: name.to_string(),
            items,
            span: Span::new(0, 0),
        }
    }

    fn make_float_param(name: &str, default: f64, min: f64, max: f64, unit: &str) -> ParamDef {
        ParamDef {
            name: name.to_string(),
            param_type: ParamType::Float,
            default: Some((Expr::Number(default, None), Span::new(0, 0))),
            range: Some(ParamRange {
                min: (Expr::Number(min, None), Span::new(0, 0)),
                max: (Expr::Number(max, None), Span::new(0, 0)),
                span: Span::new(0, 0),
            }),
            options: if unit.is_empty() {
                vec![]
            } else {
                vec![(ParamOption::Unit(unit.to_string()), Span::new(0, 0))]
            },
            span: Span::new(0, 0),
        }
    }

    fn make_int_param(name: &str, default: f64, min: f64, max: f64) -> ParamDef {
        ParamDef {
            name: name.to_string(),
            param_type: ParamType::Int,
            default: Some((Expr::Number(default, None), Span::new(0, 0))),
            range: Some(ParamRange {
                min: (Expr::Number(min, None), Span::new(0, 0)),
                max: (Expr::Number(max, None), Span::new(0, 0)),
                span: Span::new(0, 0),
            }),
            options: vec![],
            span: Span::new(0, 0),
        }
    }

    fn make_bool_param(name: &str) -> ParamDef {
        ParamDef {
            name: name.to_string(),
            param_type: ParamType::Bool,
            default: None,
            range: None,
            options: vec![],
            span: Span::new(0, 0),
        }
    }

    fn dark_gui(accent: &str) -> GuiBlock {
        GuiBlock {
            items: vec![
                (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                (GuiItem::Accent(accent.to_string()), Span::new(0, 0)),
            ],
            span: Span::new(0, 0),
        }
    }

    fn light_gui(accent: &str) -> GuiBlock {
        GuiBlock {
            items: vec![
                (GuiItem::Theme("light".to_string()), Span::new(0, 0)),
                (GuiItem::Accent(accent.to_string()), Span::new(0, 0)),
            ],
            span: Span::new(0, 0),
        }
    }

    // ── collect_param_info ───────────────────────────────────

    #[test]
    fn gui_collect_param_info_float_and_int() {
        let plugin = make_plugin(
            "Test",
            vec![
                make_float_param("gain", 0.0, -30.0, 30.0, "dB"),
                make_int_param("octave", 0.0, -2.0, 2.0),
            ],
            None,
        );
        let infos = collect_param_info(&plugin);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].name, "gain");
        assert_eq!(infos[0].param_type, ParamInfoType::Float);
        assert_eq!(infos[0].unit, "dB");
        assert_eq!(infos[1].name, "octave");
        assert_eq!(infos[1].param_type, ParamInfoType::Int);
    }

    #[test]
    fn gui_collect_param_info_skips_bool() {
        let plugin = make_plugin(
            "Test",
            vec![
                make_float_param("gain", 0.0, -30.0, 30.0, ""),
                make_bool_param("bypass"),
            ],
            None,
        );
        let infos = collect_param_info(&plugin);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "gain");
    }

    // ── generate_editor_css ──────────────────────────────────

    #[test]
    fn gui_css_dark_theme() {
        let css = generate_editor_css("dark", "#E8A87C");
        assert!(css.contains("--bg: #1a1a1a"));
        assert!(css.contains("--fg: #e0e0e0"));
        assert!(css.contains("--accent: #E8A87C"));
        assert!(css.contains("--knob-bg: #2a2a2a"));
    }

    #[test]
    fn gui_css_light_theme() {
        let css = generate_editor_css("light", "#3366FF");
        assert!(css.contains("--bg: #f5f5f5"));
        assert!(css.contains("--fg: #1a1a1a"));
        assert!(css.contains("--accent: #3366FF"));
        assert!(css.contains("--knob-bg: #e0e0e0"));
    }

    #[test]
    fn gui_css_dark_vs_light_differs() {
        let dark = generate_editor_css("dark", "#E8A87C");
        let light = generate_editor_css("light", "#E8A87C");
        assert_ne!(dark, light);
        // Both share the same accent
        assert!(dark.contains("--accent: #E8A87C"));
        assert!(light.contains("--accent: #E8A87C"));
    }

    // ── generate_editor_js ───────────────────────────────────

    #[test]
    fn gui_js_contains_knob_class() {
        let js = generate_editor_js(&[]);
        assert!(js.contains("class MuseKnob"));
        assert!(js.contains("window.updateParam"));
        assert!(js.contains("sendParam"));
        assert!(js.contains("paramBridge.postMessage"));
    }

    #[test]
    fn gui_js_instantiates_knobs() {
        let params = vec![ParamInfo {
            name: "gain".to_string(),
            display_name: "Gain".to_string(),
            param_type: ParamInfoType::Float,
            default: 0.0,
            min: -30.0,
            max: 30.0,
            unit: "dB".to_string(),
        }];
        let js = generate_editor_js(&params);
        assert!(js.contains("knobs[\"gain\"] = new MuseKnob(\"gain\""));
        assert!(js.contains("min: -30"));
        assert!(js.contains("max: 30"));
        assert!(js.contains("unit: \"dB\""));
    }

    // ── generate_editor_html ─────────────────────────────────

    #[test]
    fn gui_html_single_param_dark() {
        let plugin = make_plugin(
            "Warm Gain",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            Some(dark_gui("#E8A87C")),
        );
        let html = generate_editor_html(&plugin);

        // Structure
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Warm Gain — Muse Editor</title>"));
        assert!(html.contains("data-theme=\"dark\""));

        // Grid + knob
        assert!(html.contains("class=\"knob-grid\""));
        assert!(html.contains("data-param=\"gain\""));
        assert!(html.contains("id=\"knob-gain\""));
        assert!(html.contains("class=\"knob-label\">Gain</div>"));

        // CSS variables
        assert!(html.contains("--bg: #1a1a1a"));
        assert!(html.contains("--accent: #E8A87C"));

        // JS
        assert!(html.contains("class MuseKnob"));
        assert!(html.contains("knobs[\"gain\"]"));
    }

    #[test]
    fn gui_html_light_theme() {
        let plugin = make_plugin(
            "Bright",
            vec![make_float_param("level", 0.5, 0.0, 1.0, "")],
            Some(light_gui("#0066CC")),
        );
        let html = generate_editor_html(&plugin);

        assert!(html.contains("data-theme=\"light\""));
        assert!(html.contains("--bg: #f5f5f5"));
        assert!(html.contains("--accent: #0066CC"));
    }

    #[test]
    fn gui_html_multiple_params() {
        let plugin = make_plugin(
            "Multi",
            vec![
                make_float_param("gain", 0.0, -30.0, 30.0, "dB"),
                make_float_param("mix", 0.5, 0.0, 1.0, ""),
                make_int_param("octave", 0.0, -2.0, 2.0),
            ],
            Some(dark_gui("#FF6633")),
        );
        let html = generate_editor_html(&plugin);

        assert!(html.contains("data-param=\"gain\""));
        assert!(html.contains("data-param=\"mix\""));
        assert!(html.contains("data-param=\"octave\""));
        assert!(html.contains("knobs[\"gain\"]"));
        assert!(html.contains("knobs[\"mix\"]"));
        assert!(html.contains("knobs[\"octave\"]"));
    }

    #[test]
    fn gui_html_no_gui_block_uses_defaults() {
        let plugin = make_plugin(
            "Plain",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            None,
        );
        let html = generate_editor_html(&plugin);
        // Should still generate with dark theme and default accent
        assert!(html.contains("data-theme=\"dark\""));
        assert!(html.contains("--accent: #E8A87C"));
    }

    #[test]
    fn gui_html_int_param_display() {
        let plugin = make_plugin(
            "IntTest",
            vec![make_int_param("octave", 1.0, -2.0, 2.0)],
            Some(dark_gui("#AABBCC")),
        );
        let html = generate_editor_html(&plugin);
        // Int param should show integer default
        assert!(html.contains(">1</div>"));
        assert!(html.contains("isInt: true"));
    }

    // ── ParamInfo extraction ─────────────────────────────────

    #[test]
    fn gui_param_info_negative_range() {
        let p = ParamDef {
            name: "gain".to_string(),
            param_type: ParamType::Float,
            default: Some((Expr::Number(0.0, None), Span::new(0, 0))),
            range: Some(ParamRange {
                min: (
                    Expr::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new((Expr::Number(30.0, None), Span::new(0, 0))),
                    },
                    Span::new(0, 0),
                ),
                max: (Expr::Number(30.0, None), Span::new(0, 0)),
                span: Span::new(0, 0),
            }),
            options: vec![],
            span: Span::new(0, 0),
        };
        let info = param_info_from_def(&p).unwrap();
        assert_eq!(info.min, -30.0);
        assert_eq!(info.max, 30.0);
    }

    // ── JS bridge protocol ───────────────────────────────────

    #[test]
    fn gui_js_bridge_protocol() {
        let js = generate_editor_js(&[]);
        // JS→Rust bridge
        assert!(js.contains("window.webkit.messageHandlers.paramBridge.postMessage"));
        assert!(js.contains("JSON.stringify({ id: id, value: normalizedValue })"));
        // Rust→JS bridge
        assert!(js.contains("window.updateParam = function(id, normalizedValue)"));
    }
}
