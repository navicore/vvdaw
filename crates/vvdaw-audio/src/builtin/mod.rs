//! Built-in audio processors.
//!
//! These are simple, essential processors implemented directly in Rust.
//! They implement the `Plugin` trait just like external VST3/CLAP plugins,
//! but have zero overhead (no IPC, no FFI, just direct vtable dispatch).

pub mod gain;
pub mod mixer;
pub mod pan;

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
        "mixer" => Some(Box::new(mixer::MixerProcessor::default())),
        "pan" => Some(Box::new(pan::PanProcessor::default())),
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
    fn test_create_mixer() {
        let plugin = create_builtin("mixer");
        assert!(plugin.is_some());
    }

    #[test]
    fn test_create_pan() {
        let plugin = create_builtin("pan");
        assert!(plugin.is_some());
    }

    #[test]
    fn test_unknown_builtin() {
        let plugin = create_builtin("nonexistent");
        assert!(plugin.is_none());
    }
}
