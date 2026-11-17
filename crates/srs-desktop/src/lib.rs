//! Desktop-only application adapter. The shared audio/network crates have no UI dependency.
mod app;
mod devices;
mod endpoint;
mod input;
mod keyboard;
mod profiles;
mod session;
#[cfg(all(test, target_os = "linux"))]
mod test_relay;
// Slint generates toolkit-owned object/vtable glue. No hand-written unsafe is
// permitted in the rest of this crate; no native device handles cross this API.
#[allow(unsafe_code)]
pub mod ui {
    slint::include_modules!();
}
pub use app::run;
