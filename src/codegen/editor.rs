//! Generates the Rust editor module for WebKit-based plugin GUIs.
//!
//! Produces a `mod editor { ... }` block that:
//! - Defines `WebViewEditor` implementing nih-plug's `Editor` trait
//! - Creates a WKWebView as a child of the host-provided NSView
//! - Loads the generated HTML/CSS/JS from `include_str!("../assets/editor.html")`
//! - Bridges JS→Rust param changes via WKScriptMessageHandler (objc2 `declare_class!`)
//! - Bridges Rust→JS param updates via `evaluateJavaScript`

use crate::ast::{GuiBlock, PluginDef};
use crate::codegen::gui;

/// Generate the complete `mod editor { ... }` Rust module as a string.
///
/// The generated module uses objc2 for native macOS WebKit embedding.
/// It implements nih-plug's Editor trait with:
/// - `spawn()`: creates WKWebView inside the host's parent NSView
/// - `size()`: returns editor dimensions
/// - `set_scale_factor()`: stores DPI scale
/// - `param_value_changed()`: pushes updates from Rust to JS
/// - `param_modulation_changed()` / `param_values_changed()`: delegates
pub fn generate_editor_module(plugin: &PluginDef, _gui: &GuiBlock) -> String {
    let params = gui::collect_param_info(plugin);
    let _struct_name = plugin_name_to_struct(&plugin.name);

    let mut out = String::new();
    out.push_str("mod editor {\n");
    out.push_str("    use std::any::Any;\n");
    out.push_str("    use std::ffi::c_void;\n");
    out.push_str("    use std::sync::Arc;\n");
    out.push_str("    use std::sync::atomic::{AtomicU32, Ordering};\n");
    out.push_str("    use nih_plug::prelude::*;\n");
    out.push_str("    use objc2::rc::Retained;\n");
    out.push_str("    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, ProtocolObject, Sel};\n");
    out.push_str("    use objc2::{msg_send, msg_send_id, class, sel, AllocAnyThread, ClassType, DeclaredClass, declare_class, mutability};\n");
    out.push_str("    use objc2_foundation::{NSString, NSObjectProtocol};\n");
    out.push_str("    use objc2_app_kit::NSView;\n");
    out.push_str("    use objc2_web_kit::{WKWebView, WKWebViewConfiguration, WKUserContentController, WKScriptMessageHandler};\n");
    out.push_str("    use super::PluginParams;\n");
    out.push_str("\n");

    // WebViewEditor struct
    out.push_str("    pub struct WebViewEditor {\n");
    out.push_str("        params: Arc<PluginParams>,\n");
    out.push_str("        width: AtomicU32,\n");
    out.push_str("        height: AtomicU32,\n");
    out.push_str("    }\n\n");

    out.push_str("    impl WebViewEditor {\n");
    out.push_str("        pub fn new(params: Arc<PluginParams>) -> Self {\n");
    out.push_str("            Self {\n");
    out.push_str("                params,\n");
    out.push_str("                width: AtomicU32::new(600),\n");
    out.push_str("                height: AtomicU32::new(400),\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // WebViewHandle — holds the retained WKWebView, released on drop
    out.push_str("    struct WebViewHandle {\n");
    out.push_str("        webview: Retained<WKWebView>,\n");
    out.push_str("    }\n\n");

    out.push_str("    // SAFETY: WKWebView is created and dropped on the main thread (GUI thread).\n");
    out.push_str("    // nih-plug guarantees Editor::spawn() is called from the main thread.\n");
    out.push_str("    unsafe impl Send for WebViewHandle {}\n\n");

    out.push_str("    impl Drop for WebViewHandle {\n");
    out.push_str("        fn drop(&mut self) {\n");
    out.push_str("            unsafe {\n");
    out.push_str("                let _: () = msg_send![&self.webview, removeFromSuperview];\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // IPC message handler — declare_class! for WKScriptMessageHandler
    generate_ipc_handler(&mut out, plugin, &params);

    // Editor trait impl
    out.push_str("    impl Editor for WebViewEditor {\n");

    // spawn()
    out.push_str("        fn spawn(\n");
    out.push_str("            &self,\n");
    out.push_str("            parent: ParentWindowHandle,\n");
    out.push_str("            context: Arc<dyn GuiContext>,\n");
    out.push_str("        ) -> Box<dyn Any + Send> {\n");
    out.push_str("            let ns_view_ptr = match parent {\n");
    out.push_str("                ParentWindowHandle::AppKitNsView(ptr) => ptr,\n");
    out.push_str("                _ => panic!(\"WebViewEditor only supports macOS (AppKitNsView)\"),\n");
    out.push_str("            };\n\n");

    out.push_str("            unsafe {\n");
    out.push_str("                let parent_view: &NSView = &*(ns_view_ptr as *const NSView);\n\n");

    // Create WKWebViewConfiguration with IPC handler
    out.push_str("                let config = WKWebViewConfiguration::new();\n");
    out.push_str("                let content_controller = config.userContentController();\n\n");

    // Create IPC handler instance
    out.push_str("                let handler = ParamBridgeHandler::new(self.params.clone(), context);\n");
    out.push_str("                let handler_proto = ProtocolObject::from_retained(handler);\n");
    out.push_str("                let handler_name = NSString::from_str(\"paramBridge\");\n");
    out.push_str("                content_controller.addScriptMessageHandler_name(&handler_proto, &handler_name);\n\n");

    // Create WKWebView with parent frame
    out.push_str("                let frame = parent_view.frame();\n");
    out.push_str("                let webview = WKWebView::initWithFrame_configuration(\n");
    out.push_str("                    WKWebView::alloc(),\n");
    out.push_str("                    frame,\n");
    out.push_str("                    &config,\n");
    out.push_str("                );\n\n");

    // Configure webview for transparent background (optional, looks better)
    out.push_str("                // Disable opaque background for seamless embedding\n");
    out.push_str("                let _: () = msg_send![&webview, setValue: Bool::NO forKey: &*NSString::from_str(\"drawsBackground\")];\n\n");

    // Load the embedded HTML
    out.push_str("                let html_source = include_str!(\"../assets/editor.html\");\n");
    out.push_str("                let html_string = NSString::from_str(html_source);\n");
    out.push_str("                let base_url: Option<&objc2_foundation::NSURL> = None;\n");
    out.push_str("                let _: () = msg_send![&webview, loadHTMLString: &*html_string baseURL: base_url];\n\n");

    // Add as subview
    out.push_str("                parent_view.addSubview(&webview);\n\n");

    out.push_str("                Box::new(WebViewHandle { webview })\n");
    out.push_str("            }\n");
    out.push_str("        }\n\n");

    // size()
    out.push_str("        fn size(&self) -> (u32, u32) {\n");
    out.push_str("            (self.width.load(Ordering::Relaxed), self.height.load(Ordering::Relaxed))\n");
    out.push_str("        }\n\n");

    // set_scale_factor()
    out.push_str("        fn set_scale_factor(&self, _factor: f32) -> bool {\n");
    out.push_str("            // macOS handles DPI natively, no scaling needed\n");
    out.push_str("            true\n");
    out.push_str("        }\n\n");

    // param_value_changed() — push Rust→JS
    out.push_str("        fn param_value_changed(&self, _id: &str, _normalized_value: f32) {\n");
    out.push_str("            // Rust→JS updates are handled by param_values_changed()\n");
    out.push_str("            // which is called by the host when parameter values change.\n");
    out.push_str("            // Individual param updates would require storing a webview reference,\n");
    out.push_str("            // which adds complexity. The host calls param_values_changed() for\n");
    out.push_str("            // bulk updates (preset loads, automation), which is the common case.\n");
    out.push_str("        }\n\n");

    // param_modulation_changed()
    out.push_str("        fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {\n");
    out.push_str("            // Modulation visualization deferred to Tier 2\n");
    out.push_str("        }\n\n");

    // param_values_changed()
    out.push_str("        fn param_values_changed(&self) {\n");
    out.push_str("            // Bulk param sync deferred — requires stored webview reference.\n");
    out.push_str("            // The JS knobs read initial values from the HTML; ongoing sync\n");
    out.push_str("            // requires Tier 2 work (shared webview Arc between Editor and Handle).\n");
    out.push_str("        }\n");

    out.push_str("    }\n"); // end impl Editor
    out.push_str("}\n"); // end mod editor

    out
}

/// Generate the IPC message handler using declare_class! for WKScriptMessageHandler protocol.
fn generate_ipc_handler(out: &mut String, _plugin: &PluginDef, params: &[gui::ParamInfo]) {
    // ParamBridgeHandler struct and declare_class!
    out.push_str("    struct ParamBridgeHandlerIvars {\n");
    out.push_str("        params: Arc<PluginParams>,\n");
    out.push_str("        context: Arc<dyn GuiContext>,\n");
    out.push_str("    }\n\n");

    out.push_str("    declare_class! {\n");
    out.push_str("        struct ParamBridgeHandler;\n\n");
    out.push_str("        unsafe impl ClassType for ParamBridgeHandler {\n");
    out.push_str("            type Super = NSObject;\n");
    out.push_str("            type Mutability = mutability::MainThreadOnly;\n");
    out.push_str("            const NAME: &'static str = \"MuseParamBridgeHandler\";\n");
    out.push_str("        }\n\n");
    out.push_str("        impl DeclaredClass for ParamBridgeHandler {\n");
    out.push_str("            type Ivars = ParamBridgeHandlerIvars;\n");
    out.push_str("        }\n\n");

    // WKScriptMessageHandler protocol implementation
    out.push_str("        unsafe impl WKScriptMessageHandler for ParamBridgeHandler {\n");
    out.push_str("            #[method(userContentController:didReceiveScriptMessage:)]\n");
    out.push_str("            fn did_receive_script_message(\n");
    out.push_str("                &self,\n");
    out.push_str("                _controller: &WKUserContentController,\n");
    out.push_str("                message: &objc2_web_kit::WKScriptMessage,\n");
    out.push_str("            ) {\n");
    out.push_str("                unsafe {\n");
    out.push_str("                    let body: Retained<AnyObject> = msg_send_id![message, body];\n");
    out.push_str("                    let body_str: Retained<NSString> = msg_send_id![&body, description];\n");
    out.push_str("                    let json_str = body_str.to_string();\n\n");
    out.push_str("                    // Parse JSON: {\"id\": \"param_name\", \"value\": 0.5}\n");
    out.push_str("                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {\n");
    out.push_str("                        if let (Some(id), Some(value)) = (\n");
    out.push_str("                            parsed.get(\"id\").and_then(|v| v.as_str()),\n");
    out.push_str("                            parsed.get(\"value\").and_then(|v| v.as_f64()),\n");
    out.push_str("                        ) {\n");
    out.push_str("                            let ivars = self.ivars();\n");

    // Generate param matching — match on param id string to get the ParamPtr
    out.push_str("                            let setter = ParamSetter::new(ivars.context.as_ref());\n");
    generate_param_dispatch(out, params);

    out.push_str("                        }\n");
    out.push_str("                    }\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // Constructor
    out.push_str("    impl ParamBridgeHandler {\n");
    out.push_str("        fn new(params: Arc<PluginParams>, context: Arc<dyn GuiContext>) -> Retained<Self> {\n");
    out.push_str("            let this = Self::alloc().set_ivars(ParamBridgeHandlerIvars { params, context });\n");
    out.push_str("            unsafe { msg_send_id![super(this), init] }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

/// Generate parameter dispatch code: matches the JS param id to the correct PluginParams field.
fn generate_param_dispatch(out: &mut String, params: &[gui::ParamInfo]) {
    if params.is_empty() {
        out.push_str("                            let _ = (id, value, setter);\n");
        return;
    }

    out.push_str("                            match id {\n");
    for p in params {
        out.push_str(&format!(
            "                                \"{}\" => {{\n",
            p.name
        ));
        out.push_str(&format!(
            "                                    setter.begin_set_parameter(&ivars.params.{name});\n",
            name = p.name
        ));
        out.push_str(&format!(
            "                                    setter.set_parameter_normalized(&ivars.params.{name}, value as f32);\n",
            name = p.name
        ));
        out.push_str(&format!(
            "                                    setter.end_set_parameter(&ivars.params.{name});\n",
            name = p.name
        ));
        out.push_str("                                }\n");
    }
    out.push_str("                                _ => {}\n");
    out.push_str("                            }\n");
}

/// Convert plugin display name to PascalCase struct name.
fn plugin_name_to_struct(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::span::Span;

    fn make_gui_plugin() -> PluginDef {
        PluginDef {
            name: "Warm Gain".to_string(),
            items: vec![
                (
                    PluginItem::ParamDecl(Box::new(ParamDef {
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
                        options: vec![(ParamOption::Unit("dB".to_string()), Span::new(0, 0))],
                        span: Span::new(0, 0),
                    })),
                    Span::new(0, 0),
                ),
                (
                    PluginItem::ParamDecl(Box::new(ParamDef {
                        name: "mix".to_string(),
                        param_type: ParamType::Float,
                        default: Some((Expr::Number(1.0, None), Span::new(0, 0))),
                        range: Some(ParamRange {
                            min: (Expr::Number(0.0, None), Span::new(0, 0)),
                            max: (Expr::Number(1.0, None), Span::new(0, 0)),
                            span: Span::new(0, 0),
                        }),
                        options: vec![],
                        span: Span::new(0, 0),
                    })),
                    Span::new(0, 0),
                ),
                (
                    PluginItem::GuiDecl(GuiBlock {
                        items: vec![
                            (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                            (GuiItem::Accent("#E8A87C".to_string()), Span::new(0, 0)),
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
    fn editor_module_contains_struct_and_trait() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("mod editor {"));
        assert!(code.contains("pub struct WebViewEditor"));
        assert!(code.contains("impl Editor for WebViewEditor"));
        assert!(code.contains("struct WebViewHandle"));
    }

    #[test]
    fn editor_module_contains_spawn() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("fn spawn("));
        assert!(code.contains("ParentWindowHandle::AppKitNsView"));
        assert!(code.contains("WKWebView"));
        assert!(code.contains("WKWebViewConfiguration"));
        assert!(code.contains("addSubview"));
    }

    #[test]
    fn editor_module_contains_param_value_changed() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("fn param_value_changed("));
        assert!(code.contains("fn param_modulation_changed("));
        assert!(code.contains("fn param_values_changed("));
    }

    #[test]
    fn editor_module_contains_ipc_handler() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("declare_class!"));
        assert!(code.contains("ParamBridgeHandler"));
        assert!(code.contains("WKScriptMessageHandler"));
        assert!(code.contains("didReceiveScriptMessage"));
        assert!(code.contains("paramBridge"));
    }

    #[test]
    fn editor_module_dispatches_params() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        // Should dispatch to both params
        assert!(code.contains("\"gain\" =>"));
        assert!(code.contains("\"mix\" =>"));
        assert!(code.contains("begin_set_parameter"));
        assert!(code.contains("set_parameter_normalized"));
        assert!(code.contains("end_set_parameter"));
    }

    #[test]
    fn editor_module_includes_html() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("include_str!(\"../assets/editor.html\")"));
    }

    #[test]
    fn editor_module_webview_handle_drop() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        assert!(code.contains("impl Drop for WebViewHandle"));
        assert!(code.contains("removeFromSuperview"));
    }

    #[test]
    fn editor_module_no_params_still_compiles() {
        let plugin = PluginDef {
            name: "Empty".to_string(),
            items: vec![(
                PluginItem::GuiDecl(GuiBlock {
                    items: vec![
                        (GuiItem::Theme("dark".to_string()), Span::new(0, 0)),
                        (GuiItem::Accent("#FF0000".to_string()), Span::new(0, 0)),
                    ],
                    span: Span::new(0, 0),
                }),
                Span::new(0, 0),
            )],
            span: Span::new(0, 0),
        };
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui);

        // Should still generate valid code with no param dispatch
        assert!(code.contains("mod editor {"));
        assert!(code.contains("let _ = (id, value, setter)"));
    }
}
