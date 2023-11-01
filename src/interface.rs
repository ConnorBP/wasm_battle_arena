use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "export function is_secure() { return window.location.protocol == 'https:'; }")]
extern "C" {
    pub fn is_secure() -> bool;
}