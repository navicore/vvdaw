//! VST3 COM interface helpers.
//!
//! This module provides safe wrappers around VST3's COM-style interfaces.
//! VST3 uses manual reference counting and vtable-based polymorphism,
//! similar to Microsoft COM.
//!
//! ## Vtable Layout
//!
//! VST3 interfaces inherit from `FUnknown` which provides:
//! - vtable[0] = queryInterface
//! - vtable[1] = addRef
//! - vtable[2] = release
//!
//! `IPluginFactory` adds:
//! - vtable[3] = getFactoryInfo
//! - vtable[4] = countClasses
//! - vtable[5] = getClassInfo
//! - vtable[6] = createInstance

use crate::ffi;
use std::ffi::c_void;
use vvdaw_plugin::PluginError;

/// COM result type (tresult in VST3)
pub type TResult = i32;

/// COM success result
pub const K_RESULT_OK: TResult = 0;

/// Function pointer type for `IPluginFactory::countClasses`
///
/// Returns the number of classes exported by this factory.
type CountClassesFn = unsafe extern "C" fn(this: *mut c_void) -> i32;

/// Function pointer type for `IPluginFactory::getClassInfo`
///
/// Fills a `PClassInfo` structure with information about the class at the specified index.
type GetClassInfoFn = unsafe extern "C" fn(
    this: *mut c_void,
    index: i32,
    info: *mut ffi::Steinberg_PClassInfo,
) -> TResult;

/// Function pointer type for `FUnknown::release`
///
/// Decrements the reference count and destroys the object if it reaches zero.
type ReleaseFn = unsafe extern "C" fn(this: *mut c_void) -> u32;

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
    #[allow(unsafe_code)]
    pub fn count_classes(&self) -> i32 {
        unsafe {
            // Get the vtable pointer from the interface
            let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

            // countClasses is at vtable[4] (after queryInterface, addRef, release, getFactoryInfo)
            let count_classes_ptr = *vtable_ptr.add(4);
            let count_classes_fn: CountClassesFn = std::mem::transmute(count_classes_ptr);

            // Call the function
            count_classes_fn(self.ptr.cast::<c_void>())
        }
    }

    /// Get information about a class by index
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds or the call fails.
    #[allow(unsafe_code)]
    pub fn get_class_info(&self, index: i32) -> Result<ClassInfo, PluginError> {
        unsafe {
            // Allocate space for the result
            let mut class_info: ffi::Steinberg_PClassInfo = std::mem::zeroed();

            // Get the vtable pointer
            let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

            // getClassInfo is at vtable[5]
            let get_class_info_ptr = *vtable_ptr.add(5);
            let get_class_info_fn: GetClassInfoFn = std::mem::transmute(get_class_info_ptr);

            // Call the function
            let result = get_class_info_fn(self.ptr.cast::<c_void>(), index, &raw mut class_info);

            if result != K_RESULT_OK {
                return Err(PluginError::FormatError(format!(
                    "getClassInfo failed with result: {result}"
                )));
            }

            // Convert the C struct to our Rust struct
            Ok(Self::convert_class_info(&class_info))
        }
    }

    /// Convert a VST3 `PClassInfo` to our `ClassInfo` struct
    ///
    /// # Safety
    ///
    /// The `PClassInfo` must be a valid, initialized struct from VST3.
    #[allow(unsafe_code)]
    fn convert_class_info(info: &ffi::Steinberg_PClassInfo) -> ClassInfo {
        unsafe {
            // Extract the class ID (TUID is a 16-byte array of i8, convert to u8)
            let mut class_id = [0u8; 16];
            for (i, &byte) in info.cid.iter().enumerate() {
                class_id[i] = byte as u8;
            }

            // Convert category and name from C strings
            let category = std::ffi::CStr::from_ptr(info.category.as_ptr())
                .to_string_lossy()
                .into_owned();

            let name = std::ffi::CStr::from_ptr(info.name.as_ptr())
                .to_string_lossy()
                .into_owned();

            ClassInfo {
                class_id,
                cardinality: info.cardinality,
                category,
                name,
            }
        }
    }
}

impl Drop for PluginFactory {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                // Get the vtable pointer
                let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

                // release() is at vtable[2] (after queryInterface, addRef)
                let release_ptr = *vtable_ptr.add(2);
                let release_fn: ReleaseFn = std::mem::transmute(release_ptr);

                // Call release - this decrements ref count and may destroy the object
                let ref_count = release_fn(self.ptr.cast::<c_void>());
                tracing::debug!("Released PluginFactory, ref count now: {}", ref_count);
            }
        }
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
