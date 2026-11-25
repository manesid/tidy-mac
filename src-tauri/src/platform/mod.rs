//! Everything OS-specific lives behind this module. Add `windows.rs` / `linux.rs` here later.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
