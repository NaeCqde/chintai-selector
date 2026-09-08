//! chintai-web — the Leptos CSR single-page app. Built to wasm32 with
//! `cargo build --target wasm32-unknown-unknown` + `wasm-bindgen`; served as
//! static files by `chintai-server`, which also hosts the JSON API it calls.

mod app;
mod maplibre;
mod net;
mod state;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
