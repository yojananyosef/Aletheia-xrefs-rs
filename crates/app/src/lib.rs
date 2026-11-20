#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod axis;
#[cfg(target_arch = "wasm32")]
mod i18n;
#[cfg(target_arch = "wasm32")]
mod render;
#[cfg(target_arch = "wasm32")]
mod threads;

#[cfg(target_arch = "wasm32")]
pub use app::start;
