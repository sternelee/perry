//! WebAssembly code generation backend for Perry
//!
//! Compiles HIR modules to WebAssembly binary format for `--target wasm`.
//! Produces a self-contained HTML file with embedded WASM (base64) and JS runtime bridge.
//!
//! All JSValues use NaN-boxing (f64) consistent with perry-runtime.
//! Runtime operations (strings, console, objects) are imported from JavaScript.

pub mod emit;

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use perry_hir::ir::Module;

/// Embedded WASM runtime JavaScript (bridge between WASM and browser APIs)
const WASM_RUNTIME_JS: &str = include_str!("wasm_runtime.js");

/// Compile multiple HIR modules into a self-contained HTML file with embedded WASM.
pub fn compile_modules_to_wasm_html(
    modules: &[(String, Module)],
    title: &str,
    minify: bool,
) -> Result<String> {
    let output = emit::compile_to_wasm_with_async(modules);
    let wasm_b64 = BASE64.encode(&output.wasm_bytes);

    let runtime_js = if minify {
        perry_codegen_js::minify::minify_js(WASM_RUNTIME_JS)
    } else {
        WASM_RUNTIME_JS.to_string()
    };

    // If there are async functions, inject them into the runtime
    let async_inject = if output.async_js.is_empty() {
        String::new()
    } else {
        format!("\n// === Generated async function implementations ===\nconst __asyncFuncImpls = {{\n{}\n}};\n", output.async_js)
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title}</title>
  <style>
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    html, body {{ width: 100vw; height: 100vh; overflow: hidden; }}
    #perry-root {{ width: 100%; flex: 1 1 0%; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }}
  </style>
</head>
<body>
  <div id="perry-root"></div>
  <script>
{runtime_js}{async_inject}
  </script>
  <script>
bootPerryWasm("{wasm_b64}");
  </script>
</body>
</html>"#,
        title = html_escape(title),
        runtime_js = runtime_js,
        async_inject = async_inject,
        wasm_b64 = wasm_b64,
    );

    Ok(html)
}

/// Get the raw WASM binary (for non-HTML output)
pub fn compile_modules_to_wasm(modules: &[(String, Module)]) -> Result<Vec<u8>> {
    Ok(emit::compile_to_wasm(modules))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
