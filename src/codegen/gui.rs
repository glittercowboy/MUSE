//! Generates HTML/CSS/JS assets for the web editor.
//!
//! **Tier 1 (auto-layout):** CSS grid of canvas-drawn rotary knobs, one per float/int param.
//! **Tier 2 (structured layout):** Recursive layout/panel/widget tree → distinct HTML.
//! **Tier 3 (CSS escape hatch):** Raw CSS injection via `css "..."` blocks.
//!
//! The JS↔Rust bridge uses WebKit message handlers (JS→Rust) and
//! `window.updateParam()` (Rust→JS).

use crate::ast::{
    GuiBlock, GuiItem, LayoutDecl, LayoutDirection, PanelDecl, ParamDef, ParamType, PluginDef,
    PluginItem, WidgetDecl, WidgetProp, WidgetType,
};

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

// ── Tier detection & size extraction ──────────────────────────

/// Returns true if the GuiBlock contains any Tier 2+ items (Layout, Widget, Panel).
pub fn is_tier2(gui: &GuiBlock) -> bool {
    gui.items.iter().any(|(item, _)| {
        matches!(
            item,
            GuiItem::Layout(_) | GuiItem::Widget(_) | GuiItem::Panel(_)
        )
    })
}

/// Extract editor dimensions from a GuiBlock. Defaults to (600, 400).
pub fn gui_size(gui: &GuiBlock) -> (u32, u32) {
    for (item, _) in &gui.items {
        if let GuiItem::Size(w, h) = item {
            return (*w, *h);
        }
    }
    (600, 400)
}

/// Collect all raw CSS strings from `css "..."` items (Tier 3).
fn collect_custom_css(gui: &GuiBlock) -> Vec<&str> {
    gui.items
        .iter()
        .filter_map(|(item, _)| {
            if let GuiItem::Css(s) = item {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect()
}

// ── Tier 2 HTML generation ───────────────────────────────────

/// Generate HTML body content for Tier 2 structured layouts.
/// Walks the GuiItem tree recursively, producing div containers for
/// layouts/panels and appropriate widget HTML for each widget type.
fn generate_tier2_body(items: &[(GuiItem, crate::span::Span)], params: &[ParamInfo]) -> String {
    let mut html = String::new();
    for (item, _) in items {
        match item {
            GuiItem::Layout(layout) => {
                html.push_str(&generate_layout_html(layout, params));
            }
            GuiItem::Panel(panel) => {
                html.push_str(&generate_panel_html(panel, params));
            }
            GuiItem::Widget(widget) => {
                html.push_str(&generate_widget_html(widget, params));
            }
            // Theme/Accent/Size/Css are handled elsewhere
            _ => {}
        }
    }
    html
}

fn generate_layout_html(layout: &LayoutDecl, params: &[ParamInfo]) -> String {
    let dir_class = match layout.direction {
        LayoutDirection::Horizontal => "layout-horizontal",
        LayoutDirection::Vertical => "layout-vertical",
        LayoutDirection::Grid => "layout-grid",
    };
    let mut html = format!("<div class=\"layout {}\">\n", dir_class);
    html.push_str(&generate_tier2_body(&layout.children, params));
    html.push_str("</div>\n");
    html
}

fn generate_panel_html(panel: &PanelDecl, params: &[ParamInfo]) -> String {
    let mut html = String::from("<div class=\"panel\">\n");
    html.push_str(&format!(
        "<h2 class=\"panel-title\">{}</h2>\n",
        panel.title
    ));
    html.push_str(&generate_tier2_body(&panel.children, params));
    html.push_str("</div>\n");
    html
}

fn generate_widget_html(widget: &WidgetDecl, params: &[ParamInfo]) -> String {
    // Collect optional class and style props
    let extra_class = widget
        .props
        .iter()
        .find_map(|p| {
            if let WidgetProp::Class(c) = p {
                Some(c.as_str())
            } else {
                None
            }
        })
        .unwrap_or("");
    let data_style = widget
        .props
        .iter()
        .find_map(|p| {
            if let WidgetProp::Style(s) = p {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("");
    let custom_label = widget.props.iter().find_map(|p| {
        if let WidgetProp::Label(l) = p {
            Some(l.as_str())
        } else {
            None
        }
    });

    // Find the param info for param-bound widgets
    let param = widget
        .param_name
        .as_ref()
        .and_then(|name| params.iter().find(|p| p.name == *name));

    match widget.widget_type {
        WidgetType::Knob => generate_knob_widget_html(widget, param, extra_class, data_style, custom_label),
        WidgetType::Slider => generate_slider_widget_html(widget, param, extra_class, data_style, custom_label),
        WidgetType::Meter => generate_meter_widget_html(widget, param, extra_class, data_style),
        WidgetType::Switch => generate_switch_widget_html(widget, param, extra_class, data_style, custom_label),
        WidgetType::Label => generate_label_widget_html(widget, extra_class, data_style),
        WidgetType::Value => generate_value_widget_html(widget, param, extra_class, data_style),
        // Advanced widgets
        WidgetType::XyPad => generate_xy_pad_widget_html(widget, extra_class, data_style),
        WidgetType::Spectrum => generate_vis_widget_html("spectrum", extra_class, data_style),
        WidgetType::Waveform => generate_vis_widget_html("waveform", extra_class, data_style),
        WidgetType::Envelope => generate_vis_widget_html("envelope", extra_class, data_style),
        WidgetType::EqCurve => generate_vis_widget_html("eq-curve", extra_class, data_style),
        WidgetType::Reduction => generate_reduction_widget_html(extra_class, data_style),
    }
}

fn widget_class_attr(base: &str, extra: &str) -> String {
    if extra.is_empty() {
        format!("class=\"{}\"", base)
    } else {
        format!("class=\"{} {}\"", base, extra)
    }
}

fn data_style_attr(style: &str) -> String {
    if style.is_empty() {
        String::new()
    } else {
        format!(" data-style=\"{}\"", style)
    }
}

fn generate_knob_widget_html(
    widget: &WidgetDecl,
    param: Option<&ParamInfo>,
    extra_class: &str,
    data_style: &str,
    custom_label: Option<&str>,
) -> String {
    let name = param
        .map(|p| p.name.as_str())
        .or(widget.param_name.as_deref())
        .unwrap_or("unknown");
    let display_name = custom_label
        .map(|s| s.to_string())
        .or_else(|| param.map(|p| p.display_name.clone()))
        .unwrap_or_else(|| capitalize_first(name));
    let default_display = param
        .map(|p| format_display_value(p.default, &p.unit, &p.param_type))
        .unwrap_or_else(|| "0.00".to_string());

    let cls = widget_class_attr("knob-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param=\"{name}\"{ds}>\n  <canvas id=\"knob-{name}\" width=\"80\" height=\"80\"></canvas>\n  <div class=\"knob-value\" id=\"value-{name}\">{default}</div>\n  <div class=\"knob-label\">{label}</div>\n</div>\n",
        cls = cls,
        name = name,
        ds = ds,
        default = default_display,
        label = display_name,
    )
}

fn generate_slider_widget_html(
    widget: &WidgetDecl,
    param: Option<&ParamInfo>,
    extra_class: &str,
    data_style: &str,
    custom_label: Option<&str>,
) -> String {
    let name = param
        .map(|p| p.name.as_str())
        .or(widget.param_name.as_deref())
        .unwrap_or("unknown");
    let display_name = custom_label
        .map(|s| s.to_string())
        .or_else(|| param.map(|p| p.display_name.clone()))
        .unwrap_or_else(|| capitalize_first(name));
    let min = param.map_or(0.0, |p| p.min);
    let max = param.map_or(1.0, |p| p.max);
    let default = param.map_or(0.5, |p| p.default);
    let step = if param.map_or(false, |p| p.param_type == ParamInfoType::Int) {
        "1"
    } else {
        "0.01"
    };

    let cls = widget_class_attr("slider-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param=\"{name}\"{ds}>\n  <label class=\"slider-label\">{label}</label>\n  <input type=\"range\" id=\"slider-{name}\" min=\"{min}\" max=\"{max}\" value=\"{default}\" step=\"{step}\" class=\"slider-input\">\n  <div class=\"slider-value\" id=\"value-{name}\">{default}</div>\n</div>\n",
        cls = cls,
        name = name,
        ds = ds,
        label = display_name,
        min = min,
        max = max,
        default = default,
        step = step,
    )
}

fn generate_meter_widget_html(
    widget: &WidgetDecl,
    param: Option<&ParamInfo>,
    extra_class: &str,
    data_style: &str,
) -> String {
    let name = param
        .map(|p| p.name.as_str())
        .or(widget.param_name.as_deref())
        .unwrap_or("unknown");
    let cls = widget_class_attr("meter-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param=\"{name}\"{ds}>\n  <svg id=\"meter-{name}\" class=\"meter-svg\" viewBox=\"0 0 20 80\"><rect class=\"meter-bg\" x=\"0\" y=\"0\" width=\"20\" height=\"80\" rx=\"4\"/><rect class=\"meter-fill\" id=\"meter-fill-{name}\" x=\"2\" y=\"40\" width=\"16\" height=\"38\" rx=\"2\"/></svg>\n</div>\n",
        cls = cls,
        name = name,
        ds = ds,
    )
}

fn generate_switch_widget_html(
    widget: &WidgetDecl,
    param: Option<&ParamInfo>,
    extra_class: &str,
    data_style: &str,
    custom_label: Option<&str>,
) -> String {
    let name = param
        .map(|p| p.name.as_str())
        .or(widget.param_name.as_deref())
        .unwrap_or("unknown");
    let display_name = custom_label
        .map(|s| s.to_string())
        .or_else(|| param.map(|p| p.display_name.clone()))
        .unwrap_or_else(|| capitalize_first(name));

    let cls = widget_class_attr("switch-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param=\"{name}\"{ds}>\n  <label class=\"switch\"><input type=\"checkbox\" id=\"switch-{name}\"><span class=\"switch-slider\"></span></label>\n  <div class=\"switch-label\">{label}</div>\n</div>\n",
        cls = cls,
        name = name,
        ds = ds,
        label = display_name,
    )
}

fn generate_label_widget_html(
    widget: &WidgetDecl,
    extra_class: &str,
    data_style: &str,
) -> String {
    let text = widget.label_text.as_deref().unwrap_or("");
    let cls = widget_class_attr("label-widget", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls}{ds}>{text}</div>\n",
        cls = cls,
        ds = ds,
        text = text,
    )
}

fn generate_value_widget_html(
    widget: &WidgetDecl,
    param: Option<&ParamInfo>,
    extra_class: &str,
    data_style: &str,
) -> String {
    let name = param
        .map(|p| p.name.as_str())
        .or(widget.param_name.as_deref())
        .unwrap_or("unknown");
    let default_display = param
        .map(|p| format_display_value(p.default, &p.unit, &p.param_type))
        .unwrap_or_else(|| "0.00".to_string());

    let cls = widget_class_attr("value-widget", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param=\"{name}\"{ds}><span id=\"value-{name}\">{display}</span></div>\n",
        cls = cls,
        name = name,
        ds = ds,
        display = default_display,
    )
}

// ── Advanced widget HTML generators ──────────────────────────

fn generate_xy_pad_widget_html(
    widget: &WidgetDecl,
    extra_class: &str,
    data_style: &str,
) -> String {
    let param_x = widget.param_name.as_deref().unwrap_or("unknown_x");
    let param_y = widget.param_name_y.as_deref().unwrap_or("unknown_y");
    let cls = widget_class_attr("xy-pad-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls} data-param-x=\"{px}\" data-param-y=\"{py}\"{ds}>\n  <canvas id=\"xy-pad-{px}-{py}\" width=\"200\" height=\"200\"></canvas>\n  <div class=\"xy-pad-labels\"><span>{px}</span><span>{py}</span></div>\n</div>\n",
        cls = cls,
        px = param_x,
        py = param_y,
        ds = ds,
    )
}

/// Generate HTML for visualization widgets (spectrum, waveform, envelope, eq-curve).
/// These have no param binding — just a canvas for animated mock display.
fn generate_vis_widget_html(
    vis_type: &str,
    extra_class: &str,
    data_style: &str,
) -> String {
    let cls = widget_class_attr(&format!("{}-cell", vis_type), extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls}{ds}>\n  <canvas id=\"{vis_type}-display\" width=\"300\" height=\"150\"></canvas>\n</div>\n",
        cls = cls,
        ds = ds,
        vis_type = vis_type,
    )
}

/// Generate HTML for gain reduction meter — a vertical bar with animated fill.
fn generate_reduction_widget_html(
    extra_class: &str,
    data_style: &str,
) -> String {
    let cls = widget_class_attr("reduction-cell", extra_class);
    let ds = data_style_attr(data_style);

    format!(
        "<div {cls}{ds}>\n  <svg class=\"reduction-meter\" viewBox=\"0 0 24 100\"><rect class=\"reduction-bg\" x=\"0\" y=\"0\" width=\"24\" height=\"100\" rx=\"4\"/><rect class=\"reduction-fill\" id=\"reduction-fill\" x=\"2\" y=\"60\" width=\"20\" height=\"38\" rx=\"2\"/></svg>\n  <div class=\"reduction-label\">GR</div>\n</div>\n",
        cls = cls,
        ds = ds,
    )
}

// ── Layout/widget CSS ────────────────────────────────────────

/// CSS rules for Tier 2 layout containers and widgets.
fn generate_layout_css() -> &'static str {
    r#"
.layout { display: flex; gap: 16px; }
.layout-horizontal { flex-direction: row; }
.layout-vertical { flex-direction: column; }
.layout-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); gap: 16px; }

.panel { padding: 12px; border-radius: 8px; background: rgba(255,255,255,0.03); }
.panel-title { font-size: 12px; opacity: 0.5; text-transform: uppercase; margin-bottom: 8px; letter-spacing: 0.05em; }

.slider-cell { display: flex; flex-direction: column; align-items: stretch; gap: 4px; }
.slider-label { font-size: 11px; opacity: 0.5; text-transform: uppercase; letter-spacing: 0.05em; }
.slider-input { width: 100%; accent-color: var(--accent); }
.slider-value { font-size: 12px; font-variant-numeric: tabular-nums; opacity: 0.9; text-align: center; }

.meter-cell { display: flex; flex-direction: column; align-items: center; }
.meter-svg { width: 20px; height: 80px; }
.meter-bg { fill: var(--knob-bg); }
.meter-fill { fill: var(--accent); }

.switch-cell { display: flex; flex-direction: column; align-items: center; gap: 4px; }
.switch { position: relative; display: inline-block; width: 36px; height: 20px; }
.switch input { opacity: 0; width: 0; height: 0; }
.switch-slider { position: absolute; inset: 0; background: var(--knob-track); border-radius: 10px; transition: background 0.2s; cursor: pointer; }
.switch-slider::before { content: ''; position: absolute; left: 2px; top: 2px; width: 16px; height: 16px; background: var(--fg); border-radius: 50%; transition: transform 0.2s; }
.switch input:checked + .switch-slider { background: var(--accent); }
.switch input:checked + .switch-slider::before { transform: translateX(16px); }
.switch-label { font-size: 11px; opacity: 0.5; text-transform: uppercase; letter-spacing: 0.05em; }

.label-widget { font-size: 13px; opacity: 0.7; }
.value-widget { font-size: 14px; font-variant-numeric: tabular-nums; }

.xy-pad-cell { display: flex; flex-direction: column; align-items: center; gap: 4px; }
.xy-pad-cell canvas { border: 1px solid var(--knob-track); border-radius: 4px; cursor: crosshair; background: var(--knob-bg); }
.xy-pad-labels { display: flex; justify-content: space-between; width: 100%; font-size: 10px; opacity: 0.4; text-transform: uppercase; }

.spectrum-cell, .waveform-cell, .envelope-cell, .eq-curve-cell { display: flex; flex-direction: column; align-items: center; }
.spectrum-cell canvas, .waveform-cell canvas, .envelope-cell canvas, .eq-curve-cell canvas { border-radius: 4px; background: var(--knob-bg); }

.reduction-cell { display: flex; flex-direction: column; align-items: center; gap: 4px; }
.reduction-meter { width: 24px; height: 100px; }
.reduction-bg { fill: var(--knob-bg); }
.reduction-fill { fill: var(--accent); transition: height 0.1s, y 0.1s; }
.reduction-label { font-size: 10px; opacity: 0.4; text-transform: uppercase; }"#
}

/// Generate the complete HTML document with inline CSS and JS for the editor.
pub fn generate_editor_html(plugin: &PluginDef) -> String {
    let gui = find_gui_block(plugin);
    let theme = gui.map_or("dark", gui_theme);
    let accent = gui.map_or("#E8A87C", gui_accent);
    let params = collect_param_info(plugin);
    let tier2 = gui.map_or(false, is_tier2);
    let custom_css_blocks: Vec<&str> = gui.map_or(vec![], collect_custom_css);

    let css = generate_editor_css(theme, accent, tier2, &custom_css_blocks);
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

    if tier2 {
        // Tier 2: structured layout from AST
        let gui_block = gui.unwrap(); // safe: tier2 implies gui exists
        html.push_str("<div class=\"tier2-root\">\n");
        html.push_str(&generate_tier2_body(&gui_block.items, &params));
        html.push_str("</div>\n");
    } else {
        // Tier 1: auto-layout grid of knobs
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
    }

    // JS
    html.push_str("<script>\n");
    html.push_str(&js);
    html.push_str("\n</script>\n");
    html.push_str("</body>\n</html>\n");

    html
}

/// Generate the CSS stylesheet for the editor.
/// When `include_layout` is true, appends Tier 2 layout/widget rules.
/// Any `custom_css` strings are appended inside a `/* --- Custom CSS --- */` block.
pub fn generate_editor_css(
    theme: &str,
    accent: &str,
    include_layout: bool,
    custom_css: &[&str],
) -> String {
    let (bg, fg, knob_bg, knob_track) = if theme == "light" {
        ("#f5f5f5", "#1a1a1a", "#e0e0e0", "#c0c0c0")
    } else {
        ("#1a1a1a", "#e0e0e0", "#2a2a2a", "#3a3a3a")
    };

    let mut css = format!(
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
    );

    if include_layout {
        css.push_str(generate_layout_css());
    }

    if !custom_css.is_empty() {
        css.push_str("\n\n/* --- Custom CSS --- */\n");
        for block in custom_css {
            css.push_str(block);
            css.push('\n');
        }
    }

    css
}

/// Generate the JavaScript for knob widgets and the parameter bridge.
pub fn generate_editor_js(params: &[ParamInfo]) -> String {
    let mut js = String::new();

    // Knob widget class
    js.push_str(KNOB_CLASS_JS);
    js.push('\n');

    // Advanced widget classes
    js.push_str(XY_PAD_CLASS_JS);
    js.push('\n');
    js.push_str(SPECTRUM_CLASS_JS);
    js.push('\n');
    js.push_str(WAVEFORM_CLASS_JS);
    js.push('\n');
    js.push_str(ENVELOPE_CLASS_JS);
    js.push('\n');
    js.push_str(EQ_CURVE_CLASS_JS);
    js.push('\n');
    js.push_str(REDUCTION_CLASS_JS);
    js.push('\n');

    // Parameter bridge
    js.push_str(PARAM_BRIDGE_JS);
    js.push('\n');

    // Instantiate knobs + bind all interactive widgets on DOMContentLoaded
    js.push_str("// --- Widget initialization ---\n");
    js.push_str("const knobs = {};\n");
    js.push_str("document.addEventListener('DOMContentLoaded', () => {\n");

    // Knob instances
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

    // Slider IPC binding
    js.push_str(SLIDER_IPC_JS);
    js.push('\n');

    // Switch IPC binding
    js.push_str(SWITCH_IPC_JS);
    js.push('\n');

    // XY pad instances
    js.push_str(XY_PAD_INIT_JS);
    js.push('\n');

    // Visualization widget instances
    js.push_str(VIS_INIT_JS);
    js.push('\n');

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

// ── Slider/Switch IPC binding ────────────────────────────────

const SLIDER_IPC_JS: &str = r#"  // --- Slider IPC binding ---
  document.querySelectorAll('.slider-cell[data-param]').forEach(cell => {
    const paramId = cell.getAttribute('data-param');
    const input = cell.querySelector('input[type="range"]');
    const display = cell.querySelector('.slider-value');
    if (input) {
      input.addEventListener('input', () => {
        const min = parseFloat(input.min);
        const max = parseFloat(input.max);
        const val = parseFloat(input.value);
        const normalized = (max - min) > 0 ? (val - min) / (max - min) : 0;
        if (display) display.textContent = input.step === '1' ? val.toString() : val.toFixed(2);
        sendParam(paramId, normalized);
      });
    }
  });"#;

const SWITCH_IPC_JS: &str = r#"  // --- Switch IPC binding ---
  document.querySelectorAll('.switch-cell[data-param]').forEach(cell => {
    const paramId = cell.getAttribute('data-param');
    const input = cell.querySelector('input[type="checkbox"]');
    if (input) {
      input.addEventListener('change', () => {
        sendParam(paramId, input.checked ? 1.0 : 0.0);
      });
    }
  });"#;

// ── Advanced widget JS classes ───────────────────────────────

const XY_PAD_CLASS_JS: &str = r#"// --- MuseXyPad canvas widget ---
class MuseXyPad {
  constructor(canvas, paramX, paramY) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.paramX = paramX;
    this.paramY = paramY;
    this.x = 0.5;
    this.y = 0.5;
    this.dragging = false;

    const style = getComputedStyle(document.documentElement);
    this.accent = style.getPropertyValue('--accent').trim();
    this.bg = style.getPropertyValue('--knob-bg').trim();
    this.track = style.getPropertyValue('--knob-track').trim();

    this.draw();
    this.bindEvents();
  }

  draw() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    ctx.clearRect(0, 0, w, h);

    // Crosshair guides
    const px = this.x * w;
    const py = (1 - this.y) * h;
    ctx.strokeStyle = this.track;
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(px, 0); ctx.lineTo(px, h); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(0, py); ctx.lineTo(w, py); ctx.stroke();
    ctx.setLineDash([]);

    // Position dot
    ctx.beginPath();
    ctx.arc(px, py, 8, 0, 2 * Math.PI);
    ctx.fillStyle = this.accent;
    ctx.fill();
    ctx.strokeStyle = 'rgba(255,255,255,0.3)';
    ctx.lineWidth = 2;
    ctx.stroke();
  }

  update(e) {
    const rect = this.canvas.getBoundingClientRect();
    this.x = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    this.y = Math.max(0, Math.min(1, 1 - (e.clientY - rect.top) / rect.height));
    this.draw();
    sendParam(this.paramX, this.x);
    sendParam(this.paramY, this.y);
  }

  bindEvents() {
    this.canvas.addEventListener('mousedown', (e) => { this.dragging = true; this.update(e); });
    document.addEventListener('mousemove', (e) => { if (this.dragging) this.update(e); });
    document.addEventListener('mouseup', () => { this.dragging = false; });
  }
}"#;

const SPECTRUM_CLASS_JS: &str = r#"// --- MuseSpectrum canvas widget ---
class MuseSpectrum {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    const style = getComputedStyle(document.documentElement);
    this.accent = style.getPropertyValue('--accent').trim();
    this.bg = style.getPropertyValue('--knob-bg').trim();
    this.bars = new Float32Array(32);
    for (let i = 0; i < 32; i++) this.bars[i] = Math.random() * 0.6 + 0.1;
    this.animate();
  }

  animate() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    ctx.clearRect(0, 0, w, h);

    const barW = w / this.bars.length;
    for (let i = 0; i < this.bars.length; i++) {
      this.bars[i] += (Math.random() - 0.5) * 0.08;
      this.bars[i] = Math.max(0.05, Math.min(1, this.bars[i]));
      const barH = this.bars[i] * h;
      ctx.fillStyle = this.accent;
      ctx.globalAlpha = 0.7 + this.bars[i] * 0.3;
      ctx.fillRect(i * barW + 1, h - barH, barW - 2, barH);
    }
    ctx.globalAlpha = 1;
    requestAnimationFrame(() => this.animate());
  }
}"#;

const WAVEFORM_CLASS_JS: &str = r#"// --- MuseWaveform canvas widget ---
class MuseWaveform {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    const style = getComputedStyle(document.documentElement);
    this.accent = style.getPropertyValue('--accent').trim();
    this.phase = 0;
    this.animate();
  }

  animate() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    ctx.clearRect(0, 0, w, h);

    ctx.strokeStyle = this.accent;
    ctx.lineWidth = 2;
    ctx.beginPath();
    for (let x = 0; x < w; x++) {
      const t = x / w * 4 * Math.PI + this.phase;
      const y = h / 2 + Math.sin(t) * (h * 0.35) + Math.sin(t * 2.7) * (h * 0.1);
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();
    this.phase += 0.03;
    requestAnimationFrame(() => this.animate());
  }
}"#;

const ENVELOPE_CLASS_JS: &str = r#"// --- MuseEnvelope canvas widget ---
class MuseEnvelope {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    const style = getComputedStyle(document.documentElement);
    this.accent = style.getPropertyValue('--accent').trim();
    this.track = style.getPropertyValue('--knob-track').trim();
    this.draw();
  }

  draw() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    ctx.clearRect(0, 0, w, h);

    // Static ADSR shape
    const a = 0.15, d = 0.25, s = 0.6, r = 0.35;
    const points = [
      [0, h],
      [a * w, h * 0.05],
      [(a + d) * w, h * (1 - s)],
      [(1 - r) * w, h * (1 - s)],
      [w, h],
    ];

    // Filled area
    ctx.beginPath();
    ctx.moveTo(points[0][0], points[0][1]);
    for (const [x, y] of points) ctx.lineTo(x, y);
    ctx.closePath();
    ctx.fillStyle = this.accent;
    ctx.globalAlpha = 0.15;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Outline
    ctx.beginPath();
    ctx.moveTo(points[0][0], points[0][1]);
    for (const [x, y] of points) ctx.lineTo(x, y);
    ctx.strokeStyle = this.accent;
    ctx.lineWidth = 2;
    ctx.stroke();

    // Phase labels
    ctx.fillStyle = this.track;
    ctx.font = '10px sans-serif';
    ctx.fillText('A', a * w * 0.4, h - 4);
    ctx.fillText('D', (a + d * 0.4) * w, h - 4);
    ctx.fillText('S', (a + d + (1 - r - a - d) * 0.4) * w, h - 4);
    ctx.fillText('R', (1 - r * 0.5) * w, h - 4);
  }
}"#;

const EQ_CURVE_CLASS_JS: &str = r#"// --- MuseEqCurve canvas widget ---
class MuseEqCurve {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    const style = getComputedStyle(document.documentElement);
    this.accent = style.getPropertyValue('--accent').trim();
    this.track = style.getPropertyValue('--knob-track').trim();
    this.draw();
  }

  draw() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    ctx.clearRect(0, 0, w, h);

    // Grid lines
    ctx.strokeStyle = this.track;
    ctx.lineWidth = 0.5;
    ctx.globalAlpha = 0.3;
    for (let y = 0; y < h; y += h / 4) {
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke();
    }
    ctx.globalAlpha = 1;

    // 0 dB reference line
    ctx.strokeStyle = this.track;
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(0, h / 2); ctx.lineTo(w, h / 2); ctx.stroke();
    ctx.setLineDash([]);

    // Mock EQ curve (low shelf + parametric peak + high shelf)
    ctx.strokeStyle = this.accent;
    ctx.lineWidth = 2;
    ctx.beginPath();
    for (let x = 0; x < w; x++) {
      const t = x / w;
      const lowShelf = 3 * Math.exp(-t * 8);
      const peak = 6 * Math.exp(-Math.pow((t - 0.4) * 8, 2));
      const highShelf = -2 * (1 - Math.exp(-(t - 0.7) * 5));
      const db = lowShelf + peak + highShelf;
      const y = h / 2 - (db / 12) * (h / 2);
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();

    // Filled area
    ctx.beginPath();
    for (let x = 0; x < w; x++) {
      const t = x / w;
      const lowShelf = 3 * Math.exp(-t * 8);
      const peak = 6 * Math.exp(-Math.pow((t - 0.4) * 8, 2));
      const highShelf = -2 * (1 - Math.exp(-(t - 0.7) * 5));
      const db = lowShelf + peak + highShelf;
      const y = h / 2 - (db / 12) * (h / 2);
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.lineTo(w, h / 2);
    ctx.lineTo(0, h / 2);
    ctx.closePath();
    ctx.fillStyle = this.accent;
    ctx.globalAlpha = 0.1;
    ctx.fill();
    ctx.globalAlpha = 1;
  }
}"#;

const REDUCTION_CLASS_JS: &str = r#"// --- MuseReduction bar widget ---
class MuseReduction {
  constructor(svgEl) {
    this.svg = svgEl;
    this.fill = svgEl.querySelector('.reduction-fill');
    if (!this.fill) return;
    this.value = 0;
    this.animate();
  }

  animate() {
    // Simulate gain reduction bouncing
    this.value += (Math.random() - 0.45) * 0.1;
    this.value = Math.max(0, Math.min(0.8, this.value));
    const h = this.value * 100;
    this.fill.setAttribute('height', h.toFixed(1));
    this.fill.setAttribute('y', (100 - h).toFixed(1));
    requestAnimationFrame(() => this.animate());
  }
}"#;

// ── Widget initialization JS (inside DOMContentLoaded) ───────

const XY_PAD_INIT_JS: &str = r#"  // --- XY Pad instances ---
  document.querySelectorAll('.xy-pad-cell').forEach(cell => {
    const canvas = cell.querySelector('canvas');
    const px = cell.getAttribute('data-param-x');
    const py = cell.getAttribute('data-param-y');
    if (canvas && px && py) new MuseXyPad(canvas, px, py);
  });"#;

const VIS_INIT_JS: &str = r#"  // --- Visualization widget instances ---
  document.querySelectorAll('.spectrum-cell canvas').forEach(c => new MuseSpectrum(c));
  document.querySelectorAll('.waveform-cell canvas').forEach(c => new MuseWaveform(c));
  document.querySelectorAll('.envelope-cell canvas').forEach(c => new MuseEnvelope(c));
  document.querySelectorAll('.eq-curve-cell canvas').forEach(c => new MuseEqCurve(c));
  document.querySelectorAll('.reduction-cell .reduction-meter').forEach(s => new MuseReduction(s));"#;

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
        let css = generate_editor_css("dark", "#E8A87C", false, &[]);
        assert!(css.contains("--bg: #1a1a1a"));
        assert!(css.contains("--fg: #e0e0e0"));
        assert!(css.contains("--accent: #E8A87C"));
        assert!(css.contains("--knob-bg: #2a2a2a"));
    }

    #[test]
    fn gui_css_light_theme() {
        let css = generate_editor_css("light", "#3366FF", false, &[]);
        assert!(css.contains("--bg: #f5f5f5"));
        assert!(css.contains("--fg: #1a1a1a"));
        assert!(css.contains("--accent: #3366FF"));
        assert!(css.contains("--knob-bg: #e0e0e0"));
    }

    #[test]
    fn gui_css_dark_vs_light_differs() {
        let dark = generate_editor_css("dark", "#E8A87C", false, &[]);
        let light = generate_editor_css("light", "#E8A87C", false, &[]);
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

    // ── Tier detection ───────────────────────────────────────

    #[test]
    fn gui_is_tier2_with_layout() {
        let gui = GuiBlock {
            items: vec![
                (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                (
                    GuiItem::Layout(LayoutDecl {
                        direction: LayoutDirection::Horizontal,
                        children: vec![],
                        span: Span::new(0, 0),
                    }),
                    Span::new(0, 0),
                ),
            ],
            span: Span::new(0, 0),
        };
        assert!(is_tier2(&gui));
    }

    #[test]
    fn gui_is_tier2_with_widget() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Knob,
                    param_name: Some("gain".to_string()),
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        assert!(is_tier2(&gui));
    }

    #[test]
    fn gui_is_not_tier2_theme_only() {
        let gui = dark_gui("#E8A87C");
        assert!(!is_tier2(&gui));
    }

    // ── gui_size ─────────────────────────────────────────────

    #[test]
    fn gui_size_default_when_absent() {
        let gui = dark_gui("#E8A87C");
        assert_eq!(gui_size(&gui), (600, 400));
    }

    #[test]
    fn gui_size_custom() {
        let gui = GuiBlock {
            items: vec![
                (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                (GuiItem::Size(800, 600), Span::new(0, 0)),
            ],
            span: Span::new(0, 0),
        };
        assert_eq!(gui_size(&gui), (800, 600));
    }

    // ── Tier 2 HTML ──────────────────────────────────────────

    fn make_tier2_gui() -> GuiBlock {
        GuiBlock {
            items: vec![
                (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                (GuiItem::Accent("#FF6633".to_string()), Span::new(0, 0)),
                (
                    GuiItem::Layout(LayoutDecl {
                        direction: LayoutDirection::Horizontal,
                        children: vec![
                            (
                                GuiItem::Panel(PanelDecl {
                                    title: "Main".to_string(),
                                    children: vec![(
                                        GuiItem::Widget(WidgetDecl {
                                            widget_type: WidgetType::Knob,
                                            param_name: Some("gain".to_string()),
                                            param_name_y: None,
                                            label_text: None,
                                            props: vec![],
                                            span: Span::new(0, 0),
                                        }),
                                        Span::new(0, 0),
                                    )],
                                    span: Span::new(0, 0),
                                }),
                                Span::new(0, 0),
                            ),
                            (
                                GuiItem::Widget(WidgetDecl {
                                    widget_type: WidgetType::Slider,
                                    param_name: Some("mix".to_string()),
                                    param_name_y: None,
                                    label_text: None,
                                    props: vec![],
                                    span: Span::new(0, 0),
                                }),
                                Span::new(0, 0),
                            ),
                        ],
                        span: Span::new(0, 0),
                    }),
                    Span::new(0, 0),
                ),
            ],
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn gui_tier2_html_has_layout_divs() {
        let plugin = make_plugin(
            "Tier2Test",
            vec![
                make_float_param("gain", 0.0, -30.0, 30.0, "dB"),
                make_float_param("mix", 0.5, 0.0, 1.0, ""),
            ],
            Some(make_tier2_gui()),
        );
        let html = generate_editor_html(&plugin);

        // Should have Tier 2 structure, not Tier 1 knob-grid
        assert!(html.contains("class=\"tier2-root\""));
        assert!(html.contains("class=\"layout layout-horizontal\""));
        assert!(html.contains("class=\"panel\""));
        assert!(html.contains("class=\"panel-title\">Main</h2>"));
        assert!(!html.contains("class=\"knob-grid\""));
    }

    #[test]
    fn gui_tier2_html_has_knob_widget() {
        let plugin = make_plugin(
            "Tier2Test",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            Some(make_tier2_gui()),
        );
        let html = generate_editor_html(&plugin);

        // Knob widget should use same canvas pattern as Tier 1
        assert!(html.contains("id=\"knob-gain\" width=\"80\" height=\"80\""));
        assert!(html.contains("data-param=\"gain\""));
    }

    #[test]
    fn gui_tier2_html_has_slider_widget() {
        let plugin = make_plugin(
            "Tier2Test",
            vec![
                make_float_param("gain", 0.0, -30.0, 30.0, "dB"),
                make_float_param("mix", 0.5, 0.0, 1.0, ""),
            ],
            Some(make_tier2_gui()),
        );
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"slider-cell\""));
        assert!(html.contains("id=\"slider-mix\""));
        assert!(html.contains("type=\"range\""));
    }

    #[test]
    fn gui_tier2_css_includes_layout_rules() {
        let css = generate_editor_css("dark", "#E8A87C", true, &[]);
        assert!(css.contains(".layout-horizontal"));
        assert!(css.contains(".layout-vertical"));
        assert!(css.contains(".layout-grid"));
        assert!(css.contains(".panel {"));
        assert!(css.contains(".panel-title"));
        assert!(css.contains(".slider-cell"));
        assert!(css.contains(".switch-cell"));
    }

    #[test]
    fn gui_tier1_css_omits_layout_rules() {
        let css = generate_editor_css("dark", "#E8A87C", false, &[]);
        assert!(!css.contains(".layout-horizontal"));
        assert!(!css.contains(".panel {"));
    }

    // ── Tier 3 CSS injection ─────────────────────────────────

    #[test]
    fn gui_tier3_css_injection() {
        let custom = ".my-knob { animation: glow 2s infinite; }";
        let css = generate_editor_css("dark", "#E8A87C", true, &[custom]);
        assert!(css.contains("/* --- Custom CSS --- */"));
        assert!(css.contains(".my-knob { animation: glow 2s infinite; }"));
    }

    #[test]
    fn gui_tier3_css_multiple_blocks() {
        let css = generate_editor_css("dark", "#E8A87C", false, &[
            ":root { --glow: #ff0; }",
            ".knob-cell:hover { filter: brightness(1.2); }",
        ]);
        assert!(css.contains("/* --- Custom CSS --- */"));
        assert!(css.contains(":root { --glow: #ff0; }"));
        assert!(css.contains(".knob-cell:hover { filter: brightness(1.2); }"));
    }

    #[test]
    fn gui_tier3_full_html_with_css() {
        let gui = GuiBlock {
            items: vec![
                (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                (GuiItem::Accent("#E8A87C".to_string()), Span::new(0, 0)),
                (
                    GuiItem::Css(".custom { color: red; }".to_string()),
                    Span::new(0, 0),
                ),
            ],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin(
            "CssTest",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            Some(gui),
        );
        let html = generate_editor_html(&plugin);

        // Tier 1 (no Layout/Widget/Panel), but CSS should still be injected
        assert!(html.contains("class=\"knob-grid\""));
        assert!(html.contains("/* --- Custom CSS --- */"));
        assert!(html.contains(".custom { color: red; }"));
    }

    // ── Widget prop tests ────────────────────────────────────

    #[test]
    fn gui_widget_class_and_style_props() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Knob,
                    param_name: Some("gain".to_string()),
                    param_name_y: None,
                    label_text: None,
                    props: vec![
                        WidgetProp::Class("hero-knob".to_string()),
                        WidgetProp::Style("vintage".to_string()),
                    ],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin(
            "PropTest",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            Some(gui),
        );
        let html = generate_editor_html(&plugin);

        assert!(html.contains("knob-cell hero-knob"));
        assert!(html.contains("data-style=\"vintage\""));
    }

    #[test]
    fn gui_label_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Label,
                    param_name: None,
                    param_name_y: None,
                    label_text: Some("Output Level".to_string()),
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("LabelTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"label-widget\""));
        assert!(html.contains("Output Level"));
    }

    #[test]
    fn gui_switch_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Switch,
                    param_name: Some("bypass".to_string()),
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("SwitchTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"switch-cell\""));
        assert!(html.contains("id=\"switch-bypass\""));
        assert!(html.contains("type=\"checkbox\""));
    }

    #[test]
    fn gui_value_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Value,
                    param_name: Some("gain".to_string()),
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin(
            "ValueTest",
            vec![make_float_param("gain", 0.0, -30.0, 30.0, "dB")],
            Some(gui),
        );
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"value-widget\""));
        assert!(html.contains("id=\"value-gain\""));
    }

    #[test]
    fn gui_meter_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Meter,
                    param_name: Some("level".to_string()),
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("MeterTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"meter-cell\""));
        assert!(html.contains("id=\"meter-level\""));
        assert!(html.contains("<svg"));
    }

    // ── Advanced widget HTML tests ───────────────────────────

    #[test]
    fn gui_xy_pad_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::XyPad,
                    param_name: Some("cutoff".to_string()),
                    param_name_y: Some("resonance".to_string()),
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("XyTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"xy-pad-cell\""), "XY pad cell class");
        assert!(html.contains("data-param-x=\"cutoff\""), "X param attribute");
        assert!(html.contains("data-param-y=\"resonance\""), "Y param attribute");
        assert!(html.contains("id=\"xy-pad-cutoff-resonance\""), "Canvas id");
        assert!(html.contains("width=\"200\" height=\"200\""), "Canvas size");
    }

    #[test]
    fn gui_xy_pad_with_props() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::XyPad,
                    param_name: Some("x".to_string()),
                    param_name_y: Some("y".to_string()),
                    label_text: None,
                    props: vec![WidgetProp::Class("big-pad".to_string())],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("XyPropTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("xy-pad-cell big-pad"), "Extra class on XY pad");
    }

    #[test]
    fn gui_spectrum_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Spectrum,
                    param_name: None,
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("SpectrumTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"spectrum-cell\""), "Spectrum cell class");
        assert!(html.contains("id=\"spectrum-display\""), "Spectrum canvas id");
        assert!(html.contains("width=\"300\" height=\"150\""), "Canvas size");
    }

    #[test]
    fn gui_waveform_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Waveform,
                    param_name: None,
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("WaveformTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"waveform-cell\""), "Waveform cell class");
        assert!(html.contains("id=\"waveform-display\""), "Waveform canvas id");
    }

    #[test]
    fn gui_envelope_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Envelope,
                    param_name: None,
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("EnvelopeTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"envelope-cell\""), "Envelope cell class");
        assert!(html.contains("id=\"envelope-display\""), "Envelope canvas id");
    }

    #[test]
    fn gui_eq_curve_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::EqCurve,
                    param_name: None,
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("EqCurveTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"eq-curve-cell\""), "EQ curve cell class");
        assert!(html.contains("id=\"eq-curve-display\""), "EQ curve canvas id");
    }

    #[test]
    fn gui_reduction_widget_html() {
        let gui = GuiBlock {
            items: vec![(
                GuiItem::Widget(WidgetDecl {
                    widget_type: WidgetType::Reduction,
                    param_name: None,
                    param_name_y: None,
                    label_text: None,
                    props: vec![],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let plugin = make_plugin("ReductionTest", vec![], Some(gui));
        let html = generate_editor_html(&plugin);

        assert!(html.contains("class=\"reduction-cell\""), "Reduction cell class");
        assert!(html.contains("class=\"reduction-meter\""), "Reduction SVG");
        assert!(html.contains("class=\"reduction-fill\""), "Reduction fill rect");
        assert!(html.contains("GR"), "Gain reduction label");
    }

    // ── JS class tests ───────────────────────────────────────

    #[test]
    fn gui_js_contains_advanced_widget_classes() {
        let js = generate_editor_js(&[]);
        assert!(js.contains("class MuseXyPad"), "XY pad class");
        assert!(js.contains("class MuseSpectrum"), "Spectrum class");
        assert!(js.contains("class MuseWaveform"), "Waveform class");
        assert!(js.contains("class MuseEnvelope"), "Envelope class");
        assert!(js.contains("class MuseEqCurve"), "EQ curve class");
        assert!(js.contains("class MuseReduction"), "Reduction class");
    }

    #[test]
    fn gui_js_slider_ipc_binding() {
        let js = generate_editor_js(&[]);
        assert!(js.contains(".slider-cell[data-param]"), "Slider selector");
        assert!(js.contains("input[type=\"range\"]"), "Range input query");
        assert!(js.contains("sendParam(paramId, normalized)"), "Slider sends normalized");
    }

    #[test]
    fn gui_js_switch_ipc_binding() {
        let js = generate_editor_js(&[]);
        assert!(js.contains(".switch-cell[data-param]"), "Switch selector");
        assert!(js.contains("input[type=\"checkbox\"]"), "Checkbox query");
        assert!(js.contains("input.checked ? 1.0 : 0.0"), "Switch sends 0/1");
    }

    #[test]
    fn gui_js_vis_init() {
        let js = generate_editor_js(&[]);
        assert!(js.contains("new MuseSpectrum(c)"), "Spectrum init");
        assert!(js.contains("new MuseWaveform(c)"), "Waveform init");
        assert!(js.contains("new MuseEnvelope(c)"), "Envelope init");
        assert!(js.contains("new MuseEqCurve(c)"), "EQ curve init");
        assert!(js.contains("new MuseReduction(s)"), "Reduction init");
    }

    #[test]
    fn gui_js_xy_pad_init() {
        let js = generate_editor_js(&[]);
        assert!(js.contains(".xy-pad-cell"), "XY pad init selector");
        assert!(js.contains("new MuseXyPad(canvas, px, py)"), "XY pad init");
    }

    // ── CSS tests for new widgets ────────────────────────────

    #[test]
    fn gui_css_includes_advanced_widget_rules() {
        let css = generate_editor_css("dark", "#E8A87C", true, &[]);
        assert!(css.contains(".xy-pad-cell"), "XY pad CSS");
        assert!(css.contains(".spectrum-cell"), "Spectrum CSS");
        assert!(css.contains(".reduction-cell"), "Reduction CSS");
        assert!(css.contains(".reduction-fill"), "Reduction fill CSS");
        assert!(css.contains(".reduction-label"), "Reduction label CSS");
    }
}
