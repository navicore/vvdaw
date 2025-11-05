//! VST3 COM interface helpers.
//!
//! This module provides safe wrappers around VST3's COM-style interfaces.
//! VST3 uses manual reference counting and vtable-based polymorphism,
//! similar to Microsoft COM.

use crate::ffi;
use vvdaw_plugin::PluginError;

/// Type alias for the `GetPluginFactory` function signature
///
/// The VST3 entry point that returns the plugin factory.
/// ```c
/// extern "C" IPluginFactory* PLUGIN_API GetPluginFactory();
/// ```
pub type GetPluginFactoryFn = unsafe extern "C" fn() -> *mut ffi::Steinberg_IPluginFactory;

/// Wrapper around a VST3 `IPluginFactory` pointer
///
/// Provides safe access to the plugin factory interface with automatic
/// reference counting.
pub struct PluginFactory {
    #[allow(dead_code)] // Will be used for vtable calls
    ptr: *mut ffi::Steinberg_IPluginFactory,
}

// SAFETY: VST3 interfaces are designed to be thread-safe and can be called
// from multiple threads. The `IPluginFactory` interface specifically is documented
// as thread-safe in the VST3 SDK. The pointer itself is opaque and managed by
// the plugin, which handles thread safety internally.
#[allow(unsafe_code)]
unsafe impl Send for PluginFactory {}

impl PluginFactory {
    /// Create a new `PluginFactory` from a raw pointer
    ///
    /// # Safety
    ///
    /// The pointer must be valid and properly reference counted.
    /// This function does NOT increment the reference count - the caller
    /// is responsible for ensuring proper ownership.
    #[allow(unsafe_code)]
    pub unsafe fn from_raw(ptr: *mut ffi::Steinberg_IPluginFactory) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    /// Get the number of classes exported by this factory
    ///
    /// # Errors
    ///
    /// Returns an error if the vtable call fails.
    #[allow(unsafe_code)]
    #[allow(unused)]
    #[allow(clippy::unused_self)] // Will be used for vtable call
    #[allow(clippy::unnecessary_wraps)] // Will return errors once implemented
    pub fn count_classes(&self) -> Result<i32, PluginError> {
        // TODO: Call the vtable function
        // For now, return a placeholder
        Ok(0)
    }

    /// Get information about a class by index
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds or the call fails.
    #[allow(unsafe_code)]
    #[allow(unused)]
    #[allow(clippy::unused_self)] // Will be used for vtable call
    pub fn get_class_info(&self, _index: i32) -> Result<ClassInfo, PluginError> {
        // TODO: Call the vtable function and parse PClassInfo
        Err(PluginError::FormatError(
            "get_class_info not yet implemented".to_string(),
        ))
    }
}

impl Drop for PluginFactory {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // TODO: Call Release() on the COM interface
        tracing::debug!("Dropping PluginFactory");
    }
}

/// Information about a plugin class
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub class_id: [u8; 16], // FUID (128-bit UUID)
    #[allow(dead_code)] // Will be used for plugin validation
    pub cardinality: i32,
    #[allow(dead_code)] // Will be used for plugin filtering
    pub category: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unsafe_code)] // Tests need unsafe for FFI testing
    fn test_null_factory() {
        let factory = unsafe { PluginFactory::from_raw(std::ptr::null_mut()) };
        assert!(factory.is_none());
    }
}
