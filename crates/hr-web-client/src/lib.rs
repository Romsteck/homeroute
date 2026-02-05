use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_islands();
}

// Ensure hr-web island components are linked into the WASM binary
#[allow(unused_imports)]
use hr_web;
