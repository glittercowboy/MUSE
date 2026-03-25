//! Safe wrapper around a loaded plugin dylib's C-ABI preview exports.
//!
//! `HostPlugin` loads a `.dylib` via `libloading`, resolves all
//! `muse_preview_*` symbols, and provides safe Rust methods for
//! creating the plugin instance, processing audio, and querying/setting
//! parameters. Drop calls `muse_preview_destroy` automatically.

use libloading::{Library, Symbol};
use std::path::Path;

/// A loaded plugin instance ready for audio processing.
///
/// Owns the `libloading::Library` handle and the opaque plugin pointer.
/// Drop calls `muse_preview_destroy` then closes the library.
pub struct HostPlugin {
    _lib: Library,
    instance: *mut u8,

    // Cached function pointers — resolved once at load time.
    fn_destroy: unsafe extern "C" fn(*mut u8),
    fn_process:
        unsafe extern "C" fn(*mut u8, *const *const f32, *mut *mut f32, u32, u32),
    fn_get_param_count: extern "C" fn() -> u32,
    fn_get_param_name: unsafe extern "C" fn(u32, *mut u8, u32) -> u32,
    fn_get_param_default: extern "C" fn(u32) -> f32,
    fn_set_param: unsafe extern "C" fn(*mut u8, u32, f32),
    fn_get_param: unsafe extern "C" fn(*mut u8, u32) -> f32,
    fn_get_num_channels: extern "C" fn() -> u32,
}

// SAFETY: The plugin instance pointer is only accessed from one thread at a
// time — the CPAL audio callback holds an exclusive &mut through the Mutex,
// and parameter access from the main thread also goes through the same Mutex.
unsafe impl Send for HostPlugin {}

impl HostPlugin {
    /// Load a plugin dylib, resolve all preview symbols, and create an instance.
    ///
    /// `sample_rate` is passed to `muse_preview_create` so the plugin
    /// initialises its smoothers and oscillators at the correct rate.
    pub fn load(dylib_path: &Path, sample_rate: f32) -> Result<Self, String> {
        // SAFETY: We trust the dylib was compiled from muse codegen and exports
        // the expected C-ABI symbols. libloading handles dlopen/dlsym.
        unsafe {
            let lib = Library::new(dylib_path)
                .map_err(|e| format!("failed to load {}: {e}", dylib_path.display()))?;

            // Resolve all symbols up front so any missing export fails fast.
            let fn_create: Symbol<unsafe extern "C" fn(f32) -> *mut u8> =
                lib.get(b"muse_preview_create")
                    .map_err(|e| format!("symbol muse_preview_create: {e}"))?;
            let fn_destroy: Symbol<unsafe extern "C" fn(*mut u8)> =
                lib.get(b"muse_preview_destroy")
                    .map_err(|e| format!("symbol muse_preview_destroy: {e}"))?;
            let fn_process: Symbol<
                unsafe extern "C" fn(*mut u8, *const *const f32, *mut *mut f32, u32, u32),
            > = lib
                .get(b"muse_preview_process")
                .map_err(|e| format!("symbol muse_preview_process: {e}"))?;
            let fn_get_param_count: Symbol<extern "C" fn() -> u32> = lib
                .get(b"muse_preview_get_param_count")
                .map_err(|e| format!("symbol muse_preview_get_param_count: {e}"))?;
            let fn_get_param_name: Symbol<unsafe extern "C" fn(u32, *mut u8, u32) -> u32> = lib
                .get(b"muse_preview_get_param_name")
                .map_err(|e| format!("symbol muse_preview_get_param_name: {e}"))?;
            let fn_get_param_default: Symbol<extern "C" fn(u32) -> f32> = lib
                .get(b"muse_preview_get_param_default")
                .map_err(|e| format!("symbol muse_preview_get_param_default: {e}"))?;
            let fn_set_param: Symbol<unsafe extern "C" fn(*mut u8, u32, f32)> = lib
                .get(b"muse_preview_set_param")
                .map_err(|e| format!("symbol muse_preview_set_param: {e}"))?;
            let fn_get_param: Symbol<unsafe extern "C" fn(*mut u8, u32) -> f32> = lib
                .get(b"muse_preview_get_param")
                .map_err(|e| format!("symbol muse_preview_get_param: {e}"))?;
            let fn_get_num_channels: Symbol<extern "C" fn() -> u32> = lib
                .get(b"muse_preview_get_num_channels")
                .map_err(|e| format!("symbol muse_preview_get_num_channels: {e}"))?;

            // Copy function pointers out of Symbols before moving `lib`.
            // *symbol dereferences the Symbol wrapper to get the raw fn pointer.
            let p_create = *fn_create;
            let p_destroy = *fn_destroy;
            let p_process = *fn_process;
            let p_get_param_count = *fn_get_param_count;
            let p_get_param_name = *fn_get_param_name;
            let p_get_param_default = *fn_get_param_default;
            let p_set_param = *fn_set_param;
            let p_get_param = *fn_get_param;
            let p_get_num_channels = *fn_get_num_channels;

            // Drop all Symbol borrows before moving lib.
            drop(fn_create);
            drop(fn_destroy);
            drop(fn_process);
            drop(fn_get_param_count);
            drop(fn_get_param_name);
            drop(fn_get_param_default);
            drop(fn_set_param);
            drop(fn_get_param);
            drop(fn_get_num_channels);

            // Create the plugin instance.
            let instance = p_create(sample_rate);
            if instance.is_null() {
                return Err("muse_preview_create returned null".into());
            }

            Ok(Self {
                _lib: lib,
                instance,
                fn_destroy: p_destroy,
                fn_process: p_process,
                fn_get_param_count: p_get_param_count,
                fn_get_param_name: p_get_param_name,
                fn_get_param_default: p_get_param_default,
                fn_set_param: p_set_param,
                fn_get_param: p_get_param,
                fn_get_num_channels: p_get_num_channels,
            })
        }
    }

    /// Process audio through the plugin.
    ///
    /// `outputs` is a slice of mutable channel buffers that will be filled
    /// with processed audio. For effect plugins, `inputs` provides the source
    /// audio; for instruments, `inputs` can be empty (silence is used).
    ///
    /// # Safety
    /// The caller must ensure buffer lengths are consistent: all channels in
    /// `inputs` and `outputs` must have the same length (`num_samples`).
    pub fn process(&self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) {
        if outputs.is_empty() {
            return;
        }
        let num_samples = outputs[0].len() as u32;
        let num_channels = outputs.len() as u32;

        // Build pointer arrays for the C ABI.
        let input_ptrs: Vec<*const f32> = inputs.iter().map(|ch| ch.as_ptr()).collect();
        let mut output_ptrs: Vec<*mut f32> = outputs.iter_mut().map(|ch| ch.as_mut_ptr()).collect();

        let inputs_ptr = if input_ptrs.is_empty() {
            std::ptr::null()
        } else {
            input_ptrs.as_ptr()
        };

        unsafe {
            (self.fn_process)(
                self.instance,
                inputs_ptr,
                output_ptrs.as_mut_ptr(),
                num_channels,
                num_samples,
            );
        }
    }

    /// Number of audio output channels the plugin expects.
    pub fn num_channels(&self) -> u32 {
        (self.fn_get_num_channels)()
    }

    /// Number of parameters exposed by the plugin.
    pub fn param_count(&self) -> u32 {
        (self.fn_get_param_count)()
    }

    /// Get the name of parameter at `index`. Returns empty string for invalid index.
    pub fn param_name(&self, index: u32) -> String {
        let mut buf = [0u8; 256];
        let len = unsafe {
            (self.fn_get_param_name)(index, buf.as_mut_ptr(), buf.len() as u32)
        };
        String::from_utf8_lossy(&buf[..len as usize]).into_owned()
    }

    /// Get the default value of parameter at `index`.
    pub fn param_default(&self, index: u32) -> f32 {
        (self.fn_get_param_default)(index)
    }

    /// Get the current value of parameter at `index`.
    pub fn get_param(&self, index: u32) -> f32 {
        unsafe { (self.fn_get_param)(self.instance, index) }
    }

    /// Set the value of parameter at `index`.
    pub fn set_param(&self, index: u32, value: f32) {
        unsafe { (self.fn_set_param)(self.instance, index, value) }
    }

    /// Snapshot all current parameter values as `(index, value)` pairs.
    /// Used to preserve state across hot-reload swaps.
    pub fn snapshot_params(&self) -> Vec<(u32, f32)> {
        let count = self.param_count();
        (0..count).map(|i| (i, self.get_param(i))).collect()
    }

    /// Restore parameter values from a snapshot. Matches by index — the caller
    /// should verify param counts match before calling.
    pub fn restore_params(&self, snapshot: &[(u32, f32)]) {
        for &(index, value) in snapshot {
            self.set_param(index, value);
        }
    }
}

impl Drop for HostPlugin {
    fn drop(&mut self) {
        if !self.instance.is_null() {
            unsafe {
                (self.fn_destroy)(self.instance);
            }
            self.instance = std::ptr::null_mut();
        }
        // _lib drops here, calling dlclose
    }
}
