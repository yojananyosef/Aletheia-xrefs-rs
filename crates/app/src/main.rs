#[cfg(target_arch = "wasm32")]
fn main() {
    bible_app::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("This app targets wasm32. Run: cd crates/app && trunk serve");
}
