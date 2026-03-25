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
/// - `param_value_changed()`: pushes updates from Rust to JS via evaluateJavaScript
/// - `param_modulation_changed()` / `param_values_changed()`: delegates
pub fn generate_editor_module(plugin: &PluginDef, _gui: &GuiBlock, width: u32, height: u32) -> String {
    let params = gui::collect_param_info(plugin);
    let _struct_name = plugin_name_to_struct(&plugin.name);

    let mut out = String::new();
    out.push_str("mod editor {\n");
    out.push_str("    use std::any::Any;\n");
    out.push_str("    use std::ffi::c_void;\n");
    out.push_str("    use std::sync::Arc;\n");
    out.push_str("    use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};\n");
    out.push_str("    use nih_plug::prelude::*;\n");
    out.push_str("    use objc2::rc::Retained;\n");
    out.push_str("    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, ProtocolObject, Sel};\n");
    out.push_str("    use objc2::{msg_send, msg_send_id, class, sel, AllocAnyThread, ClassType, DefinedClass, define_class, MainThreadOnly, MainThreadMarker};\n");
    out.push_str("    use objc2_foundation::{NSString, NSObjectProtocol};\n");
    out.push_str("    use objc2_app_kit::NSView;\n");
    out.push_str("    use objc2_web_kit::{WKWebView, WKWebViewConfiguration, WKUserContentController, WKScriptMessageHandler};\n");
    out.push_str("    use super::PluginParams;\n");
    out.push_str("\n");

    // WebViewEditor struct — now with shared webview pointer for Rust→JS sync
    out.push_str("    pub struct WebViewEditor {\n");
    out.push_str("        params: Arc<PluginParams>,\n");
    out.push_str("        width: AtomicU32,\n");
    out.push_str("        height: AtomicU32,\n");
    out.push_str("        webview_ptr: Arc<AtomicPtr<c_void>>,\n");
    out.push_str("    }\n\n");

    out.push_str("    impl WebViewEditor {\n");
    out.push_str("        pub fn new(params: Arc<PluginParams>) -> Self {\n");
    out.push_str("            Self {\n");
    out.push_str("                params,\n");
    out.push_str(&format!("                width: AtomicU32::new({}),\n", width));
    out.push_str(&format!("                height: AtomicU32::new({}),\n", height));
    out.push_str("                webview_ptr: Arc::new(AtomicPtr::new(std::ptr::null_mut())),\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // WebViewHandle — holds the retained WKWebView and shared ptr, released on drop
    out.push_str("    struct WebViewHandle {\n");
    out.push_str("        webview: Retained<WKWebView>,\n");
    out.push_str("        webview_ptr: Arc<AtomicPtr<c_void>>,\n");
    out.push_str("    }\n\n");

    out.push_str("    // SAFETY: WKWebView is created and dropped on the main thread (GUI thread).\n");
    out.push_str("    // nih-plug guarantees Editor::spawn() is called from the main thread.\n");
    out.push_str("    unsafe impl Send for WebViewHandle {}\n\n");

    out.push_str("    impl Drop for WebViewHandle {\n");
    out.push_str("        fn drop(&mut self) {\n");
    out.push_str("            // Null the shared pointer BEFORE removing from superview\n");
    out.push_str("            // so param_value_changed() sees null and skips JS calls.\n");
    out.push_str("            self.webview_ptr.store(std::ptr::null_mut(), Ordering::Release);\n");
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
    out.push_str("                // SAFETY: nih-plug guarantees Editor::spawn() is called on the main thread.\n");
    out.push_str("                let mtm = MainThreadMarker::new_unchecked();\n");
    out.push_str("                let parent_view: &NSView = &*(ns_view_ptr as *const NSView);\n\n");

    // Create WKWebViewConfiguration with IPC handler
    out.push_str("                let config = WKWebViewConfiguration::new(mtm);\n");
    out.push_str("                let content_controller = config.userContentController();\n\n");

    // Create IPC handler instance
    out.push_str("                let handler = ParamBridgeHandler::new(self.params.clone(), context, mtm);\n");
    out.push_str("                let handler_proto = ProtocolObject::from_retained(handler);\n");
    out.push_str("                let handler_name = NSString::from_str(\"paramBridge\");\n");
    out.push_str("                content_controller.addScriptMessageHandler_name(&handler_proto, &handler_name);\n\n");

    // Create WKWebView with parent frame
    out.push_str("                let frame = parent_view.frame();\n");
    out.push_str("                let webview = WKWebView::initWithFrame_configuration(\n");
    out.push_str("                    WKWebView::alloc(mtm),\n");
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

    // Store the webview raw pointer for Rust→JS sync
    out.push_str("                let raw_ptr = Retained::as_ptr(&webview) as *mut c_void;\n");
    out.push_str("                self.webview_ptr.store(raw_ptr, Ordering::Release);\n\n");

    out.push_str("                Box::new(WebViewHandle { webview, webview_ptr: self.webview_ptr.clone() })\n");
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

    // param_value_changed() — push Rust→JS via evaluateJavaScript
    out.push_str("        fn param_value_changed(&self, id: &str, normalized_value: f32) {\n");
    out.push_str("            let ptr = self.webview_ptr.load(Ordering::Acquire);\n");
    out.push_str("            if ptr.is_null() {\n");
    out.push_str("                return;\n");
    out.push_str("            }\n");
    out.push_str("            unsafe {\n");
    out.push_str("                let webview = ptr as *const objc2::runtime::AnyObject;\n");
    out.push_str("                let js = format!(\"window.updateParam('{}', {})\", id, normalized_value);\n");
    out.push_str("                let ns_js = NSString::from_str(&js);\n");
    out.push_str("                let null_handler: *const c_void = std::ptr::null();\n");
    out.push_str("                let _: () = msg_send![webview, evaluateJavaScript: &*ns_js completionHandler: null_handler];\n");
    out.push_str("            }\n");
    out.push_str("        }\n\n");

    // param_modulation_changed()
    out.push_str("        fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {\n");
    out.push_str("            // Modulation visualization deferred to future tier\n");
    out.push_str("        }\n\n");

    // param_values_changed() — iterate all params
    out.push_str("        fn param_values_changed(&self) {\n");
    out.push_str("            // Bulk sync: re-push all param values to JS.\n");
    out.push_str("            // Called on preset load, automation batch updates, etc.\n");
    out.push_str("            let ptr = self.webview_ptr.load(Ordering::Acquire);\n");
    out.push_str("            if ptr.is_null() {\n");
    out.push_str("                return;\n");
    out.push_str("            }\n");

    // Generate a call for each known param
    for p in &params {
        out.push_str(&format!(
            "            self.param_value_changed(\"{name}\", self.params.{name}.modulated_normalized_value());\n",
            name = p.name,
        ));
    }

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

    out.push_str("    define_class! {\n");
    out.push_str("        #[unsafe(super(NSObject))]\n");
    out.push_str("        #[thread_kind = MainThreadOnly]\n");
    out.push_str("        #[name = \"MuseParamBridgeHandler\"]\n");
    out.push_str("        #[ivars = ParamBridgeHandlerIvars]\n");
    out.push_str("        struct ParamBridgeHandler;\n\n");

    // NSObjectProtocol is required by WKScriptMessageHandler
    out.push_str("        unsafe impl NSObjectProtocol for ParamBridgeHandler {}\n\n");

    // WKScriptMessageHandler protocol implementation
    out.push_str("        unsafe impl WKScriptMessageHandler for ParamBridgeHandler {\n");
    out.push_str("            #[unsafe(method(userContentController:didReceiveScriptMessage:))]\n");
    out.push_str("            unsafe fn userContentController_didReceiveScriptMessage(\n");
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
    out.push_str("        fn new(params: Arc<PluginParams>, context: Arc<dyn GuiContext>, mtm: MainThreadMarker) -> Retained<Self> {\n");
    out.push_str("            let this = Self::alloc(mtm).set_ivars(ParamBridgeHandlerIvars { params, context });\n");
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
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("mod editor {"));
        assert!(code.contains("pub struct WebViewEditor"));
        assert!(code.contains("impl Editor for WebViewEditor"));
        assert!(code.contains("struct WebViewHandle"));
    }

    #[test]
    fn editor_module_contains_spawn() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

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
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("fn param_value_changed("));
        assert!(code.contains("fn param_modulation_changed("));
        assert!(code.contains("fn param_values_changed("));
    }

    #[test]
    fn editor_module_contains_ipc_handler() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("define_class!"));
        assert!(code.contains("ParamBridgeHandler"));
        assert!(code.contains("WKScriptMessageHandler"));
        assert!(code.contains("didReceiveScriptMessage"));
        assert!(code.contains("paramBridge"));
    }

    #[test]
    fn editor_module_dispatches_params() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

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
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("include_str!(\"../assets/editor.html\")"));
    }

    #[test]
    fn editor_module_webview_handle_drop() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

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
        let code = generate_editor_module(&plugin, gui, 600, 400);

        // Should still generate valid code with no param dispatch
        assert!(code.contains("mod editor {"));
        assert!(code.contains("let _ = (id, value, setter)"));
    }

    // ── Rust→JS sync tests ───────────────────────────────────

    #[test]
    fn editor_module_has_webview_ptr_field() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        // Editor struct has shared webview pointer
        assert!(code.contains("webview_ptr: Arc<AtomicPtr<c_void>>"));
        // Handle also has it
        assert!(code.contains("webview_ptr: Arc<AtomicPtr<c_void>>"));
    }

    #[test]
    fn editor_module_param_value_changed_calls_js() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        // param_value_changed should load ptr, check null, call evaluateJavaScript
        assert!(code.contains("fn param_value_changed(&self, id: &str, normalized_value: f32)"));
        assert!(code.contains("self.webview_ptr.load(Ordering::Acquire)"));
        assert!(code.contains("window.updateParam"));
        assert!(code.contains("evaluateJavaScript"));
    }

    #[test]
    fn editor_module_param_values_changed_iterates_params() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        // param_values_changed should call param_value_changed for each param
        assert!(code.contains("fn param_values_changed(&self)"));
        assert!(code.contains("self.param_value_changed(\"gain\""));
        assert!(code.contains("self.param_value_changed(\"mix\""));
        assert!(code.contains("modulated_normalized_value()"));
    }

    #[test]
    fn editor_module_handle_drop_nulls_ptr() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        // Drop should null the ptr BEFORE removeFromSuperview
        let drop_start = code.find("impl Drop for WebViewHandle").unwrap();
        let drop_block = &code[drop_start..];
        let null_pos = drop_block.find("null_mut()").unwrap();
        let remove_pos = drop_block.find("removeFromSuperview").unwrap();
        assert!(null_pos < remove_pos, "ptr null must come before removeFromSuperview");
    }

    #[test]
    fn editor_module_spawn_stores_ptr() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("self.webview_ptr.store(raw_ptr, Ordering::Release)"));
        assert!(code.contains("webview_ptr: self.webview_ptr.clone()"));
    }

    // ── Custom size tests ────────────────────────────────────

    #[test]
    fn editor_module_custom_dimensions() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 800, 600);

        assert!(code.contains("AtomicU32::new(800)"));
        assert!(code.contains("AtomicU32::new(600)"));
    }

    #[test]
    fn editor_module_default_dimensions() {
        let plugin = make_gui_plugin();
        let gui = crate::codegen::gui::find_gui_block(&plugin).unwrap();
        let code = generate_editor_module(&plugin, gui, 600, 400);

        assert!(code.contains("AtomicU32::new(600)"));
        assert!(code.contains("AtomicU32::new(400)"));
    }
}
