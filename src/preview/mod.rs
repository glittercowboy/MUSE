//! Live-preview host: loads a compiled plugin dylib via libloading,
//! drives audio through CPAL, and supports hot-swap reloading.
//!
//! macOS-only (gated by `cfg(target_os = "macos")`).

#[cfg(target_os = "macos")]
pub mod host_plugin;

#[cfg(target_os = "macos")]
pub mod audio;

#[cfg(target_os = "macos")]
pub mod watcher;

#[cfg(target_os = "macos")]
pub mod reload;
