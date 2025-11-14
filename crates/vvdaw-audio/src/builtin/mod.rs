//! Built-in audio processors.
//!
//! These are simple, essential processors implemented directly in Rust.
//! They implement the `Plugin` trait just like external VST3/CLAP plugins,
//! but have zero overhead (no IPC, no FFI, just direct vtable dispatch).

pub mod gain;

use vvdaw_plugin::Plugin;

/// Create a built-in processor by name
///
/// Returns `None` if the name doesn't match any known built-in processor.
///
/// # Examples
///
/// ```
/// use vvdaw_audio::builtin;
///
/// let gain = builtin::create_builtin("gain").expect("gain processor exists");
/// ```
pub fn create_builtin(name: &str) -> Option<Box<dyn Plugin>> {
    match name {
        "gain" => Some(Box::new(gain::GainProcessor::default())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_gain() {
        let plugin = create_builtin("gain");
        assert!(plugin.is_some());
    }

    #[test]
    fn test_unknown_builtin() {
        let plugin = create_builtin("nonexistent");
        assert!(plugin.is_none());
    }
}
