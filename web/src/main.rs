//! Client Leptos. Só roda no navegador (wasm32): `trunk serve` dentro de `web/`.
//!
//! No alvo nativo o crate compila só este `main`, para `cargo test --workspace` continuar
//! funcionando sem os módulos que dependem do navegador.

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod gateway;
#[cfg(target_arch = "wasm32")]
mod store;
#[cfg(target_arch = "wasm32")]
mod views;

#[cfg(target_arch = "wasm32")]
fn main() {
    app::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("O client roda no navegador. Rode `trunk serve` dentro de web/.");
}
